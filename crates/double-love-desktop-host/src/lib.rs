pub mod framing;
pub mod protocol;

use std::io::{self, Read, Write};

use serde_json::Value;
use thiserror::Error;

use crate::framing::{FrameReadError, read_frame, write_frame};
use crate::protocol::{
    HostRequest, HostRequestMethod, HostResponse, HostResult, PROTOCOL_VERSION, UNKNOWN_REQUEST_ID,
    hello,
};

#[derive(Debug, Error)]
pub enum HostRuntimeError {
    #[error("host input/output failed: {0}")]
    Io(#[from] io::Error),
    #[error("host could not serialize a response: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub fn run_host(reader: &mut impl Read, writer: &mut impl Write) -> Result<(), HostRuntimeError> {
    loop {
        let frame = match read_frame(reader) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(FrameReadError::TooLarge { declared, maximum }) => {
                write_response(
                    writer,
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
                write_response(writer, &response)?;
                continue;
            }
        };

        let (response, shutdown) = handle_request(request);
        write_response(writer, &response)?;
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

    if let Some(method) = value.get("method").and_then(Value::as_str)
        && !matches!(method, "handshake" | "health" | "shutdown")
    {
        return Err(HostResponse::error(
            request_id,
            "UNKNOWN_METHOD",
            format!("unknown host method: {method}"),
        ));
    }

    serde_json::from_value(value).map_err(|error| {
        HostResponse::error(
            request_id,
            "INVALID_REQUEST",
            format!("request does not match the host protocol: {error}"),
        )
    })
}

fn handle_request(request: HostRequest) -> (HostResponse, bool) {
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
    }
}

fn write_response(
    writer: &mut impl Write,
    response: &HostResponse,
) -> Result<(), HostRuntimeError> {
    let body = serde_json::to_vec(response)?;
    write_frame(writer, &body)?;
    Ok(())
}
