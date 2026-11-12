# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Defines the abstract Router interface for coordinating communication with a remote simulator.

This module provides a thin Abstract Base Class (ABC) that formalizes the responsibilities
of any router implementation used to control and communicate with a simulation process.
Subclasses are expected to:

- Establish and manage a connection to a remote simulator (`connect_to_sim`), including
    any transport setup, authentication, or protocol negotiation.
- Launch or attach to a simulator process (`launch_sim`) given a command
    specification, handling process lifecycle and error reporting.
- Run the main simulation loop (`run`), orchestrating I/O, message routing, event handling,
    and graceful shutdown.

Implementations should clearly define:
- Connection semantics (blocking vs. non-blocking, retries, timeouts).
- Error handling strategies and resource cleanup.
- Logging and observability for diagnosing connectivity or runtime issues.

This abstraction enables interchangeable router implementations (local or remote)
while keeping higher-level orchestration code decoupled from
transport and process-management details.
"""

from abc import ABC, abstractmethod


class Router(ABC):
    def __init__(self):  # noqa: B027
        pass

    @abstractmethod
    def connect_to_sim(self, host, port, **kwargs) -> None:
        """
        Connect to the remote simulator.

        Args:
            host (str): Hostname or IP address of the simulator.
            port (int): Port number of the simulator.
            **kwargs: Additional connection parameters.
        """
        return None

    @abstractmethod
    def launch_sim(self, sim_cmd: list[str]) -> None:
        """
        Launch a coordinator with the given simulation command.

        Args:
            sim_cmd (list[str]): Command and arguments to launch the simulator.
        """
        return None

    @abstractmethod
    async def run(self) -> None:
        """
        Run the main simulation loop.
        """
        return None

    @abstractmethod
    def stop(self) -> None:
        """
        Stop the router and clean up resources.
        """
        return None
