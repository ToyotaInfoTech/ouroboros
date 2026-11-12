# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Asynchronous UNIX domain socket peer for inter-process communication with a child
edge process.

This module defines UdsPeer, an asyncio-based IPC peer that:
- Creates a connected AF_UNIX socket pair to bridge a local Python RPC client and
    a remote subprocess (typically a Rust-based edge service).
- Launches the child process with the socket file descriptor passed via the **RPC_FD**
    environment variable, and enables rich diagnostics with **RUST_LOG** and **RUST_BACKTRACE**.
- Wraps the local socket end into asyncio StreamReader/StreamWriter for request/response
    communication.
- Continuously pipes the child process's stdout and stderr into the logger (debug level),
    line-by-line, using background asyncio tasks.
- Provides graceful shutdown via stop(), cancelling background tasks, closing streams,
    and terminating the child process with a timeout fallback to kill.
- Registers an atexit handler to perform best-effort cleanup at interpreter exit,
    including scenarios with or without a running event loop, and delegates final
    cleanup from __del__ to the atexit path.

Typical usage:
- Instantiate UdsPeer with the desired command line to start the edge subprocess.
- Await start() to obtain asyncio streams connected to the child process.
- Interact with the edge over these streams and call stop() to release resources.

This design ensures robust lifecycle management, proper integration with asyncio,
and reliable logging/diagnostics for the child process.
"""

import asyncio
import atexit
import contextlib
import logging
import os
import socket
import subprocess
from typing import override

from ouroboros.edge import ipc

_logger = logging.getLogger(__name__)


class UdsPeer(ipc.Peer):
    def __init__(self, cmdline: list[str]):
        """
        Initialize UdsPeer with the command line for the child process.
        Args:
            cmdline (list[str]): Command line arguments to launch the edge subprocess.
        """
        super().__init__(cmdline)
        self.proc: subprocess.Popen | None = None
        self.background_tasks: list[asyncio.Task] = []
        atexit.register(self._atexit_cleanup)

    @override
    async def start(self) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
        """
        Start the edge subprocess and set up asynchronous communication.
        Returns:
            Tuple[asyncio.StreamReader, asyncio.StreamWriter]:
                Asyncio streams connected to the child process for request/response.
        """
        # Create a connected socket pair between local (Python) and remote (Rust)
        local_sock, remote_sock = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        local_sock.setblocking(False)

        remote_fd = remote_sock.fileno()

        child_env = {
            **os.environ,
            "RPC_FD": str(remote_fd),
        }

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Starting subprocess with command: '{' '.join(self.cmdline)}'")
            _logger.debug(f"Environment variables: {' '.join([f'{k}={v}' for k, v in child_env.items()])}")

        self.proc = subprocess.Popen(
            self.cmdline,
            pass_fds=[remote_fd],
            env=child_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        # Wrap the local socket end with asyncio streams for the RPC client
        reader, writer = await asyncio.open_unix_connection(sock=local_sock)

        async def _pipe_stream(stream: asyncio.StreamReader, prefix: str):
            while True:
                line = await stream.readline()
                if not line:
                    break
                _logger.info(f"{prefix}{line.rstrip().decode('utf-8', errors='replace')}")

        edge_pid = self.proc.pid

        # If child stdout/stderr exists, pipe them into asyncio and consume
        if self.proc.stdout:
            loop = asyncio.get_running_loop()
            stdout_reader = asyncio.StreamReader()
            stdout_proto = asyncio.StreamReaderProtocol(stdout_reader)
            await loop.connect_read_pipe(lambda: stdout_proto, self.proc.stdout)
            task = asyncio.create_task(_pipe_stream(stdout_reader, f"[edge({edge_pid}) stdout] "))
            self.background_tasks.append(task)

        if self.proc.stderr:
            loop = asyncio.get_running_loop()
            stderr_reader = asyncio.StreamReader()
            stderr_proto = asyncio.StreamReaderProtocol(stderr_reader)
            await loop.connect_read_pipe(lambda: stderr_proto, self.proc.stderr)
            task = asyncio.create_task(_pipe_stream(stderr_reader, f"[edge({edge_pid}) stderr] "))
            self.background_tasks.append(task)

        return reader, writer

    @override
    async def stop(self) -> None:
        """
        Gracefully stop the edge subprocess and clean up resources.
        Cancels background tasks, closes streams, and terminates the child process.
        """
        # Attempt to cancel the receive task and await as much as possible
        for task in self.background_tasks:
            if task and not task.done():
                task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await task

        # Attempt to gracefully stop the child process (up to 3s, then kill)
        proc = self.proc
        if proc and proc.poll() is None:
            try:
                proc.terminate()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
            except OSError:
                pass

    def _atexit_cleanup(self):
        """
        Perform best-effort cleanup at interpreter exit.
        Handles both running and non-running event loop scenarios.
        """
        # Last-resort cleanup at process exit; schedule async stop if an event loop is running
        try:
            loop = asyncio.get_running_loop()
            if loop.is_running():
                # Fire-and-forget async stop (do not wait)
                loop.create_task(self.stop())  # noqa: RUF006
            else:
                # If no loop, wait synchronously as much as possible
                if self.proc and self.proc.poll() is None:
                    try:
                        self.proc.terminate()
                        self.proc.wait(timeout=2)
                    except OSError:
                        try:
                            self.proc.kill()
                            self.proc.wait(timeout=1)
                        except OSError:
                            pass
        except RuntimeError:
            # No event loop created
            if self.proc and self.proc.poll() is None:
                try:
                    self.proc.terminate()
                    self.proc.wait(timeout=2)
                except OSError:
                    try:
                        self.proc.kill()
                        self.proc.wait(timeout=1)
                    except OSError:
                        pass

    def __del__(self):
        """
        Destructor: delegates cleanup to the atexit handler.
        """
        # Cannot await here; delegate cleanup to the atexit handler
        with contextlib.suppress(OSError):
            self._atexit_cleanup()
