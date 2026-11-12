# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Tests for ouroboros.edge.rpc module."""


import flatbuffers

from ouroboros.gen import rpc_generated as fbr


def _build_response(req_id: int, result: bytes = b"ok") -> bytes:
    """Build a FlatBuffers RPC response message."""
    fbb = flatbuffers.Builder(64)
    result_off = fbb.CreateByteVector(result)
    fbr.ResponseStart(fbb)
    fbr.ResponseAddId(fbb, req_id)
    fbr.ResponseAddResult(fbb, result_off)
    fbb.Finish(fbr.ResponseEnd(fbb))
    return bytes(fbb.Output())


class TestRpcClientRequestId:
    """Tests for RPC client request ID management."""

    def test_request_ids_start_at_one(self):
        """Each RPC client should start with ID 1."""
        from ouroboros.edge.rpc import RpcClient

        client = RpcClient(peer=None)
        assert client.next_id == 1

    def test_multiple_clients_have_independent_ids(self):
        """Different RPC clients should have independent ID counters."""
        from ouroboros.edge.rpc import RpcClient

        client_a = RpcClient(peer=None)
        client_b = RpcClient(peer=None)
        client_a.next_id = 5
        assert client_b.next_id == 1

    def test_pending_dict_starts_empty(self):
        """Pending requests dict should be empty on init."""
        from ouroboros.edge.rpc import RpcClient

        client = RpcClient(peer=None)
        assert client.pending == {}

    def test_recv_task_initially_none(self):
        """recv_task should be None before start."""
        from ouroboros.edge.rpc import RpcClient

        client = RpcClient(peer=None)
        assert client.recv_task is None


class TestFlatBuffersResponseParsing:
    """Tests for FlatBuffers response message construction and parsing."""

    def test_build_response_is_valid(self):
        """Built response should be parseable by FlatBuffers."""
        data = _build_response(42, b"hello")
        resp = fbr.Response.GetRootAs(data, 0)
        assert resp.Id() == 42

    def test_response_round_trip(self):
        """Response should preserve the ID through build/parse cycle."""
        for req_id in [1, 100, 999999]:
            data = _build_response(req_id)
            resp = fbr.Response.GetRootAs(data, 0)
            assert resp.Id() == req_id
