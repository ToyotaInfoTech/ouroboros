// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use erased_serde::Serialize as ErasedSerialize;
use ouro_common::io::{FromCbor, ToCbor};
use ouro_state::daemon::DaemonSpec;
use serde::{Deserialize, Serialize};

use crate::{
    rpc,
    schema::shadow::{report::SystemInfo, EdgeShadow},
    Void,
};

rpc! {
    Cli,
    Version(VersionReq) -> VersionResp,
    Echo(EchoReq) -> EchoResp,
    Show(ShowReq) -> ShowResp,
    List(Void) -> ListResp,
    Describe(DescribeReq) -> DescribeResp,
    Operate(OperateReq) -> OperateResp
}

impl ToCbor for CliRequest {}
impl FromCbor for CliRequest {}
impl ToCbor for CliResponse {}
impl FromCbor for CliResponse {}

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionReq {
    version: String,
}

impl VersionReq {
    pub fn version(&self) -> &str {
        &self.version
    }
}

const PROTOCOL_VERSION: &str = "0.1";

impl Default for VersionReq {
    fn default() -> Self {
        Self {
            version: format!("{}-{}", env!("CARGO_PKG_VERSION"), PROTOCOL_VERSION),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct VersionResp {
    pub result: VersionResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum VersionResult {
    VersionOk { version: String },
    VersionMismatch { message: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EchoReq {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EchoResp {
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowReq {
    pub target: ShowTargetKind,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ShowTargetKind {
    State,
    Runtime,
    Edge,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowResp {
    pub target: ShowTarget,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ShowTarget {
    State(Box<DaemonSpec>),
    Runtime(SystemInfo),
    Edge(Vec<EdgeShadow>),
}

impl ShowTarget {
    /// Get a boxed reference to the inner value as an erased Serialize trait object
    pub fn as_serialize(&self) -> Box<dyn ErasedSerialize + '_> {
        match self {
            ShowTarget::State(state) => Box::new(state) as Box<dyn ErasedSerialize>,
            ShowTarget::Runtime(runtime) => Box::new(runtime) as Box<dyn ErasedSerialize>,
            ShowTarget::Edge(edge) => Box::new(edge) as Box<dyn ErasedSerialize>,
        }
    }
}

impl ShowResp {
    /// Get a boxed reference to the target as an erased Serialize trait object
    pub fn as_serialize(&self) -> Box<dyn ErasedSerialize + '_> {
        self.target.as_serialize()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListResp {
    pub tasks: Vec<TaskInfo>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct TaskInfo {
    pub name: String,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DescribeReq {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DescribeResp {
    pub descriptions: Vec<Description>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Description {
    pub operation_name: String,
    pub description: String,
    pub inputs: HashMap<String, String>,
    pub output: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OperateReq {
    pub name: String,
    pub operation_name: String,
    pub inputs: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OperateResp {
    pub output: serde_value::Value,
}

#[derive(Debug, Clone)]
pub struct Error(pub String);

/// Build an error for an unsupported operation, deriving known operation names
/// from the canonical `Description` list produced by `operation_descriptions!`.
///
/// Suggests a similarly-named operation when one is close enough by edit distance.
///
/// # Arguments
/// * `operation_name` - The unrecognized operation name
/// * `descriptions` - Canonical list of supported operations
///
/// # Returns
/// An `anyhow::Error` with a "Did you mean?" suggestion when applicable.
pub fn unsupported_operation(operation_name: &str, descriptions: &[Description]) -> anyhow::Error {
    let op_names: Vec<&str> = descriptions
        .iter()
        .map(|d| d.operation_name.as_str())
        .collect();
    ouro_common::suggest::unsupported_operation_error(operation_name, &op_names)
}

/// Extract a required input value from the request, with helpful error messages.
///
/// If `key` is not found in `inputs`, the error message includes a suggestion when
/// a similarly-named key was provided (likely a typo), or lists the expected inputs
/// for the operation.
///
/// # Arguments
/// * `inputs` - The user-supplied input map
/// * `key` - The required input key
/// * `operation_name` - Current operation name (for error context)
/// * `descriptions` - Canonical list of supported operations
///
/// # Returns
/// The input value as `&str`, or an error with suggestions.
pub fn require_input<'a>(
    inputs: &'a HashMap<String, String>,
    key: &str,
    operation_name: &str,
    descriptions: &[Description],
) -> anyhow::Result<&'a str> {
    match inputs.get(key) {
        Some(v) => Ok(v.as_str()),
        None => {
            let provided_keys: Vec<&str> = inputs.keys().map(|k| k.as_str()).collect();
            if let Some(similar) = ouro_common::suggest::find_closest(key, &provided_keys, 3) {
                Err(anyhow::anyhow!(
                    "Missing input '{}' for operation '{}'. Did you provide '{}' instead?",
                    key,
                    operation_name,
                    similar,
                ))
            } else {
                let expected = descriptions
                    .iter()
                    .find(|d| d.operation_name == operation_name)
                    .map(|d| {
                        let mut keys: Vec<&str> = d.inputs.keys().map(|k| k.as_str()).collect();
                        keys.sort_unstable();
                        keys.join(", ")
                    })
                    .unwrap_or_default();
                if expected.is_empty() {
                    Err(anyhow::anyhow!(
                        "Missing input '{}' for operation '{}'",
                        key,
                        operation_name,
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "Missing input '{}' for operation '{}'. Expected inputs: [{}]",
                        key,
                        operation_name,
                        expected,
                    ))
                }
            }
        }
    }
}

/// Inter-Component Communication (ICC) schema definitions.
///
/// Defines the request and response types for ICC messages between components.
/// The daemon uses ICC channels as an opaque forwarding mechanism — it never
/// inspects the request/response payloads, keeping task-specific logic entirely
/// within each task implementation.
pub mod icc {
    use tokio::sync::oneshot::Sender;

    use crate::{
        icc,
        schema::cli::{DescribeResp, ListResp, OperateReq, OperateResp},
        Void,
    };

    icc! {
        Icc,
        List(Void) -> ListResp,
        Describe(Void) -> DescribeResp,
        Operate(OperateReq) -> OperateResp
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::Description;

    #[test]
    fn operation_descriptions_use_doc_comments() {
        let descriptions = crate::operation_descriptions![
            /// First line.
            /// Second line.
            "demo" {
                inputs {
                    /// Identifier of the target.
                    "id",
                    /// Optional mode.
                    /// Defaults to `fast`.
                    "mode",
                }
                output {
                    /// Result payload in CBOR.
                }
            }
        ];

        assert_eq!(descriptions.len(), 1);

        let description: &Description = &descriptions[0];
        assert_eq!(description.operation_name, "demo");
        assert_eq!(description.description, "First line.\nSecond line.");
        assert_eq!(
            description.inputs,
            HashMap::from([
                ("id".to_string(), "Identifier of the target.".to_string()),
                (
                    "mode".to_string(),
                    "Optional mode.\nDefaults to `fast`.".to_string(),
                ),
            ])
        );
        assert_eq!(description.output, "Result payload in CBOR.");
    }
}
