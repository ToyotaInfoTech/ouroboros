# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Ray-based orchestration layer for distributed edge simulation.

This module integrates asyncio and Ray to coordinate many simulated edges across
a set of Ray actors ("places"). Each place hosts edge that proxy edge
state updates to an external edge process via RPC over IPC, and return computed actions.

Overview:
- Edge:
    Bridges the cluster with a edge peer process using an RpcClient over a UNIX
    domain socket. It serializes updates and deserializes actions using FlatBuffers.
- Place (Ray actor):
    Manages a collection of Edge instances for a single place. It applies
    state updates concurrently to edges and aggregates their resulting actions.
- PlaceWrapper:
    Lightweight holder for a Ray actor reference and edge-count bookkeeping.
- World:
    Top-level coordinator that:
    - Initializes Ray and manages a pool of places.
    - Accommodates edges by assigning them to the least-populated place.
    - Executes batch steps by dispatching updates to places and collecting actions.
    - Tracks in-progress Ray tasks and balances load across places.

Key behaviors:
- Concurrency:
    Edge steps within a place are scheduled with asyncio tasks and awaited as they
    complete. Place-level steps are invoked concurrently via Ray and gathered with
    ray.get.
- Load balancing:
    A SortedList of PlaceWrappers is maintained (sorted by edge count) to select
    the least-populated place efficiently. Edge additions and removals update this
    structure in O(log N).
- Configuration:
    World accepts min_edges_per_place (>=1) and max_places (>=1). Ray is
    initialized with the remaining kwargs provided to World.

Error handling:
- Attempting to reassign an already accommodated edge raises ValueError.
- Invalid configuration values (e.g., nonpositive min_edges_per_place or
    max_places) raise ValueError.

Dependencies and assumptions:
- Requires Ray for actor-based distribution and asyncio for intra-actor concurrency.
- Uses FlatBuffers-based serialization helpers (fb_serialize/fb_deserialize).
- Expects an external edge executable reachable via a UNIX domain socket.
- Relies on base classes in cluster.base (e.g., EdgeLiaison, Place, World)
    and optional global logging configuration (util.global_log_config).

This module is designed to scale simulations by:
- Minimizing coordination overhead via actor-local concurrency.
- Balancing edges across places.
- Pipelining edge additions with in-progress Ray tasks and synchronizing before
    batch steps to ensure consistent state.
"""

import asyncio
import logging
from typing import Any, override

from sortedcontainers import SortedList

from ..edge import new_peer, rpc
from ..message.fb_serde import fb_serialize
from . import base
from .map import BaseMap
from .renderer import Renderer, new_renderer
from .wgpu_renderer import SharedMapBuffers, create_shared_map_buffers

_logger = logging.getLogger(__name__)


class Edge(base.Edge):
    @override
    def __init__(
        self,
        edge_id: str,
        init_state: dict[str, Any],
        behavior_config: dict[str, Any] | None = None,
        edge_enabled: bool = False,
        edge_exec: str | None = None,
        edge_config: str | None = None,
        place_id: str | None = None,
    ):
        """
        Initialize the Edge with an RpcClient to communicate with the edge peer process.
        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
            edge_config (str): Configuration for the edge.
            edge_exec (str): Path to the edge executable.
            edge_enabled (bool): Whether to enable the RpcClient immediately. [default: False]
            behavior_config (dict[str, Any] | None): Explicit behavior config
                injected by the local World.
        """

        super().__init__(edge_id, init_state, behavior_config=behavior_config)

        if edge_enabled:
            cmdline = [
                # " lldb-server g 0.0.0.0:12345",
                # "--",
                edge_exec,
                "--id",
                edge_id,
                "--model",
                init_state["type_id"],
                "--config",
                edge_config,
            ]
            if place_id:
                cmdline += ["--place-id", place_id]
            _logger.info(f"Edge {edge_id} enabling RpcClient with cmdline: {cmdline}")
            self.client = rpc.RpcClient(new_peer("uds", cmdline))
            _logger.info(f"Edge {edge_id} RpcClient created.")
        else:
            self.client = None

    @override
    async def start(self) -> None:
        """
        Start the RpcClient to connect to the edge peer process.
        """
        if self.client:
            await self.client.start()

    @override
    async def step(self, new_state: dict[str, Any]) -> dict[str, Any]:
        """
        Send a state update to the edge peer and receive actions.
        Args:
            new_state (Dict[str, Any]): New state payload for the edge.
        Returns:
            Dict[str, Any]: Action payload with behavior patches applied.
        """

        if self.client:
            # Inject the currently active behavior name into state before
            # serialization so that ouroedge can record it in system_state.
            new_state["active_behavior"] = self._behavior_runtime.active_behavior_name

            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge {self.edge_id} sending new state")

            # We don't expect the action returned from the edge peer
            # We directry apply behavior patches on top of the new state and return that as the action.
            _response = await self.client.call("step", fb_serialize(new_state))

            if new_state["status"] == "arrived":
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug(f"Edge {self.edge_id} arrived")
                await self.stop()

            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge {self.edge_id} received actions")

        action = {"status": new_state["status"]}
        # Merge behavior patches (overrides RPC-returned values when active)
        action.update(self.apply_behaviors(new_state))

        return action

    @override
    async def stop(self) -> None:
        """
        Stop the RpcClient and clean up resources.
        """
        if self.client:
            await self.client.call("stop", b"")

            await self.client.stop()
            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge '{self.edge_id}' stopped.")


class Place(base.Place):
    def __init__(
        self,
        place_id: str,
        edge_exec: str,
        edge_config: str,
        behavior_config: dict[str, Any] | None = None,
        log_config: dict | None = None,
        edge_enabled: bool = False,
        renderer_name: str | None = None,
        map_data: BaseMap | None = None,
    ):
        """
        Initialize the Place with an empty edge mapping.
        Args:
            place_id (str): Unique identifier for the place.
            edge_exec (str): Path to the edge process executable.
            edge_config (str): Configuration for the edges.
            behavior_config (dict[str, Any] | None): Explicit behavior config
                passed from the local World.
            log_config (dict | None): Optional logging configuration.
            edge_enabled (bool): Whether to enable edge clients.
            renderer_name (str | None): Name of the renderer to use.
            map_data (BaseMap | None): Road-network geometry for the renderer.
        """
        super().__init__(place_id, behavior_config=behavior_config, log_config=log_config)
        self.edges: dict[str, Edge] = {}
        self.edge_exec = edge_exec
        self.edge_config = edge_config
        self.edge_enabled = edge_enabled
        self.renderer_name = renderer_name
        self._shared_gpu_device = None
        self._shared_map_buffers: SharedMapBuffers | None = None
        self.renderer: Renderer | None = None
        if renderer_name:
            try:
                import wgpu

                adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
                self._shared_gpu_device = adapter.request_device_sync()
                _logger.info(f"Place {place_id} initialized shared GPU device for renderer '{renderer_name}'.")
                if map_data:
                    self._shared_map_buffers = create_shared_map_buffers(self._shared_gpu_device, map_data)
                self.renderer = new_renderer(
                    renderer_name,
                    device=self._shared_gpu_device,
                    shared_map_buffers=self._shared_map_buffers,
                )
            except KeyError:
                _logger.warning(f"Renderer '{renderer_name}' not found. Place {place_id} will run without rendering.")
                self.renderer = None
            except Exception as e:
                _logger.warning(f"Failed to initialize GPU resources for Place {place_id}: {e}")
                self.renderer = None

    @override
    async def add_edge(
        self,
        edge_id: str,
        init_state: dict[str, Any],
    ) -> None:
        if edge_id in self.edges:
            return
        edge = Edge(
            edge_id,
            init_state,
            behavior_config=self.behavior_config,
            edge_enabled=self.edge_enabled,
            edge_exec=self.edge_exec,
            edge_config=self.edge_config,
            place_id=self.place_id,
        )
        await edge.start()  # Start the edge's edge peer
        self.edges[edge_id] = edge
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {self.place_id} added edge {edge_id}.")

    @override
    async def step(self, updates: dict[str, dict[str, Any]]) -> dict[str, dict[str, Any]]:
        """
        Apply state updates to all edges and aggregate their actions.
        Args:
            updates (Dict[str, Dict[str, Any]]): Mapping of edge_id -> new state payload.
        Returns:
            Dict[str, Dict[str, Any]]: Aggregated mapping of edge_id -> action payload
        """

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {self.place_id} received updates for {list(updates.keys())}.")

        task2edge: dict[asyncio.Task, str] = {}

        for edge_id, new_state in updates.items():
            edge = self.get_edge(edge_id)

            if not edge:
                if new_state["status"] != "departed":
                    _logger.warning(
                        f"Place '{self.place_id}' received update for unknown edge '{edge_id}' with state: {new_state}. Adding it to the place anyway."
                    )

                await self.add_edge(
                    edge_id,
                    new_state,
                )
                # Retrieve edge after adding (must not be None)
                edge = self.get_edge(edge_id)
                if edge is None:
                    raise RuntimeError(f"Failed to add edge {edge_id} to place {self.place_id}")

            # Render camera images at Place level before passing to Edge.
            if self.renderer:
                new_state["camera_images"] = self.renderer.render(
                    float(new_state.get("x", 0.0)),
                    float(new_state.get("y", 0.0)),
                    float(new_state.get("angle", 0.0)),
                    new_state.get("neighbor_vehicles", []),
                    traffic_lights=new_state.get("next_tls"),
                )
            elif "camera_images" not in new_state:
                new_state["camera_images"] = []

            task = asyncio.create_task(edge.step(new_state))
            task2edge[task] = edge_id

        async def tag(edge_id: str, task: asyncio.Task):
            return edge_id, await task

        actions: dict[str, dict[str, Any]] = {}
        tagged_coros = [tag(edge_id, task) for task, edge_id in task2edge.items()]
        for coro in asyncio.as_completed(tagged_coros):
            edge_id, action = await coro

            if action["status"] == "arrived":
                # if _logger.isEnabledFor(logging.DEBUG):
                #     _logger.debug(
                #         f"Edge '{edge_id}' arrived at Place '{self.place_id}' with action: {action}"
                #     )
                self.remove_edge(edge_id)

            actions[edge_id] = action

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {self.place_id} completed step")

        return actions

    @override
    async def stop(self) -> None:
        """
        Stop all edges managed by this place.
        """
        for edge in self.edges.values():
            await edge.stop()

        self.edges.clear()

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {self.place_id} stopped all edges.")

    @override
    def set_shared_data(self, key: str, data) -> None:
        if key != "map":
            return
        if self._shared_gpu_device is not None and self.renderer_name:
            try:
                self._shared_map_buffers = create_shared_map_buffers(self._shared_gpu_device, data)
                self.renderer = new_renderer(
                    self.renderer_name,
                    device=self._shared_gpu_device,
                    shared_map_buffers=self._shared_map_buffers,
                )
                _logger.info(f"Place {self.place_id} recreated renderer with updated map data.")
            except Exception as e:
                _logger.warning(f"Failed to recreate renderer for Place {self.place_id}: {e}")


class _PlaceInfo:
    def __init__(self, place_id: str, place: Place):
        """
        Lightweight wrapper for a Place actor reference and edge count.
        Args:
            place_id (str): Unique identifier for the place.
            place (Place): Ray actor reference for the place.
        """
        self.place_id = place_id
        self.place = place
        self.edge_count = 0


class World(base.World):
    def __init__(self, *args, **kwargs: Any):
        """
        Initialize the World with Ray and manage places and edges.
        Args:
            *args: Positional arguments to pass to ray.init
            **kwargs: Keyword arguments for world configuration and ray.init except:
                - min_edges_per_place (int): Minimum edges per place (default: 1)
                - max_places (int): Maximum number of places (default: 4)
                - rust_log (str): Logging level for Rust components (default: "info")
                - rust_backtrace (str): Backtrace setting for Rust components (default: "0")
        """
        super().__init__()

        # Load dangerous-behavior config (registers packs once)
        behavior_config_path = kwargs.pop("edge_behavior_config", None)
        self.load_behavior_config(behavior_config_path)

        # Maintain a sorted list of PlaceWrappers for efficient least-populated place retrieval
        self.place_info_pool = SortedList(key=lambda x: -x.edge_count)

        # Extract world configuration from kwargs with defaults
        self.edge_enabled = bool(kwargs.pop("edge_enabled", False))
        self.renderer_name = kwargs.pop("renderer_name", None)
        self.edge_exec = str(kwargs.pop("edge_exec"))
        self.edge_config = str(kwargs.pop("edge_config"))
        self.min_edges_per_place = int(kwargs.pop("min_edges_per_place"))
        self.max_edges_per_place = int(kwargs.pop("max_edges_per_place"))
        self.max_places = int(kwargs.pop("max_places", "1"))

        if self.min_edges_per_place < 1:
            raise ValueError("min_edges_per_place must be at least 1")
        if self.max_edges_per_place < 1:
            raise ValueError("max_edges_per_place must be at least 1")
        if self.max_places < 1:
            raise ValueError("max_places must be at least 1")

        self.next_place_number = 1

        self.in_progress_tasks = []

    def _create_place_info(self) -> _PlaceInfo:
        """
        Create a new Place actor and wrap it in a _PlaceInfo.
        Returns:
            _PlaceInfo: Wrapper containing the new place ID and actor reference.
        """
        from ouroboros.util import global_log_config

        place_id = f"place_{self.next_place_number:05d}"
        self.next_place_number += 1
        place = Place(
            place_id,
            edge_exec=self.edge_exec,
            edge_config=self.edge_config,
            behavior_config=self.behavior_config,
            log_config=global_log_config,
            edge_enabled=self.edge_enabled,
            renderer_name=self.renderer_name,
            map_data=self.map_data,
        )
        self.places[place_id] = place
        return _PlaceInfo(place_id, place)

    @override
    async def accommodate_edge(self, edge_id: str, init_state: dict[str, Any]) -> str:
        """
        Accommodate the edge to the least-populated place and return the place ID.
        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (Dict[str, Any]): Initial state payload for the edge.
        Returns:
            str: The place ID where the edge is accommodated.
        """

        if edge_id in self.edge2place:
            raise ValueError(f"Edge {edge_id} is already assigned to a place.")

        if len(self.places) < self.max_places:  # noqa: SIM108
            place_info = self._create_place_info()
        else:
            # Use self.place_list to get the least populated place in O(log(N))
            place_info = self.place_info_pool.pop(-1)

        if place_info.edge_count >= self.max_edges_per_place:
            self.place_info_pool.add(place_info)
            raise base.EdgeCapacityExceeded(
                f"Cannot accommodate edge {edge_id}: all places have reached max capacity of {self.max_edges_per_place} edges."
            )

        await place_info.place.add_edge(edge_id, init_state)
        place_info.edge_count += 1
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Accommodating edge {edge_id} to place {place_info.place_id}.")
        self.place_info_pool.add(place_info)

        return place_info.place_id

    @override
    async def batch_step(self, batch_updates: dict[str, dict[str, dict[str, Any]]]) -> dict[str, dict[str, Any]]:
        """
        Process batched updates per place and return aggregated actions.
        Args:
            batch_updates (Dict[str, Dict[str, Dict[str, Any]]]): Mapping of place_id -> (edge_id -> new state payload).
        Returns:
            Dict[str, Dict[str, Any]]: Aggregated mapping of edge_id -> action payload
        """

        # Synchronize any in-progress tasks
        self.balance_places()

        actions = {}

        refs = [
            await self.places[place_id].step(edge_updates)
            for place_id, edge_updates in batch_updates.items()
            if place_id in self.places
        ]

        for place_actions in refs:
            for edge_id, action in place_actions.items():
                if action["status"] == "arrived":
                    if _logger.isEnabledFor(logging.DEBUG):
                        _logger.debug(f"Edge '{edge_id}' arrived")
                    self.remove_edge(edge_id)

            actions.update(place_actions)

        return actions

    @override
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
                # Finds the corresponding place wrapper and update its edge count
                place_wrapper = next(pw for pw in self.place_info_pool if pw.place_id == place_id)
                self.place_info_pool.remove(place_wrapper)
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug(f"Removing edge {edge_id} from place {place_id}.")
                del self.edge2place[edge_id]
                place_wrapper.edge_count -= 1
                # Re-add the updated place wrapper to the sorted list
                self.place_info_pool.add(place_wrapper)

    @override
    async def stop(self) -> None:
        """
        Stop any edge processes and clean up resources.
        """
        for place in self.places.values():
            await place.stop()
        _logger.info("World stopped and resources cleaned up.")
