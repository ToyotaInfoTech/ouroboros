# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Ray-based orchestration layer for distributed edge simulation.

This module integrates asyncio and Ray to coordinate many simulated edges across
a set of Ray actors ("places"). Each place hosts edges that proxy edge
state updates to an external edge process via RPC over IPC, and return computed actions.

Overview:
- Edge:
    Bridges the cluster with an edge peer process using an RpcClient over a UNIX
    domain socket. It serializes updates and deserializes actions using FlatBuffers.
- Place (Ray actor):
    Manages a collection of Edge instances for a single place. It applies
    state updates concurrently to edges and aggregates their resulting actions.
- _NodeInfo:
    Tracks per-node metadata: wgpu benchmark score (TFLOPS) and number of
    Place actors currently scheduled on that node.
- _PlaceInfo:
    Lightweight holder for a Ray actor reference, edge-count bookkeeping, and
    the node ID the actor is pinned to.
- World:
    Top-level coordinator that:
    - Initializes Ray and runs wgpu benchmarks on all alive nodes.
    - Allocates new Places to nodes proportionally to their benchmark scores.
    - Accommodates edges by assigning them to the least-populated place.
    - Executes batch steps by dispatching updates to places and collecting actions.
    - Tracks in-progress Ray tasks.

Key behaviors:
- Placement policy:
    At startup, wgpu compute benchmarks (TFLOPS) are run in parallel across all
    Ray nodes.  New Place actors are pinned to nodes via
    NodeAffinitySchedulingStrategy.  The target place count per node is
    proportional to its benchmark score::

        target_i = max_places * score_i / sum(scores)

    When a new Place should be created (all existing places are "satisfied",
    i.e., have >= min_edges_per_place edges, and total places < max_places),
    the node with the smallest deficit ratio
    ``place_count / target`` is selected.

    Once max_places is reached, edges are assigned to the Place with the
    fewest edges.  If all Places are at max_edges_per_place, the assignment is
    rejected with EdgeCapacityExceeded.
- Concurrency:
    Edge steps within a place are scheduled with asyncio tasks and awaited as
    they complete.  Place-level steps are invoked concurrently via Ray and
    gathered with ray.get.
- Configuration:
    World accepts min_edges_per_place (>=1), max_edges_per_place (>=1),
    max_places (>=1, default: 50).  Ray is initialized with the remaining
    kwargs provided to World.

Error handling:
- Attempting to reassign an already accommodated edge raises ValueError.
- Invalid configuration values raise ValueError.
- If a node's wgpu benchmark fails, it receives a score of 0.0 and is excluded
  from placement unless all nodes score 0.0, in which case uniform distribution
  is used.

Dependencies and assumptions:
- Requires Ray for actor-based distribution and asyncio for intra-actor concurrency.
- Uses FlatBuffers-based serialization helpers (fb_serialize/fb_deserialize).
- Expects an external edge executable reachable via a UNIX domain socket.
- Relies on base classes in world.base (e.g., Edge, Place, World)
    and optional global logging configuration (util.global_log_config).
"""

import asyncio
import logging
import os
from dataclasses import dataclass, field
from threading import Lock
from typing import Any, override

import ray
from ray.util.scheduling_strategies import NodeAffinitySchedulingStrategy

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
        renderer: Renderer | None = None,
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
                injected by the World for ray workers.
            renderer (Renderer | None): Renderer instance created and owned by Place.
        """

        super().__init__(edge_id, init_state, behavior_config=behavior_config)

        self.renderer = renderer

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
        self.state = new_state

        if self.client:
            # Inject the currently active behavior name into state before
            # serialization so that ouroedge can record it in system_state.
            new_state["active_behavior"] = self._behavior_runtime.active_behavior_name

            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug(f"Edge {self.edge_id} sending new state")

            # We don't expect the action returned from the edge peer
            # We directry apply behavior patches on top of the new state and return that as the action.
            _response = await self.client.call("step", fb_serialize(new_state))

            if self.state["status"] == "arrived":
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug(f"Edge {self.edge_id} arrived at Place with state: {new_state}")
                await self.stop()

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


@ray.remote
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
                passed from the ray World process.
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

        # Store renderer configuration for Edge creation
        self.renderer_name = renderer_name

        # Initialize shared GPU device once per Place (if rendering is enabled)
        self._shared_gpu_device = None
        self._shared_map_buffers: SharedMapBuffers | None = None
        if renderer_name:
            try:
                import wgpu

                adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
                self._shared_gpu_device = adapter.request_device_sync()
                _logger.info(f"Place {place_id} initialized shared GPU device for renderer '{renderer_name}'.")

                # Create shared map buffers once per Place (avoid redundant GPU uploads)
                if map_data:
                    self._shared_map_buffers = create_shared_map_buffers(
                        self._shared_gpu_device,
                        map_data,
                    )
                    _logger.info(f"Place {place_id} created shared map buffers for all Edges.")
            except Exception as e:
                _logger.warning(f"Failed to initialize shared GPU resources for Place {place_id}: {e}")

    @override
    async def add_edge(
        self,
        edge_id: str,
        init_state: dict[str, Any],
    ) -> None:
        if edge_id in self.edges:
            return

        # Place is responsible for all GPU resource management; create renderer here.
        # The renderer works on both GPU and CPU (GPU is faster); always attempt
        # creation when renderer_name is configured.
        renderer: Renderer | None = None
        if self.renderer_name:
            try:
                renderer = new_renderer(
                    self.renderer_name,
                    device=self._shared_gpu_device,
                    shared_map_buffers=self._shared_map_buffers,
                )
            except KeyError:
                _logger.warning(
                    f"Renderer '{self.renderer_name}' not found. Edge {edge_id} will run without rendering."
                )

        edge = Edge(
            edge_id,
            init_state,
            behavior_config=self.behavior_config,
            edge_enabled=self.edge_enabled,
            edge_exec=self.edge_exec,
            edge_config=self.edge_config,
            place_id=self.place_id,
            renderer=renderer,
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

        async def prepare_and_step_edge(edge_id: str, new_state: dict[str, Any]) -> tuple[str, dict[str, Any]]:
            """
            Prepare a single edge update by ensuring it exists, rendering camera images,
            and executing the edge step.

            Args:
                edge_id (str): Unique identifier for the edge.
                new_state (Dict[str, Any]): New state payload for the edge.

            Returns:
                Tuple[str, Dict[str, Any]]: The edge_id and its action result.
            """
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

            # Render camera images at Edge level using Edge's dedicated Renderer.
            if edge.renderer:
                new_state["camera_images"] = edge.renderer.render(
                    float(new_state.get("x", 0.0)),
                    float(new_state.get("y", 0.0)),
                    float(new_state.get("angle", 0.0)),
                    new_state.get("neighbor_vehicles", []),
                    traffic_lights=new_state.get("next_tls"),
                )
            elif "camera_images" not in new_state:
                new_state["camera_images"] = []

            action = await edge.step(new_state)

            action["status"] = new_state["status"]

            return edge_id, action

        # Create tasks for all edge updates in parallel
        tasks = [
            asyncio.create_task(prepare_and_step_edge(edge_id, new_state)) for edge_id, new_state in updates.items()
        ]

        # Collect results as they complete
        actions: dict[str, dict[str, Any]] = {}
        for coro in asyncio.as_completed(tasks):
            edge_id, action = await coro

            if action["status"] == "arrived":
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug(f"Edge '{edge_id}' arrived at Place '{self.place_id}' with action: {action}")
                self.remove_edge(edge_id)

            actions[edge_id] = action

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Place {self.place_id} completed step with actions: {actions}")

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

        # Recreate shared map buffers if using shared GPU device
        if self._shared_gpu_device is not None and self.renderer_name:
            try:
                self._shared_map_buffers = create_shared_map_buffers(
                    self._shared_gpu_device,
                    data,
                )
                _logger.info(f"Place {self.place_id} recreated shared map buffers with updated data.")
                # Note: Existing Edge renderers still reference old buffers.
                # They will continue to render with old map data until recreated.
                # This is acceptable as map data updates are rare and new Edges will use new buffers.
            except Exception as e:
                _logger.warning(f"Failed to recreate shared map buffers for Place {self.place_id}: {e}")


@ray.remote
def _run_wgpu_benchmark_remote() -> float:
    """Run a wgpu compute benchmark and return the estimated TFLOPS.

    The benchmark mirrors ``tool/wgpu_bench.py``: a 256-thread compute shader
    performs 50 000 multiply-add iterations over N = 2^23 f32 elements.
    A warm-up pass is executed first so that driver/JIT overhead is excluded
    from the measurement.

    Returns:
        float: Estimated TFLOPS.  0.0 when wgpu is unavailable or the
            benchmark fails for any reason.
    """
    try:
        import time

        import wgpu

        N = 1 << 23
        LOOP_ITERS = 50_000

        adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
        device = adapter.request_device_sync()

        buffer = device.create_buffer(
            size=N * 4,
            usage=wgpu.BufferUsage.STORAGE | wgpu.BufferUsage.COPY_SRC | wgpu.BufferUsage.COPY_DST,
        )

        shader_code = f"""
@group(0) @binding(0)
var<storage, read_write> data: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let idx = gid.x;
    var x0: f32 = f32(idx);
    var x1: f32 = f32(idx + 1u);
    var x2: f32 = f32(idx + 2u);
    var x3: f32 = f32(idx + 3u);
    for (var i: u32 = 0u; i < {LOOP_ITERS}u; i = i + 1u) {{
        x0 = x0 * 1.000001 + 0.000001;
        x1 = x1 * 1.000002 + 0.000002;
        x2 = x2 * 1.000003 + 0.000003;
        x3 = x3 * 1.000004 + 0.000004;
    }}
    data[idx] = x0 + x1 + x2 + x3;
}}
"""
        shader = device.create_shader_module(code=shader_code)
        bind_group_layout = device.create_bind_group_layout(
            entries=[
                {
                    "binding": 0,
                    "visibility": wgpu.ShaderStage.COMPUTE,
                    "buffer": {"type": wgpu.BufferBindingType.storage},
                }
            ]
        )
        pipeline = device.create_compute_pipeline(
            layout=device.create_pipeline_layout(bind_group_layouts=[bind_group_layout]),
            compute={"module": shader, "entry_point": "main"},
        )
        bind_group = device.create_bind_group(
            layout=bind_group_layout,
            entries=[{"binding": 0, "resource": {"buffer": buffer}}],
        )

        def _dispatch():
            enc = device.create_command_encoder()
            p = enc.begin_compute_pass()
            p.set_pipeline(pipeline)
            p.set_bind_group(0, bind_group, [])
            p.dispatch_workgroups((N + 255) // 256, 1, 1)
            p.end()
            device.queue.submit([enc.finish()])
            device.queue.on_submitted_work_done_sync()

        _dispatch()  # warm-up
        t0 = time.time()
        _dispatch()
        elapsed = time.time() - t0

        flops = N * LOOP_ITERS * 4 * 2  # 4 registers × 2 FLOPs (mul + add)
        return flops / elapsed / 1e12
    except Exception:
        return 0.0


@dataclass
class _NodeInfo:
    """Per-node metadata for benchmark-proportional Place allocation.

    Attributes:
        node_id: Ray node ID string.
        wgpu_score: Estimated wgpu compute performance in TFLOPS.
            0.0 means the benchmark failed (e.g. no GPU / no wgpu).
        place_count: Number of Place actors currently scheduled on this node.
    """

    node_id: str
    wgpu_score: float
    place_count: int = field(default=0)


def _select_best_node(node_infos: dict[str, _NodeInfo], max_places: int) -> _NodeInfo:
    """Select the node that is most underserved relative to its benchmark score.

    The target place count for each node is proportional to its wgpu score::

        target_i = max_places * score_i / sum(scores)

    The node with the smallest deficit ratio ``place_count / target`` is
    returned.  When ``target`` is 0 (score 0 while other nodes have non-zero
    scores), the ratio is treated as infinity so that the node is never chosen.
    If all nodes score 0.0 the target is distributed uniformly.

    Args:
        node_infos: Mapping of node_id -> _NodeInfo.
        max_places: Global maximum number of Place actors.

    Returns:
        The _NodeInfo with the minimum deficit ratio.

    Raises:
        ValueError: If ``node_infos`` is empty.
    """
    if not node_infos:
        raise ValueError("No nodes available for Place allocation.")

    num_nodes = len(node_infos)
    sum_scores = sum(n.wgpu_score for n in node_infos.values())

    def deficit_ratio(node: _NodeInfo) -> float:
        if sum_scores == 0.0:
            # All nodes failed the benchmark: distribute uniformly.
            normalized_capacity = max_places / num_nodes
        elif node.wgpu_score == 0.0:
            # This node has no GPU performance; never assign Places here.
            return float("inf")
        else:
            normalized_capacity = max_places * node.wgpu_score / sum_scores
        if normalized_capacity == 0.0:
            return float("inf")
        return node.place_count / normalized_capacity

    return min(node_infos.values(), key=deficit_ratio)


class _PlaceInfo:
    """Lightweight wrapper for a Place actor reference and edge count.

    Attributes:
        place_id: Unique identifier for the place.
        inner: Ray actor reference for the Place.
        node_id: Ray node ID on which this Place actor is scheduled.
        edge_count: Number of edges currently hosted by this place.
    """

    def __init__(self, place_id: str, place: "Place", node_id: str):
        """
        Args:
            place_id (str): Unique identifier for the place.
            place (Place): Ray actor reference for the place.
            node_id (str): Ray node ID on which this Place is pinned.
        """
        self.place_id = place_id
        self.inner = place
        self.node_id = node_id
        self.edge_count = 0
        self._lock = Lock()

    def increase_edge_count_or_fail(self, max_edges_per_place: int) -> bool:
        """Atomically increment edge_count if below the cap.

        Args:
            max_edges_per_place (int): Maximum allowed edges for a single place.

        Returns:
            bool: True when the increment succeeded; False when the place is full.
        """
        with self._lock:
            if self.edge_count >= max_edges_per_place:
                return False
            self.edge_count += 1
            return True

    def decrease_edge_count(self) -> None:
        """Decrement edge_count, clamped to zero."""
        with self._lock:
            if self.edge_count > 0:
                self.edge_count -= 1

    def __getattr__(self, name):
        return getattr(self.inner, name)

    def __repr__(self):
        return f"_PlaceInfo(place_id={self.place_id}, node_id={self.node_id}, edge_count={self.edge_count})"


class World(base.World):
    def __init__(self, *args, **kwargs: Any):
        """Initialize the World with Ray and manage places and edges.

        After Ray is initialized, wgpu compute benchmarks are executed in
        parallel on every alive cluster node.  The results drive proportional
        Place allocation via :func:`_select_best_node`.

        Args:
            *args: Positional arguments forwarded to ``ray.init``.
            **kwargs: Keyword arguments.  The following keys are consumed
                before the remainder is forwarded to ``ray.init``:

                - ``ray_addr`` (str | None): Ray cluster address.
                - ``edge_behavior_config`` (str | None): Path to behavior config.
                - ``edge_enabled`` (bool): Enable edge RPC clients. [default: False]
                - ``renderer_name`` (str | None): Renderer name.
                - ``edge_exec`` (str): Path to the edge executable.
                - ``edge_config`` (str): Path to the edge config file.
                - ``min_edges_per_place`` (int): Minimum edges per place.
                - ``max_edges_per_place`` (int): Maximum edges per place.
                - ``max_places`` (int): Maximum total places. [default: 50]
                - ``num_cpus_per_place`` (float): CPUs to allocate per Place actor. [default: 0.5]
                - ``num_gpus_per_place`` (float): GPUs to allocate per Place actor. [default: 0.0]
                - ``rust_log`` (str): Rust log level. [default: "info"]
                - ``rust_log_style`` (str): Rust log style. [default: "auto"]
                - ``rust_backtrace`` (str): Rust backtrace. [default: "0"]
        """
        super().__init__()

        # Load dangerous-behavior config (registers packs once)
        behavior_config_path = kwargs.pop("edge_behavior_config", None)
        self.load_behavior_config(behavior_config_path)

        self._lock = Lock()

        # Flat list of _PlaceInfo objects (replaces the former SortedList).
        self.place_info_pool: list[_PlaceInfo] = []

        # Extract world configuration from kwargs with defaults
        self.edge_enabled = bool(kwargs.pop("edge_enabled", False))
        self.renderer_name = str(kwargs.pop("renderer_name"))
        self.edge_exec = str(kwargs.pop("edge_exec"))
        self.edge_config = str(kwargs.pop("edge_config"))
        self.min_edges_per_place = int(kwargs.pop("min_edges_per_place"))
        self.max_edges_per_place = int(kwargs.pop("max_edges_per_place"))
        self.max_places = int(kwargs.pop("max_places", "50"))
        self.num_cpus_per_place = float(kwargs.pop("num_cpus_per_place", 0.5))
        self.num_gpus_per_place = float(kwargs.pop("num_gpus_per_place", 0.0))

        if self.min_edges_per_place < 1:
            raise ValueError("min_edges_per_place must be at least 1")
        if self.max_edges_per_place < 1:
            raise ValueError("max_edges_per_place must be at least 1")
        if self.max_places < 1:
            raise ValueError("max_places must be at least 1")

        # Ensure Ray's accelerator env var override is disabled
        os.environ["RAY_ACCEL_ENV_VAR_OVERRIDE_ON_ZERO"] = "0"
        # Initialize Ray with remaining kwargs
        ray.init(
            kwargs.pop("ray_addr", None),
            ignore_reinit_error=True,
            runtime_env={
                "env_vars": {
                    "RUST_LOG": kwargs.pop("rust_log", "info"),
                    "RUST_LOG_STYLE": kwargs.pop("rust_log_style", "auto"),
                    "RUST_BACKTRACE": kwargs.pop("rust_backtrace", "0"),
                }
            },
        )
        _logger.info(ray.available_resources())

        # Run wgpu benchmarks on all alive nodes in parallel.
        self._node_infos: dict[str, _NodeInfo] = {}
        alive_nodes = [n for n in ray.nodes() if n["Alive"]]
        if not alive_nodes:
            _logger.warning("No alive Ray nodes found; Place allocation will fail until nodes join.")
        else:
            bench_refs: dict[str, ray.ObjectRef] = {}
            for n in alive_nodes:
                node_id = n["NodeID"]
                try:
                    ref = _run_wgpu_benchmark_remote.options(
                        scheduling_strategy=NodeAffinitySchedulingStrategy(node_id=node_id, soft=False)
                    ).remote()
                    bench_refs[node_id] = ref
                except Exception as exc:
                    _logger.warning(f"Could not schedule benchmark on node {node_id}: {exc}")

            for node_id, ref in bench_refs.items():
                try:
                    score: float = ray.get(ref)
                except Exception as exc:
                    _logger.warning(f"Benchmark failed on node {node_id}: {exc}")
                    score = 0.0
                self._node_infos[node_id] = _NodeInfo(node_id=node_id, wgpu_score=score)
                _logger.info(f"Node {node_id} wgpu_score={score:.4f} TFLOPS")

        self.next_place_number = 0
        self.in_progress_tasks = []

        _logger.info(f"World initialized. Places will be created on demand (max={self.max_places}).")

    def _create_place_info(self, node_id: str) -> _PlaceInfo:
        """Create a new Place actor pinned to the given node and wrap it.

        The actor is pinned via :class:`NodeAffinitySchedulingStrategy` with
        ``soft=False`` so that Ray always schedules it on the intended node.
        Resource declarations use ``num_cpus`` and ``num_gpus`` from
        ``World.num_cpus_per_place`` / ``World.num_gpus_per_place``; wgpu accesses
        the GPU directly without Ray's resource tracking.

        Args:
            node_id (str): Ray node ID to schedule the Place actor on.

        Returns:
            _PlaceInfo: Wrapper containing the new place ID, actor reference,
                and node ID.
        """
        from ouroboros.util import global_log_config

        place_id = f"place_{self.next_place_number + 1:05d}"
        self.next_place_number += 1

        place = Place.options(
            num_cpus=self.num_cpus_per_place,
            num_gpus=self.num_gpus_per_place,
            scheduling_strategy=NodeAffinitySchedulingStrategy(node_id=node_id, soft=False),
        ).remote(
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

        _logger.info(
            f"Created Place {place_id} on node {node_id} (num_cpus={self.num_cpus_per_place}, num_gpus={self.num_gpus_per_place})"
        )

        return _PlaceInfo(place_id, place, node_id)

    @override
    async def accommodate_edge(self, edge_id: str, init_state: dict[str, Any]) -> str:
        """Accommodate the edge in an appropriate Place and return the place ID.

        Decision logic
        --------------
        1. **Create a new Place** when:
           - total Place count is below ``max_places``, *and*
           - the pool is empty (no Places yet), *or* every existing Place
             already has at least ``min_edges_per_place`` edges.
           The new Place is pinned to the node with the lowest deficit ratio
           (see :func:`_select_best_node`).

        2. **Use the existing Place with the fewest edges** when condition 1
           is not met (some Place still has fewer than ``min_edges_per_place``
           edges, or ``max_places`` has been reached).

        3. **Raise** :class:`~ouroboros.world.base.EdgeCapacityExceeded` when
           the selected Place is already at ``max_edges_per_place``.

        Args:
            edge_id (str): Unique identifier for the edge.
            init_state (dict): Initial state payload for the edge.

        Returns:
            str: The place ID where the edge was accommodated.

        Raises:
            ValueError: When ``edge_id`` is already assigned to a place.
            EdgeCapacityExceeded: When no Place can accept another edge.
        """
        if edge_id in self.edge2place:
            raise ValueError(f"Edge {edge_id} is already assigned to a place.")

        with self._lock:
            all_satisfied = all(pinfo.edge_count >= self.min_edges_per_place for pinfo in self.place_info_pool)
            should_create = len(self.places) < self.max_places and (len(self.place_info_pool) == 0 or all_satisfied)

            if should_create:
                if not self._node_infos:
                    raise base.EdgeCapacityExceeded("Cannot create a Place: no benchmarked nodes available.")
                best_node = _select_best_node(self._node_infos, self.max_places)
                place_info = self._create_place_info(best_node.node_id)
                best_node.place_count += 1
                # place_info is added to pool after edge count increment below
            else:
                if not self.place_info_pool:
                    raise base.EdgeCapacityExceeded(
                        f"Cannot accommodate edge {edge_id}: max_places ({self.max_places}) "
                        "reached and no places are available."
                    )
                place_info = min(self.place_info_pool, key=lambda p: p.edge_count)
                self.place_info_pool.remove(place_info)

        if not place_info.increase_edge_count_or_fail(self.max_edges_per_place):
            self.place_info_pool.append(place_info)
            raise base.EdgeCapacityExceeded(
                f"Cannot accommodate edge {edge_id}: all places have reached "
                f"max capacity of {self.max_edges_per_place} edges."
            )

        task = place_info.add_edge.remote(edge_id, init_state)
        _logger.debug(f"Accommodating edge {edge_id} to place {place_info.place_id}.")
        self.in_progress_tasks.append(task)
        self.place_info_pool.append(place_info)

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
        ray.wait(self.in_progress_tasks, num_returns=len(self.in_progress_tasks))
        self.in_progress_tasks.clear()
        self.balance_places()

        actions = {}

        refs = [
            self.places[place_id].step.remote(edge_updates)
            for place_id, edge_updates in batch_updates.items()
            if place_id in self.places
        ]

        for place_actions in ray.get(refs):
            for edge_id, action in place_actions.items():
                if action["status"] == "arrived":
                    if _logger.isEnabledFor(logging.DEBUG):
                        _logger.debug(f"Edge '{edge_id}' arrived at World with action: {action}")
                    self.remove_edge(edge_id)

            actions.update(place_actions)

        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"World batch step completed with actions: {actions}")

        return actions

    @override
    def remove_edge(self, edge_id: str) -> None:
        """
        Remove the edge with the given ID from its place.
        Args:
            edge_id (str): Unique identifier for the edge to remove.
        """

        with self._lock:
            place_id = self.edge2place.get(edge_id, None)
            if place_id:
                place = self.places.get(place_id, None)
                if place:
                    # Finds the corresponding place wrapper and update its edge count
                    place_info = next(pinfo for pinfo in self.place_info_pool if pinfo.place_id == place_id)
                    self.place_info_pool.remove(place_info)
                    if _logger.isEnabledFor(logging.DEBUG):
                        _logger.debug(f"Removing edge {edge_id} from place {place_id}.")
                    del self.edge2place[edge_id]
                    place_info.decrease_edge_count()
                    # Re-add the updated place info to the pool
                    self.place_info_pool.append(place_info)

    @override
    async def notify_arrived(self, edge_ids) -> None:
        """
        Stop and remove edges that have arrived (left the simulation).
        Uses Ray remote calls since places are Ray actors.
        Args:
            edge_ids: Iterable of edge IDs that have arrived.
        """
        refs = []
        arrived = []
        for edge_id in edge_ids:
            place_id = self.edge2place.get(edge_id, None)
            if place_id:
                place = self.places.get(place_id, None)
                if place:
                    refs.append(place.stop_edge.remote(edge_id))
                    arrived.append(edge_id)
        if refs:
            ray.get(refs)
        for edge_id in arrived:
            self.remove_edge(edge_id)

    @override
    async def stop(self) -> None:
        """
        Stop any edge processes and clean up resources.
        """
        for place in self.places.values():
            ray.get(place.stop.remote())

        _logger.info("World stopped and resources cleaned up.")

    @override
    def set_shared_data(self, key: str, data) -> None:
        """Distribute shared data to all Ray Place actors."""
        refs = [place.set_shared_data.remote(key, data) for place in self.places.values()]
        if refs:
            ray.get(refs)
