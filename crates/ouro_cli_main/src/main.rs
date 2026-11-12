// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Ouroboros CLI binary (`ourocli`).
//!
//! Provides command-line access to daemon operations over Unix Domain Sockets
//! or TCP. Subcommands map to RPC methods defined in the `rpc!` macro.

use clap::{self, Parser, Subcommand, ValueEnum};
use ouro_core::{
    comm::{net::TransportStream, rpc::client::Client},
    error::{panic_if_env_set, InternalErrorKind},
    setup_tracing, InternalResult,
};
use ouro_schema::{
    schema::cli::{self, CliRequest, CliResponse},
    Void,
};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};
use tokio::net::UnixStream;
use tracing::instrument;

#[derive(Debug, Clone, clap::Parser)]
#[clap(
        name = env!("CARGO_PKG_NAME"),
        version = env!("CARGO_PKG_VERSION"),
        author = env!("CARGO_PKG_AUTHORS"),
        about = env!("CARGO_PKG_DESCRIPTION"),
        arg_required_else_help = true,
        long_about = None
    )]
struct Args {
    /// socket address to connect to
    #[clap(
        short = 's',
        long = "sock",
        default_value = "unix:/tmp/ourodaemon.sock"
    )]
    socket: String,

    /// sub-commands
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Show current state of the daemon
    Show {
        #[arg(short = 'f', long, required = false, default_value = "json")]
        format: Format,

        #[arg(short = 't', long, required = false, default_value = "state")]
        target: Target,
    },
    /// Echo a message through the daemon
    Echo {
        #[arg(short = 'f', long, required = false, default_value = "json")]
        format: Format,

        #[arg(
            short = 'm',
            long,
            required = false,
            default_value = "Hello, Ouroboros!"
        )]
        message: String,
    },
    List {
        #[arg(short = 'f', long, required = false, default_value = "json")]
        format: Format,
    },
    Describe {
        #[arg(short = 'f', long, required = false, default_value = "json")]
        format: Format,

        #[arg(short = 'n', long, required = true)]
        name: String,
    },
    /// Execute an operation on a specific task
    Operate {
        #[arg(short = 'f', long, required = false, default_value = "json")]
        format: Format,

        /// Target task name
        #[arg(short = 'n', long, required = true)]
        name: String,

        /// Operation name to execute
        #[arg(short = 'o', long, required = true)]
        operation: String,

        /// Input key=value pairs
        #[arg(short = 'i', long, value_parser = parse_key_val, num_args = 0..)]
        input: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum Format {
    Json,
    Toml,
}

#[derive(Debug, Clone, ValueEnum)]
enum Target {
    State,
    Runtime,
}

impl From<Target> for CliRequest {
    fn from(value: Target) -> Self {
        match value {
            Target::State => CliRequest::Show(cli::ShowReq {
                target: cli::ShowTargetKind::State,
            }),
            Target::Runtime => CliRequest::Show(cli::ShowReq {
                target: cli::ShowTargetKind::Runtime,
            }),
        }
    }
}

/// Parses a `key=value` string into a tuple.
///
/// If the value starts with `file://`, the referenced file is read and
/// converted to a JSON string:
/// - `.toml` files are parsed as TOML, then serialized to JSON.
/// - `.json` files are parsed and re-serialized as compact JSON.
/// - Other values are used as-is.
fn parse_key_val(s: &str) -> Result<(String, String), String> {
    let pos = s
        .find('=')
        .ok_or_else(|| format!("invalid KEY=VALUE: no '=' found in '{s}'"))?;
    let key = s[..pos].to_string();
    let raw_value = &s[pos + 1..];

    let value = if let Some(path) = raw_value.strip_prefix("file://") {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read file '{}': {}", path, e))?;
        if path.ends_with(".toml") {
            let toml_val: toml::Value = content
                .parse()
                .map_err(|e| format!("failed to parse TOML '{}': {}", path, e))?;
            serde_json::to_string(&toml_val)
                .map_err(|e| format!("failed to convert TOML to JSON: {}", e))?
        } else if path.ends_with(".json") {
            let json_val: serde_json::Value = serde_json::from_str(&content)
                .map_err(|e| format!("failed to parse JSON '{}': {}", path, e))?;
            serde_json::to_string(&json_val)
                .map_err(|e| format!("failed to re-serialize JSON: {}", e))?
        } else {
            return Err(format!(
                "unsupported file extension for '{}': expected .toml or .json",
                path
            ));
        }
    } else {
        raw_value.to_string()
    };

    Ok((key, value))
}

#[tokio::main]
async fn main() {
    setup_tracing();
    if let Err(e) = run().await {
        panic_if_env_set(&e);
    }
}

async fn create_stream(sock: String) -> InternalResult<Box<dyn TransportStream>> {
    if let Some(path) = sock.strip_prefix("unix:") {
        let path = PathBuf::from(path);
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| (format!("Failed to connect to socket: {}", sock), e).into())?;
        Ok(Box::new(stream) as Box<dyn TransportStream>)
    } else if let Some(addr) = sock.strip_prefix("tcp:") {
        let stream = tokio::net::TcpStream::connect(addr)
            .await
            .map_err(|e| (format!("Failed to connect to TCP address: {}", addr), e).into())?;
        Ok(Box::new(stream) as Box<dyn TransportStream>)
    } else {
        Err((
            "Unsupported socket type, only tcp sockets or unix domain sockets are supported",
            InternalErrorKind::InvalidConfig,
        )
            .into())
    }
}

#[allow(clippy::result_large_err)]
fn print_output<T: Serialize>(format: Format, output: &T) -> InternalResult<()> {
    match format {
        Format::Json => {
            let json_str = serde_json::to_string_pretty(output)
                .map_err(|e| ("Failed to serialize to JSON: {}", e).into())?;
            println!("{}", json_str);
        }
        Format::Toml => {
            // TOML does not support top-level arrays; detect them before attempting serialization.
            let json_value = serde_json::to_value(output)
                .map_err(|e| ("Failed to serialize output", e).into())?;
            if json_value.is_array() {
                return Err((
                    "TOML format does not support top-level arrays. Use '--format json' instead.",
                    InternalErrorKind::IllegalArgument,
                )
                    .into());
            }
            let toml_str = toml::to_string(&json_value)
                .map_err(|e| ("Failed to serialize to TOML", e).into())?;
            println!("{}", toml_str);
        }
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn print_output_erased(format: Format, output: &dyn erased_serde::Serialize) -> InternalResult<()> {
    match format {
        Format::Json => {
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::pretty(&mut buf);
            erased_serde::serialize(output, &mut serializer).map_err(|e| {
                (
                    format!("Failed to serialize to JSON: {}", e),
                    InternalErrorKind::Unknown(Box::new(e)),
                )
                    .into()
            })?;
            let json_str = String::from_utf8(buf).map_err(|e| {
                (
                    format!("Invalid UTF-8 in JSON output: {}", e),
                    InternalErrorKind::Unknown(Box::new(e)),
                )
                    .into()
            })?;
            println!("{}", json_str);
        }
        Format::Toml => {
            // Serialize via JSON Value as intermediary since toml doesn't directly support erased_serde
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut buf);
            erased_serde::serialize(output, &mut serializer).map_err(|e| {
                (
                    format!("Failed to serialize: {}", e),
                    InternalErrorKind::Unknown(Box::new(e)),
                )
                    .into()
            })?;

            let json_value: serde_json::Value =
                serde_json::from_slice(&buf).map_err(|e| ("Failed to parse JSON", e).into())?;

            if json_value.is_array() {
                return Err((
                    "TOML format does not support top-level arrays. Use '--format json' instead.",
                    InternalErrorKind::IllegalArgument,
                )
                    .into());
            }

            let toml_str = toml::to_string(&json_value).map_err(|e| {
                (
                    format!("Failed to serialize to TOML: {}", e),
                    InternalErrorKind::Unknown(Box::new(e)),
                )
                    .into()
            })?;
            println!("{}", toml_str);
        }
    }
    Ok(())
}

#[instrument]
async fn run() -> InternalResult<()> {
    let args = Args::parse();
    tracing::debug!("Parsed command-line arguments: {:?}", args);

    let stream = create_stream(args.socket).await?;
    let mut client = Client::new(stream).await;

    let req = CliRequest::Version(cli::VersionReq::default());
    match client.call(req).await? {
        CliResponse::Version(cli::VersionResp {
            result: cli::VersionResult::VersionOk { version },
        }) => {
            tracing::info!("Connected to ourodaemon version: {}", version);
        }
        CliResponse::Version(cli::VersionResp {
            result: cli::VersionResult::VersionMismatch { message },
        }) => {
            eprintln!("Version mismatch: {}", message);
        }
        CliResponse::Error(e) => {
            eprintln!("Failed to get version from daemon: {}", e);
            return Ok(());
        }
        _ => {
            eprintln!("Unexpected response type when requesting version");
            return Ok(());
        }
    }

    match args.command {
        Some(Command::Show { format, target }) => {
            let req: CliRequest = target.into();
            match client.call(req).await? {
                CliResponse::Show(resp) => print_output(format, &resp.as_serialize())?,

                CliResponse::Error(e) => {
                    eprintln!("{}", e);
                }

                _ => {
                    eprintln!("Unexpected response type");
                }
            }
        }
        Some(Command::Echo { format, message }) => {
            let req = CliRequest::Echo(cli::EchoReq {
                message: message.clone(),
            });
            match client.call(req).await? {
                CliResponse::Echo(resp) => {
                    print_output(format, &resp)?;
                }

                CliResponse::Error(e) => {
                    eprintln!("{}", e);
                }

                _ => {
                    eprintln!("Unexpected response type");
                }
            }
        }
        Some(Command::List { format }) => {
            let req = CliRequest::List(Void);
            match client.call(req).await? {
                CliResponse::List(cli::ListResp { tasks }) => {
                    print_output(format, &tasks)?;
                }

                CliResponse::Error(e) => {
                    eprintln!("{}", e);
                }

                _ => {
                    eprintln!("Unexpected response type");
                }
            }
        }
        Some(Command::Describe { format, name }) => {
            let req = CliRequest::Describe(cli::DescribeReq { name });
            match client.call(req).await? {
                CliResponse::Describe(cli::DescribeResp { descriptions }) => {
                    print_output_erased(format, &descriptions)?;
                }

                CliResponse::Error(e) => {
                    eprintln!("{}", e);
                }

                _ => {
                    eprintln!("Unexpected response type");
                }
            }
        }
        Some(Command::Operate {
            format,
            name,
            operation,
            input,
        }) => {
            let inputs: HashMap<String, String> = input.into_iter().collect();
            let req = CliRequest::Operate(cli::OperateReq {
                name,
                operation_name: operation,
                inputs,
            });
            match client.call(req).await? {
                CliResponse::Operate(cli::OperateResp { output }) => {
                    print_output(format, &output)?;
                }

                CliResponse::Error(e) => {
                    eprintln!("{}", e);
                }

                _ => {
                    eprintln!("Unexpected response type");
                }
            }
        }
        None => {
            println!("No command provided, exiting.");
            return Ok(());
        }
    }

    Ok(())
}
