# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
edge module

This module contains implementations for different types of edge peers.
It currently includes a Unix domain socket-based edge peer.

Usage:
        from edge import new_peer
        peer = new_edge_peer("unix")
"""

from .ipc import Peer
from .ipc_uds import UdsPeer

__all__ = ["new_peer"]

_FACTORY = {
    "uds": UdsPeer,  # Currently, only Unix domain socket-based edge peer is supported.
}


def new_peer(name: str, *args, **kwargs) -> Peer:
    """Factory function to create a new edge peer based on the specified name.
    Args:
        name (str): The name of the edge peer type to create. Supported values are "uds" for Unix domain socket-based edge peer.
    Returns:
        Peer: An instance of the specified edge peer type.
    Raises:
        ValueError: If the specified name is not supported.
    """
    if name not in _FACTORY:
        raise ValueError(f"Unsupported edge peer type: {name}")
    return _FACTORY[name](*args, **kwargs)
