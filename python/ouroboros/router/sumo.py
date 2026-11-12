# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
A SUMO/TraCI router that subscribes to vehicle state, produces structured observations,
and applies control actions back into the simulator.

This module provides SumoRouter, a runtime bridge between a SUMO simulation and a
user-defined "world" policy. It manages TraCI connections, efficiently gathers per-vehicle
state via subscriptions, and relays actions such as speed targets and lane changes.

Key responsibilities:
- Establish and manage a TraCI session:
    - Connect to a remote SUMO instance with automatic retry, or launch a local one.
- Efficient state collection:
    - Subscribe vehicles to a curated set of variables once per vehicle.
    - Use context subscriptions (within 50m) to fetch nearby vehicles and build a limited
        neighborhood view for each ego vehicle.
    - Convert planar SUMO coordinates (x, y) to geographic (lon, lat) using SUMO’s geo
        converter for downstream use.
    - Populate additional metadata including leader info, up to five upcoming traffic lights,
        and selected emissions metrics.
- Action application:
    - Accept a per-vehicle action dictionary from the world policy and apply supported
        controls:
        - "target_speed": setSpeed for immediate speed targets.
        - "lane_change": changeLane with a fixed duration.
- Logging:
    - Optional JSONL logging of the full state snapshot per tick for reproduciability and offline analysis.

State structure (per vehicle):
- Core kinematics and localization: pos (x, y, lon, lat), speed, accel, angle.
- Lane/road context: lane_id, lane_idx, lane_pos, road_id.
- Progress and emissions: dist, wait_time, fuel_cons, co2, co, nox, pmx, noise, type_id.
- Situational awareness:
    - leader: (leader_id, gap) or fallback ("", -1.0).
    - next_tls: up to 5 upcoming traffic signal entries with id, idx, dist, and signal state.
    - neighbor_vehicles: up to 10 nearby vehicles from context subscription (same core fields).

Workflow:
1) Create a SumoRouter, optionally with log_out="path/to.jsonl".
2) Connect to a running SUMO (connect_to_sim(host, port, ...)) or start one (launch_sim(sim_cmd)).
3) Call run() to step the simulation until there are no vehicles left, while:
     - Gathering observations every tick,
     - Calling world.step(states) to obtain actions,
     - Applying those actions back to SUMO.

Requirements and assumptions:
- SUMO with TraCI support must be installed and accessible.
- The world object (created via cluster.new_world) must implement step(states) -> actions,
    where actions is a dict keyed by vehicle ID with optional "target_speed" and "lane_change".
- Not all TraCI variables are reliably available via subscription across SUMO versions;
    this router subscribes to a known-working subset to reduce runtime errors.

Error handling:
- Connection attempts will retry on FatalTraCIError.
- Per-vehicle state retrieval is guarded; failures are logged and skipped.
- Logging errors are caught and reported without stopping the simulation.

Extensibility:
- Add or remove variables in the internal map to tune subscriptions.
- Adjust context radius or neighbor sampling limits to balance fidelity and throughput.
- Extend action handling to include additional controls (e.g., setAccel, setMaxSpeed).

Caveats:
- The router is designed for single-threaded use and calls traci.simulationStep() directly.
- Reliability of some subscription variables may vary with SUMO version and configuration.
- Coordinate conversion depends on SUMO’s geo settings and network projection.

"""

import asyncio
import contextlib
import itertools
import json
import logging
import os
import time
from enum import Enum
from typing import Any

import traci

from ouroboros.world.map import BaseMap
from ouroboros.world.renderer import Renderer, new_renderer

from ..world import World
from . import Router

_logger = logging.getLogger(__name__)

# Map state keys to TraCI constant names (as strings)
_VAR_MAP = {
    # Keep to variables known to work with vehicle subscriptions
    "pos": "VAR_POSITION",
    "angle": "VAR_ANGLE",
    "speed": "VAR_SPEED",
    "accel": "VAR_ACCELERATION",
    "lane_id": "VAR_LANE_ID",
    "lane_idx": "VAR_LANE_INDEX",
    "lane_pos": "VAR_LANEPOSITION",
    "road_id": "VAR_ROAD_ID",
    "wait_time": "VAR_WAITING_TIME",
    "dist": "VAR_DISTANCE",
    "fuel_cons": "VAR_FUELCONSUMPTION",
    "co2": "VAR_CO2EMISSION",
    "co": "VAR_COEMISSION",
    "nox": "VAR_NOXEMISSION",
    "pmx": "VAR_PMXEMISSION",
    "noise": "VAR_NOISEEMISSION",
    "type_id": "VAR_TYPE",
    # VAR_NEXT_TLS is reliably returned by subscription in SUMO 1.x and eliminates
    # one per-vehicle TraCI round-trip per step.
    "next_tls": "VAR_NEXT_TLS",
    # Note: VAR_LEADER requires a distance parameter that is not natively
    # supported by the standard subscribe() call across SUMO versions, so it
    # is kept as a direct per-vehicle call (see _get_leader).
}

_CONTEXT_KEYS = [  # must be a subset of var_map keys
    "pos",
    "angle",
    "speed",
    "accel",
    "road_id",
    "lane_id",
    "lane_idx",
]

_CSV_HEADERS = [
    "vin",
    "ts",
    "x",
    "y",
    "lon",
    "lat",
    "speed",
    "accel",
    "lane_id",
    "lane_idx",
    "lane_pos",
    "road_id",
    "wait_time",
    "dist",
    "fuel_cons",
    "co2",
    "co",
    "nox",
    "pmx",
    "noise",
    "type_id",
]


class _LogFormat(Enum):
    """Supported log output formats."""

    JSONL = "jsonl"
    CSV = "csv"


class SumoRouter(Router):
    def __init__(
        self,
        log_path: str | None = None,
        log_overwrite: bool = False,
        renderer_name: str | None = None,
        throughput_interval_secs: float = 5.0,
    ):
        """
        A SUMO/TraCI router that bridges a SUMO simulation with a world policy.

        Args:
            renderer_name (str | None, optional): Renderer name for log-mode image capture.
            log_path (str, optional): Path to output JSONL log file. If provided,
                the router will log the full vehicle state snapshot at each tick.
            log_overwrite (bool, optional): Whether to allow overwriting the log file if it exists.
            throughput_interval_secs (float, optional): Interval in seconds to log simulation
                throughput metrics.
        """

        super().__init__()

        self.throughput_interval_secs = throughput_interval_secs

        self.log_out = None
        self.image_dir = None
        self.log_renderer: Renderer | None = None
        self._log_renderer_device = None

        if log_path:
            if log_path.endswith(".jsonl"):
                self.log_format = _LogFormat.JSONL
            elif log_path.endswith(".csv"):
                self.log_format = _LogFormat.CSV
            else:
                raise ValueError("Unsupported log format. Use .jsonl or .csv file extension.")

            if not log_overwrite and os.path.exists(log_path):
                raise FileExistsError(f"Log file {log_path} already exists.")
            self.log_out = open(log_path, "w")  # noqa: SIM115

            self.image_dir = os.path.splitext(log_path)[0] + "_images"
            if not log_overwrite and os.path.exists(self.image_dir):
                raise FileExistsError(f"Image directory {self.image_dir} already exists.")
            os.makedirs(self.image_dir, exist_ok=True)

            # log_renderer is created in run() after SUMO is connected and map
            # data is available (SharedMapBuffers requires a live GPU device).

        self.renderer_name = renderer_name
        self._static_data_pushed = False

        self.vins = set()

        # Edge (road) ID -> number of lanes (static, cached on first use).
        self._edge_lane_count: dict[str, int] = {}

        # Auto-incrementing POI counter for unique IDs
        self._poi_seq: int = 0

        # Resolve names to traci.constants only once
        self._resolved_vars = {key: getattr(traci.constants, name) for key, name in _VAR_MAP.items()}

        self._resolved_context_vars = {key: self._resolved_vars[key] for key in _CONTEXT_KEYS}

        self._vars_to_subscribe = list(self._resolved_vars.values())
        self._vars_to_context_subscribe = list(self._resolved_context_vars.values())

        # Frame-local leader cache: used as fallback when subscribeLeader data
        # is not yet available (e.g. very first step for a new vehicle).
        self._leader_cache_sim_time: float | None = None
        self._leader_cache: dict[str, tuple[str, float]] = {}

        # Local geo projection: set up in run() from SumoMap to avoid calling
        # traci.simulation.convertGeo() once per vehicle per step.
        self._geo_proj: Any = None
        self._geo_proj_offset: tuple[float, float] = (0.0, 0.0)

    def _save_base_time(self) -> None:
        self.base_time = time.time() - traci.simulation.getTime()

    def connect_to_sim(self, host, port, **kwargs) -> None:
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Connecting to remote simulator at {host}:{port}...")
        while True:
            try:
                traci.init(host=host, port=port, **kwargs)
                break
            except traci.exceptions.FatalTraCIError:
                if _logger.isEnabledFor(logging.DEBUG):
                    _logger.debug("Connection failed, retrying...")
                time.sleep(0.2)
        self._save_base_time()
        _logger.info(f"Connected to remote simulator on {host}:{port}.")

    def launch_sim(self, sim_cmd: list[str]) -> None:
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"Starting simulator with command: {' '.join(sim_cmd)}")
        traci.start(sim_cmd)
        self._save_base_time()
        _logger.info("Local simulator started.")

    def _ensure_subscriptions(self) -> set[str]:
        """Subscribe desired vehicle variables for all current vehicles.

        Returns:
            set[str]: The current set of vehicle IDs.
        """
        current_vins = set(traci.vehicle.getIDList())
        new_vins = current_vins - self.vins
        self.vins = current_vins

        for vin in new_vins:
            traci.vehicle.subscribe(vin, self._vars_to_subscribe)
            # Subscribe to leader separately with a distance parameter (not
            # supported by the standard subscribe() call).  SUMO 1.9+ merges
            # multiple subscribe() calls for the same vehicle.
            traci.vehicle.subscribeLeader(vin, dist=0.0)
            traci.vehicle.subscribeContext(
                vin,
                domain=traci.constants.CMD_GET_VEHICLE_VARIABLE,
                dist=50.0,
                varIDs=self._vars_to_context_subscribe,
            )

        return current_vins

    def _get_leader(self, vin: str) -> tuple[str, float]:
        """Get current leader with a simulation-step-local cache.

        The cache is invalidated whenever simulation time changes so returned
        values stay exact for each simulation step.

        Args:
            vin (str): Vehicle ID.

        Returns:
            tuple[str, float]: (leader_id, gap) or ("", -1.0) when unavailable.
        """

        sim_time = traci.simulation.getTime()
        if self._leader_cache_sim_time != sim_time:
            self._leader_cache_sim_time = sim_time
            self._leader_cache.clear()

        cached = self._leader_cache.get(vin)
        if cached is not None:
            return cached

        leader = traci.vehicle.getLeader(vin, 0.0) or ("", -1.0)
        self._leader_cache[vin] = leader
        return leader

    def _get_wall_clock_time(self) -> float:
        """Convert simulation time to wall-clock time using the saved base time."""
        return self.base_time + traci.simulation.getTime()

    def _init_log_renderer(self, map_data: BaseMap) -> None:
        """Create the log renderer with map data after SUMO is connected.

        Called at the start of :meth:`run` once map data has been collected.
        The renderer is initialised with a dedicated GPU device and
        ``SharedMapBuffers`` so that road geometry is visible in saved images.

        Args:
            map_data: Collected road-network geometry.
        """

        if not self.renderer_name or not self.image_dir:
            return

        try:
            import wgpu

            from ..world.wgpu_renderer import create_shared_map_buffers

            adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
            self._log_renderer_device = adapter.request_device_sync()
            shared_map_buffers = create_shared_map_buffers(self._log_renderer_device, map_data)
            self.log_renderer = new_renderer(
                self.renderer_name,
                device=self._log_renderer_device,
                shared_map_buffers=shared_map_buffers,
            )
            _logger.info("Log renderer initialised with map data.")
        except KeyError:
            _logger.warning(f"Renderer '{self.renderer_name}' not found. Log-mode image capture disabled.")
        except Exception as exc:
            _logger.warning(f"Failed to initialise log renderer: {exc}")

    def _pos2lonlat(self, pos: tuple[float, float]) -> dict[str, float]:
        """Convert (x, y) SUMO coordinates to {x, y, lon, lat}.

        Uses a local ``pyproj.Proj`` object when available (set up by
        :meth:`_setup_geo_proj`) to avoid a TraCI round-trip per vehicle.
        Falls back to ``traci.simulation.convertGeo`` otherwise.

        Args:
            pos: (x, y) in SUMO network coordinates.

        Returns:
            dict[str, float]: Keys ``x``, ``y``, ``lon``, ``lat``.
        """
        x, y = pos
        if self._geo_proj is not None:
            x_off, y_off = self._geo_proj_offset
            lon, lat = self._geo_proj(x - x_off, y - y_off, inverse=True)
        else:
            lon, lat = traci.simulation.convertGeo(x, y)
        return {"x": x, "y": y, "lon": lon, "lat": lat}

    def _setup_geo_proj(self, sumo_map: Any) -> None:
        """Initialise local geo projection from SumoMap.

        Called once at the start of :meth:`run` after the map is collected.
        When successful, :meth:`_pos2lonlat` bypasses TraCI for coordinate
        conversion, saving one round-trip per vehicle per simulation step.

        Args:
            sumo_map: The collected SUMO map (``SumoMap`` instance).
        """
        self._geo_proj = sumo_map.geo_proj
        self._geo_proj_offset = sumo_map.geo_proj_offset
        if self._geo_proj is not None:
            _logger.info("Local geo projection active; TraCI convertGeo bypassed.")
        else:
            _logger.info("No geo projection available; using TraCI convertGeo per vehicle.")

    def _gather_vehicle_states(self) -> dict[str, dict[str, Any]]:
        """Gather the states of all vehicles using TraCI subscriptions."""

        current_vins = self._ensure_subscriptions()
        ts = self._get_wall_clock_time()
        all_results = traci.vehicle.getAllSubscriptionResults() or {}

        states = {}

        for vin in current_vins:
            res = all_results.get(vin)
            try:
                state: dict[str, Any] = {
                    "vin": vin,
                    "ts": ts,
                    "leader": ("", -1.0),  # default; overwritten from subscription below
                    "status": "running",
                }
                # Try per-vehicle subscription result once before entering the
                # expensive multi-call fallback path.
                if res is None:
                    res = traci.vehicle.getSubscriptionResults(vin)
                if res is not None:
                    for key, var in self._resolved_vars.items():
                        value = res.get(var)
                        if value is None:
                            continue
                        if key == "pos":
                            state.update(self._pos2lonlat(value))
                        elif key == "next_tls":
                            # Convert the list of raw TraCI tuples
                            # (tlsID, tlsIndex, dist, state) into dicts.
                            state["next_tls"] = (
                                [
                                    {
                                        "id": t[0],
                                        "idx": t[1],
                                        "dist": t[2],
                                        "signal": t[3],
                                    }
                                    for t in itertools.islice(value, 5)
                                ]
                                if value
                                else []
                            )
                        else:
                            state[key] = value
                    # Leader from parameterised subscription (subscribeLeader).
                    leader_val = res.get(traci.constants.VAR_LEADER)
                    state["leader"] = leader_val if leader_val is not None else self._get_leader(vin)
                else:
                    # Fallback for vehicles whose subscription data is not yet
                    # available (e.g. just-departed vehicles on their first step).
                    pos = traci.vehicle.getPosition(vin)
                    state.update(self._pos2lonlat(pos))
                    state["angle"] = traci.vehicle.getAngle(vin)
                    state["speed"] = traci.vehicle.getSpeed(vin)
                    state["road_id"] = traci.vehicle.getRoadID(vin)
                    state["lane_id"] = traci.vehicle.getLaneID(vin)
                    state["lane_idx"] = traci.vehicle.getLaneIndex(vin)
                    state["leader"] = self._get_leader(vin)

                # Number of lanes on the current road (cached, static per edge).
                road_id = state.get("road_id", "")
                if road_id and not road_id.startswith(":"):
                    num_lanes = self._edge_lane_count.get(road_id)
                    if num_lanes is None:
                        try:
                            num_lanes = traci.edge.getLaneNumber(road_id)
                        except traci.TraCIException:
                            num_lanes = 0
                        self._edge_lane_count[road_id] = num_lanes
                    state["num_lanes"] = num_lanes

                # next TLSs — populated from subscription result above;
                # fall back to a direct TraCI call only for vehicles whose
                # subscription data was not available this step (e.g. first
                # frame after departure).
                if "next_tls" not in state:
                    state["next_tls"] = [
                        {
                            "id": t[0],
                            "idx": t[1],
                            "dist": t[2],
                            "signal": t[3],
                        }
                        for t in itertools.islice(traci.vehicle.getNextTLS(vin), 5)
                    ]

                # neighbor vehicles
                state["neighbor_vehicles"] = []
                context = traci.vehicle.getContextSubscriptionResults(vin) if res is not None else None
                if context:
                    for neighbor_vin, result in itertools.islice(context.items(), 10):
                        if neighbor_vin == vin:  # ignore ego car
                            continue
                        neighbor_vehicle = {"vin": neighbor_vin}
                        for key, var in self._resolved_context_vars.items():
                            value = result.get(var)
                            if value is None:
                                continue
                            if key == "pos":
                                neighbor_vehicle.update(self._pos2lonlat(value))
                            else:
                                neighbor_vehicle[key] = value
                        state["neighbor_vehicles"].append(neighbor_vehicle)

                states[vin] = state
            except traci.TraCIException as e:
                _logger.warning(f"Failed to get state for vehicle {vin}: {e}")
                continue

        for vin in traci.simulation.getDepartedIDList():
            if vin in states:
                states[vin]["status"] = "departed"

        return states

    def _apply_actions_to_sumo(self, actions: dict[str, dict[str, Any]]) -> None:
        """Apply action-patch dicts to SUMO vehicles via TraCI.

        Supported keys in each action dict:

        - ``target_speed`` (float): ``traci.vehicle.setSpeed``
        - ``target_accel`` (float): ``traci.vehicle.setAccel``
        - ``target_decel`` (float): ``traci.vehicle.setDecel``
        - ``max_speed`` (float): ``traci.vehicle.setMaxSpeed``
        - ``speed_factor`` (float): ``traci.vehicle.setSpeedFactor``
        - ``speed_mode`` (int): ``traci.vehicle.setSpeedMode`` bitmask
        - ``lane_change`` (int | tuple[int,int]): ``changeLane(idx, dur)``
        - ``lane_change_mode`` (int): ``traci.vehicle.setLaneChangeMode``
        - ``sublane_lat`` (float): ``traci.vehicle.changeSublane``
        - ``move_to_xy`` (dict): keys ``x``, ``y``, optional ``edge``,
          ``lane``, ``angle``, ``keep_route``
        - ``reroute`` (list[str]): ``traci.vehicle.setRoute``
        - ``restore`` (bool): Reset speed_mode / lane_change_mode to safe defaults
        """
        vehicle_ids = set(traci.vehicle.getIDList())
        for vin, act in actions.items():
            if vin not in vehicle_ids:
                continue
            try:
                self._apply_single_action(vin, act)
            except traci.TraCIException as exc:
                _logger.warning("TraCI action failed for %s: %s", vin, exc)

    def _apply_single_action(self, vin: str, act: dict[str, Any]) -> None:
        """Dispatch a single vehicle action dict to TraCI calls."""
        # --- restore to safe defaults ---
        if act.get("restore"):
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setSpeedMode(vin, 31)  # all safety on
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setLaneChangeMode(vin, 1621)  # default
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setSpeed(vin, -1)  # return to default
            return

        # --- speed_mode / lane_change_mode (set early, before speed/lane) ---
        speed_mode = act.get("speed_mode")
        if speed_mode is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setSpeedMode(vin, int(speed_mode))

        lane_change_mode = act.get("lane_change_mode")
        if lane_change_mode is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setLaneChangeMode(vin, int(lane_change_mode))

        # --- speed / accel / decel ---
        target_speed = act.get("target_speed")
        if target_speed is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setSpeed(vin, float(target_speed))

        target_accel = act.get("target_accel")
        if target_accel is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setAccel(vin, float(target_accel))

        target_decel = act.get("target_decel")
        if target_decel is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setDecel(vin, float(target_decel))

        max_speed = act.get("max_speed")
        if max_speed is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setMaxSpeed(vin, float(max_speed))

        speed_factor = act.get("speed_factor")
        if speed_factor is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setSpeedFactor(vin, float(speed_factor))

        # --- lane changes ---
        target_lane = act.get("lane_change")
        if target_lane is not None:
            if isinstance(target_lane, (list, tuple)):
                lane_idx, dur = int(target_lane[0]), int(target_lane[1])
            else:
                lane_idx, dur = int(target_lane), 50
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.changeLane(vin, lane_idx, duration=dur)

        sublane_lat = act.get("sublane_lat")
        if sublane_lat is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.changeSublane(vin, float(sublane_lat))

        # --- moveToXY (teleport) ---
        move_xy = act.get("move_to_xy")
        if move_xy is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.moveToXY(
                    vin,
                    move_xy.get("edge", ""),
                    move_xy.get("lane", -1),
                    float(move_xy["x"]),
                    float(move_xy["y"]),
                    angle=float(move_xy.get("angle", -1)),
                    keepRoute=int(move_xy.get("keep_route", 2)),
                )

        # --- reroute ---
        reroute = act.get("reroute")
        if reroute is not None:
            with contextlib.suppress(traci.TraCIException):
                traci.vehicle.setRoute(vin, list(reroute))

    def _on_vehicle_reached_destination(self, vin: str) -> None:
        pos = traci.vehicle.getLanePosition(vin)
        lane_id = traci.vehicle.getLaneID(vin)
        lane_len = traci.lane.getLength(lane_id)
        # Check if the vehicle is near the end of the lane
        if lane_len - pos < traci.vehicle.getLength(vin) + 5.0:
            # Reassign route when near the end
            traci.vehicle.moveTo(vin, "E0_0", 0.0)
            traci.vehicle.setRouteID(vin, "tri")
            traci.vehicle.setSpeed(vin, -1)

    async def run(self, world: World | None = None) -> None:
        # Collect map data now that SUMO is connected (TraCI is live).
        # This must happen before the simulation loop so that:
        # - world.map_data is set before any Place is created
        # - the log renderer has road geometry from the start
        from .sumo_map import SumoMap

        sumo_map = SumoMap.collect()
        self._setup_geo_proj(sumo_map)
        if world:
            world.set_map_data(sumo_map)
        self._init_log_renderer(sumo_map)

        # Simulation loop
        _step_count = 0
        _prev_real_time = time.time()
        _prev_sim_time = traci.simulation.getTime()
        _timing_window = {
            "step": 0.0,
            "gather": 0.0,
            "world": 0.0,
            "apply": 0.0,
            "notify": 0.0,
            "yield": 0.0,
            "frames": 0,
        }

        while traci.simulation.getMinExpectedNumber() > 0:
            # Advance the simulation by one step
            _t0 = time.perf_counter()
            traci.simulationStep()
            _timing_window["step"] += time.perf_counter() - _t0
            _logger.info("Simulation time: %s", traci.simulation.getTime())

            _t0 = time.perf_counter()
            states = self._gather_vehicle_states()
            _timing_window["gather"] += time.perf_counter() - _t0

            if self.log_out:
                try:
                    if self.log_format == _LogFormat.JSONL:
                        self.log_out.write(json.dumps(states) + "\n")
                        self.log_out.flush()
                    elif self.log_format == _LogFormat.CSV:
                        # Simple CSV logging: header + one line per vehicle
                        if self.log_out.tell() == 0:
                            # Write header
                            self.log_out.write(",".join(_CSV_HEADERS) + "\n")
                        for _vin, state in states.items():
                            row = [str(state.get(h, "")) for h in _CSV_HEADERS]
                            self.log_out.write(",".join(row) + "\n")
                        self.log_out.flush()
                    else:
                        raise ValueError(f"Unsupported log format: {self.log_format}")
                except Exception as e:
                    _logger.warning(f"Failed to write JSON log: {e}")
                try:
                    if self.image_dir and self.log_renderer:
                        for _vin, state in states.items():
                            images = self.log_renderer.render(
                                float(state.get("x", 0.0)),
                                float(state.get("y", 0.0)),
                                float(state.get("angle", 0.0)),
                                state.get("neighbor_vehicles", []),
                                traffic_lights=state.get("next_tls"),
                            )
                            for camera_id, image in images:
                                image_path = os.path.join(
                                    self.image_dir,
                                    f"{_vin}",
                                    f"camera_{camera_id}",
                                    f"{traci.simulation.getTime():.2f}.jpg",
                                )
                                os.makedirs(os.path.dirname(image_path), exist_ok=True)
                                with open(image_path, "wb") as image_out:
                                    image_out.write(image)
                except Exception as e:
                    _logger.warning(f"Failed to render image: {e}")

            if world:
                _t0 = time.perf_counter()
                actions = await world.step(states)
                _timing_window["world"] += time.perf_counter() - _t0

                _t0 = time.perf_counter()
                self._apply_actions_to_sumo(actions)
                _timing_window["apply"] += time.perf_counter() - _t0

                arrived = traci.simulation.getArrivedIDList()
                if arrived:
                    _t0 = time.perf_counter()
                    await world.notify_arrived(arrived)
                    _timing_window["notify"] += time.perf_counter() - _t0

            # Yield to the event loop so that background asyncio tasks (e.g.
            # _pipe_stream for edge subprocess stdout/stderr) get a chance to run
            # after the blocking traci.simulationStep() call.
            _t0 = time.perf_counter()
            await asyncio.sleep(0)
            _timing_window["yield"] += time.perf_counter() - _t0
            _timing_window["frames"] += 1

            _step_count += 1
            _now = time.time()
            if _now - _prev_real_time > self.throughput_interval_secs:
                sim_time = traci.simulation.getTime() - _prev_sim_time
                throughput = sim_time / (_now - _prev_real_time)
                cars = len(states)
                ms_per_car = int(throughput / cars * 1000) if cars > 0 else -1
                _logger.info(
                    f"Simulation running for {cars} cars at {throughput:.2f} sim-secs per real-sec (= {ms_per_car} ms/car)."
                )
                frames = max(_timing_window["frames"], 1)
                _logger.info(
                    "Frame timing avg(ms): step=%.2f gather=%.2f world=%.2f apply=%.2f notify=%.2f yield=%.2f",
                    (_timing_window["step"] / frames) * 1000,
                    (_timing_window["gather"] / frames) * 1000,
                    (_timing_window["world"] / frames) * 1000,
                    (_timing_window["apply"] / frames) * 1000,
                    (_timing_window["notify"] / frames) * 1000,
                    (_timing_window["yield"] / frames) * 1000,
                )
                _step_count = 0
                _prev_real_time = _now
                _prev_sim_time = traci.simulation.getTime()
                _timing_window = {
                    "step": 0.0,
                    "gather": 0.0,
                    "world": 0.0,
                    "apply": 0.0,
                    "notify": 0.0,
                    "yield": 0.0,
                    "frames": 0,
                }

        self.stop()
        _logger.info("Simulation ended.")

    def stop(self) -> None:
        if self.log_out:
            self.log_out.close()
        with contextlib.suppress(traci.exceptions.TraCIException, OSError):
            traci.close()
        _logger.info("Router stopped and resources cleaned up.")
