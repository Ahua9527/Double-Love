use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const PROTOCOL_VERSION: u32 = 1;
pub const CAPABILITIES: &[&str] = &["handshake", "health", "shutdown"];
pub const UNKNOWN_REQUEST_ID: &str = "unknown";

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum HostRequestMethod {
    Handshake {
        client: String,
        client_protocol: u32,
    },
    Health,
    Shutdown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[ts(export, export_to = "host-protocol/")]
pub struct HostRequest {
    #[schemars(range(min = 1, max = 1))]
    pub v: u32,
    #[schemars(length(min = 1))]
    pub id: String,
    #[serde(flatten)]
    #[ts(flatten)]
    pub method: HostRequestMethod,
}

impl HostRequest {
    pub fn new(id: impl Into<String>, method: HostRequestMethod) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            method,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[ts(export, export_to = "host-protocol/")]
pub struct HostHello {
    pub protocol: u32,
    pub host_version: String,
    pub engine_version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[ts(export, export_to = "host-protocol/")]
pub enum HostResult {
    Hello(HostHello),
    Health { healthy: bool },
    Shutdown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[ts(export, export_to = "host-protocol/")]
pub struct HostProtocolError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HostResponseStatus {
    Ok { result: HostResult },
    Error { error: HostProtocolError },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, JsonSchema, TS)]
#[ts(export, export_to = "host-protocol/")]
pub struct HostResponse {
    #[schemars(range(min = 1, max = 1))]
    pub v: u32,
    pub id: String,
    #[serde(flatten)]
    #[ts(flatten)]
    pub response: HostResponseStatus,
}

impl HostResponse {
    pub fn ok(id: impl Into<String>, result: HostResult) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            response: HostResponseStatus::Ok { result },
        }
    }

    pub fn error(
        id: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: id.into(),
            response: HostResponseStatus::Error {
                error: HostProtocolError {
                    code: code.into(),
                    message: message.into(),
                },
            },
        }
    }
}

pub fn hello() -> HostHello {
    HostHello {
        protocol: PROTOCOL_VERSION,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
        engine_version: double_love_engine::ENGINE_VERSION.to_string(),
        capabilities: CAPABILITIES
            .iter()
            .map(|capability| (*capability).to_string())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs;
    use std::path::Path;

    use schemars::{JsonSchema, schema_for};

    use super::{HostHello, HostProtocolError, HostRequest, HostResponse, HostResult};

    fn write_schema<T: JsonSchema>(directory: &Path, name: &str) -> Result<(), Box<dyn Error>> {
        let mut json = serde_json::to_string_pretty(&schema_for!(T))?;
        json.push('\n');
        fs::write(directory.join(name), json)?;
        Ok(())
    }

    #[test]
    fn export_bindings_and_schemas() -> Result<(), Box<dyn Error>> {
        let directory =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings/host-protocol/schema");
        fs::create_dir_all(&directory)?;

        write_schema::<HostRequest>(&directory, "HostRequest.schema.json")?;
        write_schema::<HostHello>(&directory, "HostHello.schema.json")?;
        write_schema::<HostResult>(&directory, "HostResult.schema.json")?;
        write_schema::<HostProtocolError>(&directory, "HostProtocolError.schema.json")?;
        write_schema::<HostResponse>(&directory, "HostResponse.schema.json")?;
        Ok(())
    }
}
