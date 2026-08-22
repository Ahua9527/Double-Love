pub mod framing;
pub mod protocol;

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use double_love_desktop_service::{
    DesktopEventSink, DesktopRuntimeConfig, DesktopService, DesktopServiceError, HOST_UNAVAILABLE,
    INTERNAL, INVALID_PARAMS,
};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::framing::{FrameReadError, read_frame, write_frame};
use crate::protocol::{
    HostEvent, HostRequest, HostRequestMethod, HostResponse, HostResult, PROTOCOL_VERSION,
    UNKNOWN_REQUEST_ID, hello,
};

#[derive(Debug, Error)]
pub enum HostRuntimeError {
    #[error("host input/output failed: {0}")]
    Io(#[from] io::Error),
    #[error("desktop service could not start: {0}")]
    Service(#[from] DesktopServiceError),
}

pub struct HostEventSink<W> {
    writer: Mutex<W>,
}

impl<W> HostEventSink<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }

    pub fn into_inner(self) -> Result<W, DesktopServiceError> {
        self.writer
            .into_inner()
            .map_err(|_| DesktopServiceError::internal("host stdout lock is unavailable"))
    }
}

impl<W: Write> HostEventSink<W> {
    fn write_serializable(&self, value: &impl Serialize) -> Result<(), DesktopServiceError> {
        let body = serde_json::to_vec(value).map_err(|error| {
            DesktopServiceError::new(
                INTERNAL,
                format!("host frame serialization failed: {error}"),
            )
        })?;
        let mut writer = self.writer.lock().map_err(|_| {
            DesktopServiceError::new(HOST_UNAVAILABLE, "host stdout lock is unavailable")
        })?;
        write_frame(&mut *writer, &body).map_err(|error| {
            DesktopServiceError::new(
                HOST_UNAVAILABLE,
                format!("host stdout write failed: {error}"),
            )
        })
    }
}

impl<W: Write + Send> DesktopEventSink for HostEventSink<W> {
    fn emit(&self, channel: &str, payload: Value) -> Result<(), DesktopServiceError> {
        self.write_serializable(&HostEvent::new(channel, payload))
    }
}

#[derive(Debug, Clone, Default)]
pub struct HostRuntimeConfig {
    pub resource_dir: Option<PathBuf>,
    pub test_transcribe_mock: bool,
    pub test_speaker_mock: bool,
}

pub fn run_host<W: Write + Send + 'static>(
    reader: &mut impl Read,
    writer: W,
    app_data_dir: Option<PathBuf>,
) -> Result<(), HostRuntimeError> {
    run_host_with_config(reader, writer, app_data_dir, HostRuntimeConfig::default())
}

pub fn run_host_with_config<W: Write + Send + 'static>(
    reader: &mut impl Read,
    writer: W,
    app_data_dir: Option<PathBuf>,
    runtime: HostRuntimeConfig,
) -> Result<(), HostRuntimeError> {
    let output = Arc::new(HostEventSink::new(writer));
    let mut registry = double_love_desktop_service::CommandRegistry::new();
    double_love_desktop_service::register_commands(&mut registry);
    let service = DesktopService::with_registry_and_runtime(
        app_data_dir,
        output.clone(),
        registry,
        DesktopRuntimeConfig {
            resource_dir: runtime.resource_dir,
            test_transcribe_mock: runtime.test_transcribe_mock,
            test_speaker_mock: runtime.test_speaker_mock,
        },
    )?;

    loop {
        let frame = match read_frame(reader) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(FrameReadError::TooLarge { declared, maximum }) => {
                write_response(
                    output.as_ref(),
                    &HostResponse::error(
                        UNKNOWN_REQUEST_ID,
                        "FRAME_TOO_LARGE",
                        format!("frame declares {declared} bytes; maximum is {maximum}"),
                    ),
                )?;
                return Ok(());
            }
            Err(FrameReadError::Io(error)) => return Err(HostRuntimeError::Io(error)),
        };

        let request = match parse_request(&frame) {
            Ok(request) => request,
            Err(response) => {
                write_response(output.as_ref(), &response)?;
                continue;
            }
        };

        let (response, shutdown) = handle_request(&service, request);
        write_response(output.as_ref(), &response)?;
        if shutdown {
            return Ok(());
        }
    }
}

fn parse_request(frame: &[u8]) -> Result<HostRequest, HostResponse> {
    let value: Value = serde_json::from_slice(frame).map_err(|error| {
        HostResponse::error(
            UNKNOWN_REQUEST_ID,
            "MALFORMED_JSON",
            format!("request is not valid JSON: {error}"),
        )
    })?;
    let request_id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(UNKNOWN_REQUEST_ID)
        .to_string();
    let has_string_id = value.get("id").is_some_and(Value::is_string);

    let version = value
        .get("v")
        .cloned()
        .and_then(|version| serde_json::from_value::<u32>(version).ok());
    if version != Some(PROTOCOL_VERSION) {
        return Err(HostResponse::error(
            request_id,
            "PROTOCOL_VERSION_MISMATCH",
            format!("request protocol version must be {PROTOCOL_VERSION}"),
        ));
    }

    if !has_string_id || request_id.trim().is_empty() {
        return Err(HostResponse::error(
            request_id,
            "INVALID_REQUEST",
            "request id must be a non-blank string",
        ));
    }

    if let Some(method) = value.get("method").and_then(Value::as_str) {
        if !matches!(method, "handshake" | "health" | "shutdown" | "invoke") {
            return Err(HostResponse::error(
                request_id,
                "UNKNOWN_METHOD",
                format!("unknown host method: {method}"),
            ));
        }
        if method == "invoke"
            && (value
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(|name| name.trim().is_empty())
                || value.get("payload").is_none())
        {
            return Err(HostResponse::error(
                request_id,
                INVALID_PARAMS,
                "invoke requires a non-blank string name and a payload",
            ));
        }
    }

    serde_json::from_value(value).map_err(|error| {
        HostResponse::error(
            request_id,
            "INVALID_REQUEST",
            format!("request does not match the host protocol: {error}"),
        )
    })
}

fn handle_request(service: &DesktopService, request: HostRequest) -> (HostResponse, bool) {
    let HostRequest { id, method, .. } = request;
    match method {
        HostRequestMethod::Handshake {
            client: _,
            client_protocol,
        } if client_protocol != PROTOCOL_VERSION => (
            HostResponse::error(
                id,
                "PROTOCOL_VERSION_MISMATCH",
                format!(
                    "client protocol {client_protocol} does not match host protocol {PROTOCOL_VERSION}"
                ),
            ),
            false,
        ),
        HostRequestMethod::Handshake { .. } => {
            (HostResponse::ok(id, HostResult::Hello(hello())), false)
        }
        HostRequestMethod::Health => (
            HostResponse::ok(id, HostResult::Health { healthy: true }),
            false,
        ),
        HostRequestMethod::Shutdown => (HostResponse::ok(id, HostResult::Shutdown), true),
        HostRequestMethod::Invoke { name, payload } => {
            let response = match service.invoke(&name, payload) {
                Ok(result) => HostResponse::ok(id, HostResult::Invoke(result)),
                Err(error) => HostResponse::error(id, error.code, error.message),
            };
            (response, false)
        }
    }
}

fn write_response<W: Write>(
    output: &HostEventSink<W>,
    response: &HostResponse,
) -> Result<(), HostRuntimeError> {
    output.write_serializable(response).map_err(|error| {
        HostRuntimeError::Io(io::Error::other(format!(
            "{}: {}",
            error.code, error.message
        )))
    })
}
