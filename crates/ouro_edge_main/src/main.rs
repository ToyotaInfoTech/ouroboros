// SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
// SPDX-License-Identifier: Apache-2.0

//! Agent module for the Ouroboros edge runtime.
//!
//! This module defines an `Agent` that participates in the edge system by:
//! - Managing lifecycle tasks (currently a periodic heartbeat publisher).
//! - Handling RPC calls (via [`rpc::MethodHandler`]) and dispatching them to agent methods.
//! - Parsing incoming FlatBuffers messages (e.g. `fbp::Peer`) and converting them into
//!   owned JSON for logging and persistence.
//! - Persisting selected state into the local database.
//!
//! ## Main components
//!
//! - `AgentConfig`: Builder-style configuration for constructing an `Agent`, including
//!   Outbox settings, agent identifier, and heartbeat interval.
//! - `Agent`: Owns the database handle, an `Outbox` publisher resource, and a set of
//!   background tasks. The `start()` method spawns tasks; `stop()` aborts them.
//! - `HeartbeatTask`: A background task that periodically publishes a liveness timestamp to
//!   `edge/{id}/status/liveness` using the Outbox.
//! - [`AgentSkeleton`]: RPC adapter that exposes agent methods (e.g. `"step"`) through the
//!   generic RPC handler interface.
//!
//! ## RPC behavior
//!
//! The `"step"` method expects FlatBuffers-encoded `fbp::Peer` bytes, converts the peer view
//! into JSON (including nested vectors such as `next_tls` and `neighbor_vehicles`), logs the
//! JSON representation, and inserts it into the system-state database table keyed by timestamp.
//! The method returns the original parameters unchanged on success.
//!
//! ## Concurrency notes
//!
//! Background tasks are spawned using `tokio::spawn` and managed via `JoinHandle`. Stopping the
//! agent aborts these handles. The Outbox is shared across tasks using `Arc`.
//!
//! ## Error handling
//!
//! FlatBuffers decoding errors are surfaced as `rpc::RpcError::Flatbuffers`. Handler and JSON
//! serialization errors are mapped to `rpc::RpcError::Handler`.
pub mod agent;
pub mod task;
pub mod types;

use crate::agent::{AgentSkeleton, EdgeAgent};
use async_trait::async_trait;
use clap::Parser;
use ouro_common::io::FromTomlFile as _;
use ouro_core::comm::rpc::{MethodHandler, RpcDispatcher as _};
use ouro_core::error::{panic_if_env_set, InternalError, InternalErrorKind};
use ouro_core::runtime::comp::CompGuard;
use ouro_core::setup_tracing;
use ouro_core::{comm::rpc, InternalResult};
use ouro_edge::runtime::ipc::ipc_uds::UdsPeer;
use ouro_edge::runtime::ipc::Peer as _;
use ouro_edge::runtime::rpc_handler::EdgeRpcHandler;
use ouro_state::edge::EdgeSpec;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    setup_tracing();
    if let Err(e) = run().await {
        panic_if_env_set(&e);
    }
}

async fn run() -> InternalResult<()> {
    // Parse command-line arguments

    #[derive(Parser, Debug)]
    #[command(
        version,
        author,
        about = env!("CARGO_PKG_DESCRIPTION"),
        long_about = None
    )]
    struct Args {
        /// Edge Identification Number
        #[arg(short = 'i', long, default_value = "0")]
        id: String,

        /// Edge model identifier
        #[arg(short = 'm', long, default_value = "default_model")]
        model: String,

        #[arg(short = 'c', long, required = true)]
        config: String,

        #[arg(short = 'l', long)]
        lint: bool,

        #[arg(short = 'P', long, default_value_t = false)]
        peer_disabled: bool,

        #[arg(short = 'p', long)]
        place_id: Option<String>,
    }

    let args = Args::parse();

    tracing::info!(
        "******** [[ Ouroboros Edge (pid:{}) ]] ********",
        std::process::id()
    );
    //log_env_vars!();

    let mut rpc_handler = EdgeRpcHandler::new();

    // Add a simple "echo" method handler for testing
    struct EchoHandler;

    #[async_trait]
    impl MethodHandler for EchoHandler
    where
        Self: Send + Sync,
    {
        async fn handle(&self, method: &str, params: Box<[u8]>) -> InternalResult<Box<[u8]>> {
            assert_eq!(method, "echo");
            tracing::info!("Echo handler called with params_length={}", params.len());
            Ok(params)
        }
    }
    let echo_handler: Arc<dyn MethodHandler + Send + Sync> = Arc::new(EchoHandler);

    rpc_handler.set_method_handler("echo", &echo_handler);

    // Parse TOML configuration file
    let mut state = EdgeSpec::from_toml_file(&args.config).map_err(|e| {
        Into::<InternalError>::into((
            format!("Failed to parse edge config file {}: {:?}", args.config, e),
            InternalErrorKind::Parse,
        ))
    })?;

    state.runtime.edge_id = args.id.clone();
    state.runtime.model = args.model.clone();
    state.runtime.place_id = args.place_id.clone();

    if args.lint {
        println!("Syntax is valid: {}", args.config);
        println!("Loaded edge config:\n{:#?}", state);
    }

    let agent = EdgeAgent::try_from(&state).await?;
    let agent1 = agent.clone();
    let context = agent.context.clone();
    let _comp = CompGuard::new("agent", agent, state.clone(), &context).await?;
    let agent_skeleton = {
        if args.lint {
            println!("An instance of Agent was successfully created and ready to start");
            std::process::exit(0);
        }

        tracing::info!("started: [agent]");

        Arc::new(AgentSkeleton::new(agent1))
    };
    let handler: Arc<dyn rpc::MethodHandler + Send + Sync> = agent_skeleton.clone();
    rpc_handler.set_method_handler("step", &handler);
    rpc_handler.set_method_handler("stop", &handler);

    tracing::info!("Ouroboros Edge is running.");
    if args.peer_disabled {
        tracing::info!("Peer mode disabled.");
    } else {
        let mut peer = UdsPeer::new(rpc_handler)?;
        peer.run().await?;
        // IPC peer disconnected (parent process terminated) → shutdown immediately.
        tracing::info!("IPC peer disconnected. Shutting down.");
        std::process::exit(0);
    }

    loop {
        tokio::time::sleep(tokio::time::Duration::MAX).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "integration test: requires IPC-capable environment with ouroedge binary"]
    fn test_edge() {
        #[cfg(unix)]
        {
            use ouro_edge::gen::rpc_generated::fb::rpc as fbr;
            use std::io::{Read, Write};
            use std::os::unix::io::IntoRawFd;
            use std::os::unix::net::UnixStream;
            use std::os::unix::process::CommandExt;
            use std::path::PathBuf;
            use std::process::{Command, Stdio};
            use std::sync::Once;
            use std::time::Duration;
            // Build the real binary (not the test harness) once.
            static BUILD: Once = Once::new();
            BUILD.call_once(|| {
                let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
                let status = Command::new(cargo)
                    .current_dir(env!("CARGO_MANIFEST_DIR"))
                    .args(["build", "--bin", "ouroedge"])
                    .status()
                    .expect("failed to run cargo build --bin ouroedge");
                assert!(status.success(), "cargo build failed");
            });

            let bin_path: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .find(|p| p.join("Cargo.lock").exists())
                .expect("could not find workspace root")
                .join("target")
                .join("debug")
                .join("ouroedge");
            assert!(bin_path.exists(), "binary not found at {:?}", bin_path);

            // Create a connected Unix-domain socket pair; pass one end to the child via RPC_FD.
            let (mut client_stream, child_stream) =
                UnixStream::pair().expect("UnixStream::pair failed");
            client_stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("set_read_timeout failed");
            client_stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .expect("set_write_timeout failed");

            // Use a fixed fd number in the child.
            const CHILD_RPC_FD: i32 = 3;
            let child_raw_fd = child_stream.into_raw_fd();

            let mut child = unsafe {
                Command::new(&bin_path)
                    .arg("--id")
                    .arg("testvin1234567890")
                    .arg("--model")
                    .arg("testmodelABC")
                    .env("RPC_FD", CHILD_RPC_FD.to_string())
                    .stdin(Stdio::null())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .pre_exec(move || {
                        // Duplicate the passed fd onto CHILD_RPC_FD (dup2 clears FD_CLOEXEC on the new fd).
                        let rc = libc::dup2(child_raw_fd, CHILD_RPC_FD);
                        if rc < 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                        if child_raw_fd != CHILD_RPC_FD {
                            libc::close(child_raw_fd);
                        }
                        Ok(())
                    })
                    .spawn()
                    .expect("failed to spawn ouroedge")
            };

            // Simple framed request:
            // [u32 total_len_le][u32 method_len_le][method bytes][u32 params_len_le][params bytes]
            // Response:
            // [u32 resp_len_le][resp bytes]

            let mut fbb = flatbuffers::FlatBufferBuilder::new();

            // Build request
            let fb_method = Some(fbb.create_string("echo"));
            let params_bytes = b"hello from test";
            let fb_params = Some(fbb.create_vector(params_bytes));

            let fb_request = fbr::Request::create(
                &mut fbb,
                &fbr::RequestArgs {
                    id: 1,
                    method: fb_method,
                    params: fb_params,
                },
            );

            fbb.finish(fb_request, None);

            let request_bytes = fbb.finished_data().to_vec();

            // Send request
            client_stream
                .write_all(&(request_bytes.len() as u32).to_le_bytes())
                .expect("write req_len failed");
            client_stream
                .write_all(&request_bytes)
                .expect("write request failed");
            client_stream.flush().expect("flush failed");

            // Read response
            let mut len_buf = [0u8; 4];
            client_stream
                .read_exact(&mut len_buf)
                .expect("read resp_len failed");
            let response_len = u32::from_le_bytes(len_buf) as usize;

            let mut response_bytes = vec![0u8; response_len];
            client_stream
                .read_exact(&mut response_bytes)
                .expect("read resp failed");

            let fb_response = flatbuffers::root::<fbr::Response>(&response_bytes)
                .expect("flatbuffers root failed");

            if fb_response.error().is_some() {
                panic!("RPC response has error: {}", fb_response.error().unwrap());
            }
            if fb_response.result().is_none() {
                panic!("RPC response has no result");
            }
            let fb_resp_result = fb_response.result().unwrap();
            let resp_result_bytes = fb_resp_result.bytes();

            println!(
                "echo result: {}",
                String::from_utf8_lossy(resp_result_bytes)
            );
            assert_eq!(resp_result_bytes, params_bytes);

            // Ensure the child does not hang the test in case it keeps serving.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
