# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Async RPC client for communicating with a edge peer over asyncio streams using FlatBuffers.

This module implements a length-prefixed RPC protocol with the following characteristics:
- Messages are framed with a 4-byte little-endian unsigned length prefix followed by the FlatBuffers payload.
- Requests and responses are FlatBuffers tables defined in fb.rpc.Request and fb.rpc.Response.
- Each request is assigned a monotonically increasing integer ID; responses carry the same ID to match calls.

Key components:
- RpcClient: Manages a connection to a peer, a background receive loop, and a map of pending requests.
    - start(): Awaits peer.start() to obtain (asyncio.StreamReader, asyncio.StreamWriter), then launches
        a background task that continuously reads responses, decodes them, and resolves the matching futures.
    - call(method: str, params: bytes): Builds and sends a fb.rpc.Request, registers a Future in a pending
        dictionary keyed by the request ID, drains the writer, and awaits the server’s reply. The returned
        value is an fb.rpc.Response instance (FlatBuffers view).

Behavior and design notes:
- The receive loop handles:
    - asyncio.IncompleteReadError: Logs an EOF/disconnection event.
    - Other exceptions: Logs with stack trace; no automatic retries are performed.
- Pending futures:
    - Responses are matched by ID; out-of-order delivery is supported.
    - On disconnection, unresolved futures remain pending and should be handled by the caller during shutdown.
- Concurrency and lifecycle:
    - Not thread-safe; use from a single asyncio event loop.
    - Call start() before call(); reuse a single RpcClient for multiple requests.
    - No explicit stop method; callers should cancel recv_task and close the writer when done.
- Peer contract:
    - The provided peer must implement an async start() method returning (StreamReader, StreamWriter).

Typical usage:
        client = RpcClient(peer)
        await client.start()
        response = await client.call("MethodName", b"serialized_params")
        # 'response' is an fb.rpc.Response FlatBuffers table that can be accessed via generated accessors.
"""

import asyncio
import struct
import typing
from logging import getLogger

import flatbuffers

from ..gen import rpc_generated as fbr

_logger = getLogger(__name__)


class RpcClient:
    """Async RPC client that communicates with an edge peer using length-prefixed FlatBuffers messages.

    Manages a background receive loop to match responses to pending requests by ID,
    supporting out-of-order delivery.
    """

    def __init__(self, peer: typing.Any) -> None:
        """Initialize the RPC client.

        Args:
            peer: An IPC peer instance providing asyncio streams via start().
        """
        self.peer = peer
        self.next_id: int = 1
        self.pending: dict[int, asyncio.Future[typing.Any]] = {}
        self.recv_task: asyncio.Task[None] | None = None
        self.writer: asyncio.StreamWriter | None = None

    async def start(self) -> None:
        """Start the peer connection and launch the background response receive loop."""

        async def _recv_response_loop(reader):
            try:
                try:
                    while True:
                        data_length = await reader.readexactly(4)
                        data_length = struct.unpack("<I", data_length)[0]
                        response = await reader.readexactly(data_length)
                        response = fbr.Response.GetRootAs(response, 0)
                        future = self.pending.pop(response.Id(), None)
                        if future:
                            future.set_result(response)
                        else:
                            _logger.error(f"Received response for unknown request ID: {response.Id()}")
                except asyncio.IncompleteReadError:
                    _logger.error("edge peer disconnected (EOF)")
                except Exception as e:
                    _logger.exception(f"recv_loop error: {e}")
            finally:
                # reader does not have .close() method, and thus we don't need to call it.
                _logger.info("RpcClient receive loop terminated.")

        reader, writer = await self.peer.start()
        self.recv_task = asyncio.create_task(_recv_response_loop(reader))
        self.writer = writer

    async def call(self, method: str, params: bytes) -> typing.Any:
        """Send an RPC request and await the response.

        Args:
            method (str): The RPC method name to invoke.
            params (bytes): FlatBuffers-serialized parameters for the request.
        Returns:
            asyncio.Future[typing.Any]: The fb.rpc.Response FlatBuffers table from the peer.
        """
        req_id = self.next_id
        self.next_id += 1

        fbb = flatbuffers.Builder(0)

        method = fbb.CreateString(method)
        params = fbb.CreateByteVector(params)

        fbr.RequestStart(fbb)
        fbr.RequestAddId(fbb, req_id)
        fbr.RequestAddMethod(fbb, method)
        fbr.RequestAddParams(fbb, params)
        fbb.Finish(fbr.RequestEnd(fbb))
        data = fbb.Output()

        _logger.debug(f"Sending request id={req_id} method={method} size={len(data)}")
        self.writer.write(struct.pack("<I", len(data)) + data)

        future = asyncio.get_event_loop().create_future()
        self.pending[req_id] = future
        await self.writer.drain()

        return await future  # This returns FBRequest.Request

    async def stop(self) -> None:
        """
        Stop the RpcClient by cancelling the receive task and closing the writer.
        """
        if self.recv_task:
            self.recv_task.cancel()
        if self.writer:
            try:
                self.writer.close()
                await self.writer.wait_closed()
            except (OSError, asyncio.CancelledError):
                pass
        _logger.info("RpcClient stopped and resources cleaned up.")
