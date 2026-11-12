# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Utilities for serializing and deserializing vehicle peer state using FlatBuffers.

This module converts a structured Python dictionary that represents the state of a
vehicle (the "peer") and its context into a FlatBuffer message, and can reconstruct
the dictionary from a FlatBuffer. It is intended for efficient inter-process or
inter-node messaging within a cluster of vehicles or simulators.

Key features:
- Maps snake_case Python keys to the FlatBuffer schema's CamelCase fields.
- Encodes the peer's core state, the next traffic-light signals (TLS), and neighboring
    vehicles into a compact binary format.
- Supports nested structures (e.g., the "leader" vehicle is serialized as a
    VehicleDistance object containing a VIN and distance).
- Provides symmetric serialization (fb_serialize) and deserialization (fb_deserialize)
    to ensure round-trip fidelity.

Data model:
- STATE_KEYS: the main peer state fields (e.g., vin, ts, position, speed, emissions,
    lane and road identifiers, etc.). Special handling:
    - leader: a tuple (vin: str, dist: float) encoded as a VehicleDistance FlatBuffer.
- CONTEXT_KEYS: the fields recorded for each neighboring vehicle (e.g., vin, position,
    speed, acceleration, lane and road identifiers).
- next_tls: a list of TLS entries with keys {id: str, idx: int, dist: float, signal: str}.
- neighbor_vehicles: a list of vehicle context entries with keys in CONTEXT_KEYS.

Functions:
- snake2camel(s: str) -> str
    Converts snake_case to CamelCase to align Python dict keys with FlatBuffer field names.
- fb_serialize(state: Dict[str, Any]) -> bytes
    Builds a FlatBuffer message from the provided state dict. Expects:
    - All STATE_KEYS present in state.
    - state["leader"] as (vin: str, dist: float).
    - state["next_tls"] as a list of dicts with {id, idx, dist, signal}.
    - state["neighbor_vehicles"] as a list of dicts with CONTEXT_KEYS.
    Returns a bytes object containing the FlatBuffer message.
- fb_deserialize(data: bytes) -> Dict[str, Any]
    Parses a FlatBuffer message into a state dict matching the structure expected by
    fb_serialize, including reconstruction of nested objects and lists.

Notes:
- The FlatBuffer schema types are provided by fb.peer.* modules: Peer, VehicleDistance,
    NextTLS, and NeighborVehicle.
- Logging is emitted at debug level for field assignment and message size.
- This module assumes that input dict values have correct types that match the schema.

Example (conceptual):
- To send a peer state:
    - Populate a dict with STATE_KEYS, add "next_tls" and "neighbor_vehicles" lists.
    - Call fb_serialize(state) and transmit the resulting bytes.
- To receive and use a peer state:
    - Call fb_deserialize(bytes) to obtain the original dict structure.
"""

import logging
from typing import Any

import flatbuffers

from ..gen import peer_generated as fbp

_logger = logging.getLogger(__name__)


_STATE_KEYS = [
    "vin",
    "ts",
    "leader",
    "x",
    "y",
    "lon",
    "lat",
    "angle",
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

_CONTEXT_KEYS = [
    "vin",
    "x",
    "y",
    "lon",
    "lat",
    "speed",
    "accel",
    "road_id",
    "lane_id",
    "lane_idx",
    "angle",
]


def _snake2camel(snake_str: str) -> str:
    components = snake_str.split("_")
    return "".join(x.title() for x in components)


def fb_serialize(state: dict[str, Any]) -> bytes:
    """Gather the states of all vehicles using TraCI subscriptions."""

    def _prepend_vector(start_fn, fbb, entries):
        """Helper to prepend a vector of entries to the FlatBuffer builder."""
        start_fn(fbb, len(entries))
        for entry in reversed(entries):
            fbb.PrependUOffsetTRelative(entry)
        return fbb.EndVector()

    fbb = flatbuffers.Builder(0)

    # next TLSs
    next_tlss = []
    for next_tls in state["next_tls"]:
        next_tls_id = fbb.CreateString(next_tls["id"])
        next_tls_signal = fbb.CreateString(next_tls["signal"])
        fbp.NextTLSStart(fbb)
        fbp.NextTLSAddId(fbb, next_tls_id)
        fbp.NextTLSAddIdx(fbb, next_tls["idx"])
        fbp.NextTLSAddDist(fbb, next_tls["dist"])
        fbp.NextTLSAddSignal(fbb, next_tls_signal)
        next_tlss.append(fbp.NextTLSEnd(fbb))

    next_tlss = _prepend_vector(fbp.PeerStartNextTlsVector, fbb, next_tlss)

    # neighbor vehicles
    neighbor_vehicles = []
    for neighbor_vehicle in state["neighbor_vehicles"]:
        values = []
        for key in _CONTEXT_KEYS:
            setter = getattr(fbp, f"NeighborVehicleAdd{_snake2camel(key)}")
            value = neighbor_vehicle[key]
            if isinstance(value, str):
                values.append(fbb.CreateString(value))
            else:
                values.append(value)

        fbp.NeighborVehicleStart(fbb)
        for key, value in zip(_CONTEXT_KEYS, values, strict=False):
            setter = getattr(fbp, f"NeighborVehicleAdd{_snake2camel(key)}")
            setter(fbb, value)
        neighbor_vehicles.append(fbp.NeighborVehicleEnd(fbb))
    neighbor_vehicles = _prepend_vector(fbp.PeerStartNeighborVehiclesVector, fbb, neighbor_vehicles)

    # camera images (optional) — must create image byte vectors before PeerStart
    camera_images = state.get("camera_images")
    cam_vec = None
    if camera_images:
        cam_offsets = []
        for camera_id, image_bytes in camera_images:
            img_off = fbb.CreateByteVector(image_bytes)
            fbp.CameraImageStart(fbb)
            fbp.CameraImageAddCameraId(fbb, camera_id)
            fbp.CameraImageAddImage(fbb, img_off)
            cam_offsets.append(fbp.CameraImageEnd(fbb))
        cam_vec = _prepend_vector(fbp.PeerStartCameraImagesVector, fbb, cam_offsets)

    # peer values
    values = []
    for key in _STATE_KEYS:
        value = state[key]
        if key == "leader":
            vin, dist = value
            vin_leader = fbb.CreateString(vin)
            fbp.VehicleDistanceStart(fbb)
            fbp.VehicleDistanceAddVin(fbb, vin_leader)
            fbp.VehicleDistanceAddDist(fbb, dist)
            values.append(fbp.VehicleDistanceEnd(fbb))
        elif isinstance(value, str):
            values.append(fbb.CreateString(value))
        else:
            values.append(value)

    # active_behavior (optional) — must create string before PeerStart
    active_behavior = state.get("active_behavior")
    ab_off = None
    if active_behavior:
        ab_off = fbb.CreateString(active_behavior)

    # peer
    fbp.PeerStart(fbb)
    for key, value in zip(_STATE_KEYS, values, strict=False):
        setter = getattr(fbp, f"PeerAdd{_snake2camel(key)}")
        setter(fbb, value)
    fbp.PeerAddNextTls(fbb, next_tlss)
    fbp.PeerAddNeighborVehicles(fbb, neighbor_vehicles)
    if cam_vec is not None:
        fbp.PeerAddCameraImages(fbb, cam_vec)
    if ab_off is not None:
        fbp.PeerAddActiveBehavior(fbb, ab_off)

    fbb.Finish(fbp.PeerEnd(fbb))
    message = fbb.Output()

    if _logger.isEnabledFor(logging.DEBUG):
        _logger.debug(f"Gathered states for {len(state)} vehicles, serializing into {len(message)} bytes.")

    return message


def fb_deserialize(data: bytes) -> dict[str, Any]:
    """Deserialize FlatBuffer data into a dictionary representing the peer state."""
    peer = fbp.Peer.GetRootAsPeer(data, 0)
    state: dict[str, Any] = {}

    for key in _STATE_KEYS:
        getter = getattr(peer, _snake2camel(key))
        value = getter()
        if key == "leader":
            vin = value.Vin()
            dist = value.Dist()
            state[key] = (vin, dist)
        else:
            state[key] = value

    # next TLSs
    next_tls_list = []
    for i in range(peer.NextTlsLength()):
        next_tls_fb = peer.NextTls(i)
        next_tls = {
            "id": next_tls_fb.Id(),
            "idx": next_tls_fb.Idx(),
            "dist": next_tls_fb.Dist(),
            "signal": next_tls_fb.Signal(),
        }
        next_tls_list.append(next_tls)
    state["next_tls"] = next_tls_list

    # neighbor vehicles
    neighbor_vehicles_list = []
    for i in range(peer.NeighborVehiclesLength()):
        neighbor_vehicle_fb = peer.NeighborVehicles(i)
        neighbor_vehicle = {}
        for key in _CONTEXT_KEYS:
            getter = getattr(neighbor_vehicle_fb, _snake2camel(key))
            neighbor_vehicle[key] = getter()
        neighbor_vehicles_list.append(neighbor_vehicle)
    state["neighbor_vehicles"] = neighbor_vehicles_list

    # camera images
    camera_images_list = []
    for i in range(peer.CameraImagesLength()):
        cam = peer.CameraImages(i)
        camera_images_list.append((cam.CameraId(), bytes(cam.ImageAsNumpy())))
    state["camera_images"] = camera_images_list

    # active_behavior (optional string, may be None)
    ab = peer.ActiveBehavior()
    state["active_behavior"] = ab.decode() if isinstance(ab, bytes) else ab

    return state
