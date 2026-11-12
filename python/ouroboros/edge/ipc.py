# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Abstract base class for IPC peers.

This module defines the Peer interface that all IPC peer implementations
must follow. A peer manages the lifecycle of a child process and provides
asyncio streams for bidirectional communication.
"""

import asyncio
from abc import ABC, abstractmethod


class Peer(ABC):
    """Abstract base class for inter-process communication peers.

    Subclasses must implement start() and stop() to manage the lifecycle
    of a child process and provide asyncio streams for communication.
    """

    def __init__(self, cmdline: list[str]):
        """Initialize the peer with the command line for the child process.

        Args:
            cmdline (list[str]): Command line arguments to launch the child process.
        """
        self.cmdline = cmdline

    @abstractmethod
    async def start(self) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
        """Start the child process and establish communication streams.

        Returns:
            Tuple[asyncio.StreamReader, asyncio.StreamWriter]: Asyncio streams
                connected to the child process for bidirectional communication.
        """
        raise NotImplementedError

    @abstractmethod
    async def stop(self) -> None:
        """Stop the child process and release all associated resources."""
        raise NotImplementedError
