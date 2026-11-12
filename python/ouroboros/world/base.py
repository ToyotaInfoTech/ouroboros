# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
The World framework for coordinating the World, places, and edges-level orchestrator.

This module defines three abstract base classes—World, Place, and Edges—
that together provide a structured data flow for managing edge state, grouping edges
into places, and orchestrating batch processing across those places. It is intended to be
subclassed to implement concrete policies for edge accommodation, per-edge behavior,
and place balancing.

Key concepts:
- Edge: Encapsulates per-edge logic and state. It accepts new state snapshots
    (e.g., ouroboros.gen.peer_generated) via step and returns a dictionary of actions for the
    edge. Subclasses typically implement start and refine step behavior.
- Place: Manages a collection of Edge instances within a single physical
    Ray node. It supports edge lifecycle (add/remove), exposes helper queries, and processes
    batched updates for its edges, aggregating the actions produced by each edge.
    Optional global logging can be configured via util.configure_global_logging.
- World: Coordinates multiple places and routes edge updates to the appropriate place.
    It maintains mappings between edge IDs and place IDs, determines placement via an
    abstract accommodate_edge policy, and provides a step pipeline that groups updates
    per place and aggregates results. A balance_places hook is provided for rebalancing.

Data flow:
1) World.step receives a mapping of edge_id -> state payload.
2) Each edge is assigned to (or located in) a place via locate_edge_place, which
     delegates to accommodate_edge when needed.
3) Updates are batched per place and passed into Place.step.
4) Place.step fans out updates to each Edge, collects their action maps,
     and returns an aggregated result.
5) World aggregates all place-level actions and returns a unified mapping of
     edge_id -> action payload.

Logging:
- The module emits debug logs for initialization, edge assignment/removal, and world
    steps. Implementations may adjust verbosity via util.configure_global_logging at the
    place level.

Extension points to implement:
- Edge: start, step behavior and any edge-specific state management.
- Place: add_edge for creating and registering Edge instances.
- World: accommodate_edge to decide the place for a edge; balance_places to perform
    rebalancing after topology changes.

Notes:
- ouroboros.gen.peer_generated denotes an external peer/state object used to represent edge state snapshots.
    Implementations may adapt or validate incoming payloads before passing them down.
- The module does not enforce concurrency controls; if used in a multithreaded or
    asynchronous context, ensure appropriate synchronization around shared structures
    (edges, places, and mappings).
"""

import logging
from abc import ABC, abstractmethod
from typing import Any

from ouroboros import util
from ouroboros.world.behavior import BehaviorRuntime, load_behavior_config, register_packs
from ouroboros.world.behavior import get_config as _get_behavior_config
from ouroboros.world.map import BaseMap


class EdgeCapacityExceeded(Exception):
    """Raised when all places have reached their maximum edge capacity."""


_logger = logging.getLogger(__name__)


def _resolve_behavior_config(behavior_config: dict[str, Any] | None) -> dict[str, Any]:
    """Resolve the behavior policy visible to a Place or Edge.

    ``None`` does not mean "disable behaviors for this object". It means the
    caller did not provide an override, so the current shared policy loaded via
    ``load_behavior_config()`` should be used.

    This keeps ``BehaviorRuntime`` simple: it always receives a concrete config
    dict rather than interpreting ``None`` itself.
    """
    return behavior_config if behavior_config is not None else _get_behavior_config()


class Edge(ABC):  # noqa: B024
    """
    Encapsulates per-edge logic and state management.
    """

    def __init__(
        self,
        edge_id: str,
        init_state: dict[str, Any],
        behavior_config: dict[str, Any] | None = None,
    ):
        """
        Initialize the Edge with a unique ID and initial state.

        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
            behavior_config (Dict[str, Any] | None): Configuration for the
                dangerous-driving behavior runtime. When ``None``, this edge
                uses the current shared policy loaded by
                ``load_behavior_config()``. Supplying a dict here is the future
                extension point for per-edge overrides.
        """
        self.edge_id = edge_id
        self.state = init_state
        self.behavior_config = _resolve_behavior_config(behavior_config)
        self._behavior_runtime = BehaviorRuntime(edge_id, self.behavior_config)
        _logger.debug(f"Edge {edge_id} is initialized")

    async def start(self) -> None:  # noqa: B027
        """
        Start any necessary edge processes or initialization.
        """
        pass

    def get_state(self) -> dict[str, Any]:
        """
        Get the current state of the edge.
        Returns:
            Dict[str, Any]: Current state of the edge.
        """
        return self.state

    def apply_behaviors(self, state: dict[str, Any], dt_sec: float = 1.0) -> dict[str, Any]:
        """Run the behavior runtime and return an action-patch dict.

        Subclasses should call this at the end of their ``step()`` and merge
        the result into the action dict they return.

        Args:
            state: Current SUMO state dict for the vehicle.
            dt_sec: Seconds elapsed since the previous tick.

        Returns:
            Action-patch dict (may be empty).
        """
        return self._behavior_runtime.step(state, dt_sec)

    async def step(self, new_state: dict[str, Any]) -> dict[str, Any]:
        """
        Process a new state snapshot and return actions.
        Args:
            new_state (Dict[str, Any]): New state payload for the edge.
        Returns:
            Dict[str, Any]: Actions produced by processing the new state.
        """
        self.state = new_state

        if self.state["status"] == "arrived":
            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge {self.edge_id} arrived")
            await self.stop()

        action: dict[str, Any] = {"status": new_state["status"]}
        action.update(self.apply_behaviors(new_state))
        return action

    async def stop(self) -> None:  # noqa: B027
        """
        Stop any edge processes and clean up resources.
        """
        pass


class Place(ABC):
    """
    Manages a collection of Edge instances within a physical place.
    """

    def __init__(
        self,
        place_id: str,
        behavior_config: dict[str, Any] | None = None,
        log_config: dict | None = None,
    ):
        """
        Initialize the Place with a unique ID and optional logging configuration.
        Args:
            place_id (str): Unique identifier for the place.
            behavior_config (dict[str, Any] | None): Dangerous-driving behavior
                configuration shared with edges created by this place. When
                ``None``, the place inherits the current shared policy instead
                of disabling behaviors.
            log_config (dict | None): Optional logging configuration dictionary.
        """
        if log_config:
            util.configure_global_logging(log_config)

        self.place_id = place_id
        self.edges = {}
        self.behavior_config = _resolve_behavior_config(behavior_config)

        # When an explicit config dict is provided (e.g. serialized by the ray
        # driver and passed to a remote Place actor), register the declared
        # behavior packs in this process so that REGISTRY is populated.
        if behavior_config is not None:
            register_packs(self.behavior_config)

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {place_id} initialized.")

    @abstractmethod
    async def add_edge(
        self,
        edge_id: str,
        init_state: dict[str, Any],
    ) -> None:
        """
        Add and start a new edge with the given ID and initial state.
        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
            renderer_name (str | None): Name of the renderer to use for the edge.
        """
        raise NotImplementedError

    def remove_edge(self, edge_id: str) -> None:
        """
        Remove the edge with the given ID.
        Args:
            edge_id (str): Unique identifier for the edge to remove.
        """

        if edge_id in self.edges:
            del self.edges[edge_id]
            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge {edge_id} removed from Place {self.place_id}.")

    async def stop_edge(self, edge_id: str) -> None:
        """
        Stop a single edge by ID (send stop RPC and clean up), then remove it.
        Args:
            edge_id (str): Unique identifier for the edge to stop.
        """
        edge = self.get_edge(edge_id)
        if edge:
            await edge.stop()
            self.remove_edge(edge_id)

    def get_edge(self, edge_id: str) -> Edge | None:
        """
        Get the edge with the given ID.
        Args:
            edge_id (str): Unique identifier for the edge.
        Returns:
            Edge | None: The Edge instance if found, else None.
        """
        return self.edges.get(edge_id, None)

    def get_edge_count(self) -> int:
        """
        Get the number of edges managed by this place.
        Returns:
            int: Number of edges.
        """
        return len(self.edges)

    async def step(self, updates: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
        """
        Process a batch of edge updates and return aggregated actions.
        Args:
            updates (Dict[str, Dict[str, Any]]): Mapping of edge_id -> new state payload.
        Returns:
            Dict[str, Dict[str, Any]]: Aggregated mapping of edge_id -> action payload.
        """
        actions: dict[str, dict[str, Any]] = {}
        for edge_id, new_state in updates.items():
            edge = self.get_edge(edge_id)
            if edge:
                action = await edge.step(new_state)

                if action["status"] == "arrived":
                    if _logger.isEnabledFor(logging.DEBUG):
                        _logger.debug(f"Edge '{edge_id}' arrived at Place '{self.place_id}' with state: {new_state}")
                    self.remove_edge(edge_id)

                actions[edge_id] = action
        return actions

    async def stop(self) -> None:  # noqa: B027
        """
        Stop any edge processes and clean up resources.
        """
        pass

    def set_shared_data(self, key: str, data) -> None:  # noqa: B027
        """Receive shared data from the World.

        Subclasses may override this to react to specific keys (e.g.
        forwarding map data to a renderer).

        Args:
            key: Identifier for the data (e.g. ``"map_data"``).
            data: The payload.
        """
        pass


class World(ABC):
    """
    Coordinates multiple places and routes edge updates appropriately.
    """

    def __init__(self):
        """
        Initialize the World with empty place and edge mappings.
        """
        self.places: dict[str, Place] = {}
        self.edge2place: dict[str, str] = {}
        self.map_data: BaseMap | None = None
        self.behavior_config = _get_behavior_config()
        _logger.debug(f"{self.__class__.__name__} initialized.")

    def load_behavior_config(self, path: str | None) -> dict[str, Any]:
        """Load and cache the shared behavior configuration for this world.

        The loaded dict is the default policy for newly created Places and
        Edges unless a more specific override is passed explicitly.
        """
        self.behavior_config = load_behavior_config(path)
        return self.behavior_config

    def init(self) -> None:  # noqa: B027
        """
        Perform any necessary initialization for the world.
        """
        pass

    @abstractmethod
    async def accommodate_edge(self, edge_id: str, init_state: dict[str, Any]) -> str:
        """
        Determine and return the place ID where the edge should be accommodated.
        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
        Returns:
            str: The place ID where the edge is accommodated.
        """
        raise NotImplementedError

    async def locate_edge_place(self, edge_id: str, init_state: dict[str, Any]) -> str:
        """
        Locate or accommodate the edge to a place and return the place ID.
        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
        Returns:
            str: The place ID where the edge is located.
        """

        if edge_id in self.edge2place:
            return self.edge2place[edge_id]

        place_id = await self.accommodate_edge(edge_id, init_state)
        if _logger.isEnabledFor(logging.DEBUG):
            if init_state["status"] != "departed":
                _logger.debug(
                    f"Received non-departed state for new edge {edge_id}, but it is not currently assigned to a place. Accommodating it to a place anyway."
                )
            _logger.debug(f"Edge {edge_id} assigned to place {place_id}.")

        self.edge2place[edge_id] = place_id

        return place_id

    def remove_edge(self, edge_id: str) -> None:
        """
        Remove the edge with the given ID from its place.
        Args:
            edge_id (str): Unique identifier for the edge to remove.
        """
        place_id = self.edge2place.get(edge_id, None)
        if place_id:
            place = self.places.get(place_id, None)
            if place:
                place.remove_edge(edge_id)
                self.balance_places()
            del self.edge2place[edge_id]

    def balance_places(self) -> None:  # noqa: B027
        pass

    async def notify_arrived(self, edge_ids) -> None:
        """
        Stop and remove edges that have arrived (left the simulation).
        Args:
            edge_ids: Iterable of edge IDs that have arrived.
        """
        for edge_id in edge_ids:
            place_id = self.edge2place.get(edge_id, None)
            if place_id is None:
                continue
            place = self.places.get(place_id, None)
            if place:
                await place.stop_edge(edge_id)
            self.remove_edge(edge_id)

    async def step(self, states: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
        """
        Process a batch of edge state updates and return aggregated actions.
        Args:
            states (Dict[str, Dict[str, Any]]): Mapping of edge_id -> new state payload.
        Returns:
            Dict[str, Dict[str, Any]]: Aggregated mapping of edge_id -> action payload.
        """
        batch_updates: dict[str, dict[str, dict[str, Any]]] = dict()
        for edge_id, state in states.items():
            try:
                place_id = await self.locate_edge_place(edge_id, state)
            except EdgeCapacityExceeded:
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug(f"Cannot accommodate edge {edge_id}: all places at capacity. Skipping.")
                continue
            if place_id not in batch_updates:
                batch_updates[place_id] = {}
            batch_updates[place_id][edge_id] = state

        actions = await self.batch_step(batch_updates)

        return actions

    async def batch_step(self, batch_updates: dict[str, dict[str, dict[str, Any]]]) -> dict[str, dict[str, Any]]:
        """
        Process batched updates per place and return aggregated actions.
        Args:
            batch_updates (Dict[str, Dict[str, Dict[str, Any]]]): Mapping of place_id -> (edge_id -> new state payload).
        Returns:
            Dict[str, Dict[str, Any]]: Aggregated mapping of edge_id -> action payload.
        """
        actions: dict[str, dict[str, Any]] = dict()

        for place_id, edge_updates in batch_updates.items():
            place = self.places.get(place_id, None)
            if place:
                place_actions = await place.step(edge_updates)
                actions.update(place_actions)
            else:
                _logger.warning(f"Place with id '{place_id}' not found, ignoring the updates")

        return actions

    async def stop(self) -> None:  # noqa: B027
        pass

    def set_map_data(self, map_data: BaseMap) -> None:
        """Store road-network geometry and distribute to all Places.

        Args:
            map_data: BaseMap instance providing lanes, tls_list, and junctions.
        """
        self.map_data = map_data
        for place in self.places.values():
            place.set_shared_data("map", map_data)

    def set_shared_data(self, key: str, data) -> None:
        """Distribute shared data to all Places.

        Args:
            key: Identifier for the data.
            data: The payload.
        """
        for place in self.places.values():
            place.set_shared_data(key, data)
