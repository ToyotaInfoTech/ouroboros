# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
GPU-accelerated surround-view renderer using wgpu-py.

Renders 6 virtual dash-cam images per ego viewpoint by:
1. Building a cubemap (6 faces) with rectilinear projection for each
   virtual camera.
2. Remapping each cubemap to an equidistant fisheye output image.
3. JPEG-encoding the result on CPU.

Designed to be instantiated once per Place and shared across all Edges.
Road-network geometry is uploaded to the GPU once via ``SharedMapBuffers``
and reused across frames.
"""

from __future__ import annotations

import logging
import math
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import cv2
import numpy as np
import wgpu

from .map import BaseMap
from .renderer import (
    CAM_HEIGHT,
    CAMERAS,
    DEFAULT_HEIGHT,
    DEFAULT_LENGTH,
    DEFAULT_WIDTH,
    FACE_COLOURS,
    GROUND_COLOUR,
    LANE_EDGE_COLOUR,
    ROAD_COLOUR,
    SKY_COLOUR,
    TLS_DEFAULT_COLOUR,
    TLS_HEAD_SIZE,
    TLS_HEAD_Z,
    TLS_POLE_COLOUR,
    TLS_POLE_HEIGHT,
    TLS_POLE_WIDTH,
    TLS_SIGNAL_COLOURS,
    Renderer,
)

_logger = logging.getLogger(__name__)


@dataclass
class SharedMapBuffers:
    """Shared GPU buffers for road-network geometry.

    These buffers are created once per Place and shared across all Edge renderers
    to avoid redundant GPU uploads of static map data.
    """

    road_vbuf: wgpu.GPUBuffer | None
    road_vert_count: int
    junction_vbuf: wgpu.GPUBuffer | None
    junction_vert_count: int
    tls_pole_vbuf: wgpu.GPUBuffer | None
    tls_pole_vert_count: int
    tls_list: list[dict[str, Any]]


_SHADER_DIR = Path(__file__).parent / "shaders"

# Cubemap face size in pixels.
_CUBE_FACE = 256

# Cubemap face view matrices (look direction, up vector) in world space
# (before yaw rotation).  World: +X = forward, -Y = right, +Z = up.
# Shader samples cubemap with direction in (X=right, Y=up, Z=forward) space,
# so we map: shader +X → world -Y, shader +Y → world +Z, shader +Z → world +X.
# wgpu cubemap faces are: +X, -X, +Y, -Y, +Z, -Z.
_CUBE_FACE_DIRS: list[tuple[tuple[float, float, float], tuple[float, float, float]]] = [
    ((0, -1, 0), (0, 0, 1)),  # +X = right
    ((0, 1, 0), (0, 0, 1)),  # -X = left
    ((0, 0, 1), (-1, 0, 0)),  # +Y = up
    ((0, 0, -1), (1, 0, 0)),  # -Y = down
    ((1, 0, 0), (0, 0, 1)),  # +Z = forward
    ((-1, 0, 0), (0, 0, 1)),  # -Z = backward
]


def _look_at(eye: np.ndarray, target: np.ndarray, up: np.ndarray) -> np.ndarray:
    """Build a 4x4 view matrix (world → camera)."""
    f = target - eye
    f = f / np.linalg.norm(f)
    s = np.cross(f, up)
    s = s / np.linalg.norm(s)
    u = np.cross(s, f)
    m = np.eye(4, dtype=np.float32)
    m[0, :3] = s
    m[1, :3] = u
    m[2, :3] = -f
    m[0, 3] = -s.dot(eye)
    m[1, 3] = -u.dot(eye)
    m[2, 3] = f.dot(eye)
    return m


def _perspective(fov_rad: float, aspect: float, near: float, far: float) -> np.ndarray:
    """Build a 4x4 perspective projection matrix."""
    f = 1.0 / math.tan(fov_rad / 2.0)
    m = np.zeros((4, 4), dtype=np.float32)
    m[0, 0] = f / aspect
    m[1, 1] = f
    m[2, 2] = far / (near - far)
    m[2, 3] = (near * far) / (near - far)
    m[3, 2] = -1.0
    return m


def _sumo_angle_to_yaw_rad(angle_deg: float) -> float:
    """Convert SUMO heading (0=north, CW) to standard yaw (0=east, CCW) in radians."""
    return math.radians(90.0 - angle_deg)


def _rot_z(yaw: float) -> np.ndarray:
    """3x3 rotation matrix around Z axis."""
    c, s = math.cos(yaw), math.sin(yaw)
    return np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]], dtype=np.float32)


def _build_box_vertices(
    cx: float,
    cy: float,
    yaw: float,
    length: float = DEFAULT_LENGTH,
    width: float = DEFAULT_WIDTH,
    height: float = DEFAULT_HEIGHT,
) -> np.ndarray:
    """Build 36 triangle vertices (position + colour) for a vehicle box.

    Returns an (N, 6) float32 array where each row is [x, y, z, r, g, b].
    """
    hl, hw = length / 2.0, width / 2.0
    # 8 corner positions (bottom z=0, top z=height).
    corners = np.array(
        [
            [hl, hw, 0],
            [hl, -hw, 0],
            [-hl, -hw, 0],
            [-hl, hw, 0],
            [hl, hw, height],
            [hl, -hw, height],
            [-hl, -hw, height],
            [-hl, hw, height],
        ],
        dtype=np.float32,
    )
    R = _rot_z(yaw)
    corners = corners @ R.T + np.array([cx, cy, 0], dtype=np.float32)

    # 6 faces as 2 triangles each (same winding as old _FACES).
    face_indices = [
        (0, 1, 5, 0, 5, 4),  # front
        (2, 3, 7, 2, 7, 6),  # rear
        (3, 0, 4, 3, 4, 7),  # left
        (1, 2, 6, 1, 6, 5),  # right
        (4, 5, 6, 4, 6, 7),  # top
        (0, 3, 2, 0, 2, 1),  # bottom
    ]
    verts = np.empty((36, 6), dtype=np.float32)
    idx = 0
    for fi, indices in enumerate(face_indices):
        r, g, b = FACE_COLOURS[fi]
        colour = np.array([r / 255.0, g / 255.0, b / 255.0], dtype=np.float32)
        for vi in indices:
            verts[idx, :3] = corners[vi]
            verts[idx, 3:] = colour
            idx += 1
    return verts


def _build_road_mesh(lanes: list[dict[str, Any]]) -> np.ndarray:
    """Convert lane geometries into triangle vertices for GPU rendering.

    Each lane centerline is expanded into a quad strip (2 triangles per segment).
    Returns an (N, 6) float32 array [x, y, z, r, g, b].
    """
    r, g, b = ROAD_COLOUR[0] / 255.0, ROAD_COLOUR[1] / 255.0, ROAD_COLOUR[2] / 255.0
    colour = np.array([r, g, b], dtype=np.float32)

    all_verts: list[np.ndarray] = []
    for lane in lanes:
        shape = lane.get("shape", [])
        width = lane.get("width", 3.2)
        if len(shape) < 2:
            continue

        pts = np.array(shape, dtype=np.float32)
        half_w = width / 2.0
        n = len(pts)

        left = np.zeros((n, 3), dtype=np.float32)
        right = np.zeros((n, 3), dtype=np.float32)

        for i in range(n):
            if i == 0:
                dx, dy = pts[1, 0] - pts[0, 0], pts[1, 1] - pts[0, 1]
            elif i == n - 1:
                dx, dy = pts[-1, 0] - pts[-2, 0], pts[-1, 1] - pts[-2, 1]
            else:
                dx = pts[i + 1, 0] - pts[i - 1, 0]
                dy = pts[i + 1, 1] - pts[i - 1, 1]
            length = math.sqrt(dx * dx + dy * dy)
            if length < 1e-6:
                nx, ny = 0.0, 1.0
            else:
                nx, ny = -dy / length, dx / length
            left[i] = [pts[i, 0] + nx * half_w, pts[i, 1] + ny * half_w, 0.0]
            right[i] = [pts[i, 0] - nx * half_w, pts[i, 1] - ny * half_w, 0.0]

        # Pre-allocate vertices for this lane (6 verts per segment).
        lane_verts = np.empty(((n - 1) * 6, 6), dtype=np.float32)
        idx = 0
        for i in range(n - 1):
            # Two triangles per segment.
            for v in [
                left[i],
                right[i],
                right[i + 1],
                left[i],
                right[i + 1],
                left[i + 1],
            ]:
                lane_verts[idx, :3] = v
                lane_verts[idx, 3:] = colour
                idx += 1
        all_verts.append(lane_verts)

    # Lane edge markings: thin white strips along both edges of each lane.
    er = LANE_EDGE_COLOUR[0] / 255.0
    eg = LANE_EDGE_COLOUR[1] / 255.0
    eb = LANE_EDGE_COLOUR[2] / 255.0
    edge_colour = np.array([er, eg, eb], dtype=np.float32)
    marking_half_w = 0.06  # 12 cm wide white line
    marking_z = 0.005  # slightly above road surface

    for lane in lanes:
        shape = lane.get("shape", [])
        width = lane.get("width", 3.2)
        if len(shape) < 2:
            continue

        pts = np.array(shape, dtype=np.float32)
        half_w = width / 2.0
        n = len(pts)

        # Compute normal at each vertex.
        normals = np.zeros((n, 2), dtype=np.float32)
        for i in range(n):
            if i == 0:
                dx, dy = pts[1, 0] - pts[0, 0], pts[1, 1] - pts[0, 1]
            elif i == n - 1:
                dx, dy = pts[-1, 0] - pts[-2, 0], pts[-1, 1] - pts[-2, 1]
            else:
                dx = pts[i + 1, 0] - pts[i - 1, 0]
                dy = pts[i + 1, 1] - pts[i - 1, 1]
            ln = math.sqrt(dx * dx + dy * dy)
            if ln < 1e-6:
                normals[i] = [0.0, 1.0]
            else:
                normals[i] = [-dy / ln, dx / ln]

        # Draw a thin strip along left edge and right edge.
        for side_sign in (1.0, -1.0):
            inner = np.zeros((n, 3), dtype=np.float32)
            outer = np.zeros((n, 3), dtype=np.float32)
            for i in range(n):
                nx, ny = normals[i]
                offset = side_sign * half_w
                inner[i] = [
                    pts[i, 0] + nx * (offset - marking_half_w),
                    pts[i, 1] + ny * (offset - marking_half_w),
                    marking_z,
                ]
                outer[i] = [
                    pts[i, 0] + nx * (offset + marking_half_w),
                    pts[i, 1] + ny * (offset + marking_half_w),
                    marking_z,
                ]

            # Pre-allocate vertices for this edge strip (6 verts per segment).
            edge_verts = np.empty(((n - 1) * 6, 6), dtype=np.float32)
            idx = 0
            for i in range(n - 1):
                for v in [
                    inner[i],
                    outer[i],
                    outer[i + 1],
                    inner[i],
                    outer[i + 1],
                    inner[i + 1],
                ]:
                    edge_verts[idx, :3] = v
                    edge_verts[idx, 3:] = edge_colour
                    idx += 1
            all_verts.append(edge_verts)

    if not all_verts:
        return np.zeros((0, 6), dtype=np.float32)
    return np.concatenate(all_verts, axis=0)


def _build_junction_mesh(junctions: list[dict[str, Any]]) -> np.ndarray:
    """Convert junction boundary polygons into triangle vertices for GPU rendering.

    Each junction shape is triangulated using fan triangulation from the
    polygon centroid.  This fills the intersection/fork area between lanes.

    Args:
        junctions: List of dicts with "shape" (list of (x, y) tuples)
            defining the junction boundary polygon (>= 3 vertices).

    Returns:
        (N, 6) float32 array [x, y, z, r, g, b].
    """
    r, g, b = ROAD_COLOUR[0] / 255.0, ROAD_COLOUR[1] / 255.0, ROAD_COLOUR[2] / 255.0
    colour = np.array([r, g, b], dtype=np.float32)

    all_verts: list[np.ndarray] = []
    for junc in junctions:
        shape = junc.get("shape", [])
        if len(shape) < 3:
            continue

        pts = np.array(shape, dtype=np.float32)
        # Compute centroid for fan triangulation.
        cx = pts[:, 0].mean()
        cy = pts[:, 1].mean()
        center = np.array([cx, cy, 0.0], dtype=np.float32)

        n = len(pts)
        # Pre-allocate vertices for this junction (3 verts per triangle).
        junc_verts = np.empty((n * 3, 6), dtype=np.float32)
        idx = 0
        for i in range(n):
            j = (i + 1) % n
            v0 = np.array([pts[i, 0], pts[i, 1], 0.0], dtype=np.float32)
            v1 = np.array([pts[j, 0], pts[j, 1], 0.0], dtype=np.float32)
            junc_verts[idx, :3] = center
            junc_verts[idx, 3:] = colour
            idx += 1
            junc_verts[idx, :3] = v0
            junc_verts[idx, 3:] = colour
            idx += 1
            junc_verts[idx, :3] = v1
            junc_verts[idx, 3:] = colour
            idx += 1
        all_verts.append(junc_verts)

    if not all_verts:
        return np.zeros((0, 6), dtype=np.float32)
    return np.concatenate(all_verts, axis=0)


def _build_ground_plane(
    cx: float = 0.0,
    cy: float = 0.0,
    half_extent: float = 500.0,
) -> np.ndarray:
    """Build a large ground-plane quad at z = -0.01 (below road z = 0).

    Args:
        cx: Centre X coordinate of the ground quad.
        cy: Centre Y coordinate of the ground quad.
        half_extent: Half-size of the ground quad in metres.

    Returns:
        (6, 6) float32 array of two triangles [x, y, z, r, g, b].
    """
    r = GROUND_COLOUR[0] / 255.0
    g = GROUND_COLOUR[1] / 255.0
    b = GROUND_COLOUR[2] / 255.0
    z = -0.01
    h = half_extent
    verts = np.array(
        [
            [cx - h, cy - h, z, r, g, b],
            [cx + h, cy - h, z, r, g, b],
            [cx + h, cy + h, z, r, g, b],
            [cx - h, cy - h, z, r, g, b],
            [cx + h, cy + h, z, r, g, b],
            [cx - h, cy + h, z, r, g, b],
        ],
        dtype=np.float32,
    )
    return verts


def _build_tls_pole_mesh(tls_list: list[dict[str, Any]]) -> np.ndarray:
    """Build static pole geometry for all traffic lights.

    Each pole is a thin vertical box centred at the TLS junction position.

    Args:
        tls_list: List of TLS dicts with "x" and "y" keys.

    Returns:
        (N, 6) float32 array [x, y, z, r, g, b].
    """
    if not tls_list:
        return np.zeros((0, 6), dtype=np.float32)

    hw = TLS_POLE_WIDTH / 2.0
    h = TLS_POLE_HEIGHT
    r, g, b = (
        TLS_POLE_COLOUR[0] / 255.0,
        TLS_POLE_COLOUR[1] / 255.0,
        TLS_POLE_COLOUR[2] / 255.0,
    )
    colour = np.array([r, g, b], dtype=np.float32)

    all_verts: list[np.ndarray] = []
    for tls in tls_list:
        cx = float(tls.get("x", 0.0))
        cy = float(tls.get("y", 0.0))

        # 8 corners of a thin box.
        corners = np.array(
            [
                [cx - hw, cy - hw, 0],
                [cx + hw, cy - hw, 0],
                [cx + hw, cy + hw, 0],
                [cx - hw, cy + hw, 0],
                [cx - hw, cy - hw, h],
                [cx + hw, cy - hw, h],
                [cx + hw, cy + hw, h],
                [cx - hw, cy + hw, h],
            ],
            dtype=np.float32,
        )

        # 6 faces × 2 triangles = 36 vertices per TLS pole.
        face_indices = [
            (0, 1, 5, 0, 5, 4),
            (2, 3, 7, 2, 7, 6),
            (3, 0, 4, 3, 4, 7),
            (1, 2, 6, 1, 6, 5),
            (4, 5, 6, 4, 6, 7),
            (0, 3, 2, 0, 2, 1),
        ]
        pole_verts = np.empty((36, 6), dtype=np.float32)
        idx = 0
        for indices in face_indices:
            for vi in indices:
                pole_verts[idx, :3] = corners[vi]
                pole_verts[idx, 3:] = colour
                idx += 1
        all_verts.append(pole_verts)

    return np.concatenate(all_verts, axis=0)


def _most_permissive_signal(signal: str) -> str:
    """Pick the most permissive character from a SUMO signal string.

    Priority: g/G > y/Y > r/R > others.

    Args:
        signal: SUMO signal state string (e.g. "gGrrr").

    Returns:
        Single representative character.
    """
    if not signal:
        return "o"
    priority = {"g": 4, "G": 4, "y": 3, "Y": 3, "r": 2, "R": 2}
    best_char = signal[0]
    best_pri = priority.get(best_char, 0)
    for ch in signal[1:]:
        p = priority.get(ch, 0)
        if p > best_pri:
            best_pri = p
            best_char = ch
    return best_char


def _build_tls_signal_mesh(
    tls_list: list[dict[str, Any]],
    active_signals: dict[str, str] | None = None,
) -> np.ndarray:
    """Build dynamic signal-head geometry for traffic lights.

    Each signal head is a small coloured box on top of the pole.  The
    colour is determined by the current signal state if known.

    Args:
        tls_list: List of TLS dicts with "id", "x", "y" keys.
        active_signals: Mapping of TLS id → SUMO signal string.
            Traffic lights not present in this dict are coloured grey.

    Returns:
        (N, 6) float32 array [x, y, z, r, g, b].
    """
    if not tls_list:
        return np.zeros((0, 6), dtype=np.float32)

    hs = TLS_HEAD_SIZE / 2.0
    z_lo = TLS_HEAD_Z
    z_hi = TLS_HEAD_Z + TLS_HEAD_SIZE
    active = active_signals or {}

    all_verts: list[np.ndarray] = []
    for tls in tls_list:
        tls_id = tls.get("id", "")
        cx = float(tls.get("x", 0.0))
        cy = float(tls.get("y", 0.0))

        sig = active.get(tls_id, "")
        rep = _most_permissive_signal(sig) if sig else ""
        rgb = TLS_SIGNAL_COLOURS.get(rep, TLS_DEFAULT_COLOUR)
        colour = np.array([rgb[0] / 255.0, rgb[1] / 255.0, rgb[2] / 255.0], dtype=np.float32)

        corners = np.array(
            [
                [cx - hs, cy - hs, z_lo],
                [cx + hs, cy - hs, z_lo],
                [cx + hs, cy + hs, z_lo],
                [cx - hs, cy + hs, z_lo],
                [cx - hs, cy - hs, z_hi],
                [cx + hs, cy - hs, z_hi],
                [cx + hs, cy + hs, z_hi],
                [cx - hs, cy + hs, z_hi],
            ],
            dtype=np.float32,
        )

        face_indices = [
            (0, 1, 5, 0, 5, 4),
            (2, 3, 7, 2, 7, 6),
            (3, 0, 4, 3, 4, 7),
            (1, 2, 6, 1, 6, 5),
            (4, 5, 6, 4, 6, 7),
            (0, 3, 2, 0, 2, 1),
        ]
        signal_verts = np.empty((36, 6), dtype=np.float32)
        idx = 0
        for indices in face_indices:
            for vi in indices:
                signal_verts[idx, :3] = corners[vi]
                signal_verts[idx, 3:] = colour
                idx += 1
        all_verts.append(signal_verts)

    return np.concatenate(all_verts, axis=0)


class WgpuRenderer(Renderer):
    """wgpu-py (Metal/Vulkan) GPU renderer implementing the Place-level Renderer ABC.

    Args:
        width: Output image width in pixels.
        height: Output image height in pixels.
        jpeg_quality: JPEG compression quality (0-100).
        max_range: Maximum render distance from ego in metres.
        cube_face_size: Cubemap face resolution in pixels.
    """

    def __init__(
        self,
        width: int = 640,
        height: int = 360,
        jpeg_quality: int = 60,
        max_range: float = 80.0,
        cube_face_size: int = _CUBE_FACE,
        device: wgpu.GPUDevice | None = None,
        shared_map_buffers: SharedMapBuffers | None = None,
    ):
        super().__init__(width, height, jpeg_quality, max_range)
        self._cube_size = cube_face_size

        # --- wgpu device ---
        if device is not None:
            # Use shared GPU device (efficient, no redundant initialization)
            self._device: wgpu.GPUDevice = device
        else:
            # Create new GPU device (fallback for standalone use)
            adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
            self._device: wgpu.GPUDevice = adapter.request_device_sync()

        # --- shaders ---
        scene_src = (_SHADER_DIR / "scene.wgsl").read_text()
        remap_src = (_SHADER_DIR / "fisheye_remap.wgsl").read_text()
        self._scene_shader = self._device.create_shader_module(code=scene_src)
        self._remap_shader = self._device.create_shader_module(code=remap_src)

        # --- cubemap face projection (90° FOV, aspect 1:1) ---
        self._cube_proj = _perspective(math.radians(90.0), 1.0, 0.1, 500.0)

        # --- scene render pipeline ---
        self._camera_bind_layout = self._device.create_bind_group_layout(
            entries=[
                {
                    "binding": 0,
                    "visibility": wgpu.ShaderStage.VERTEX,
                    "buffer": {"type": wgpu.BufferBindingType.uniform},
                }
            ],
        )
        self._scene_pipeline_layout = self._device.create_pipeline_layout(
            bind_group_layouts=[self._camera_bind_layout],
        )
        self._scene_pipeline = self._device.create_render_pipeline(
            layout=self._scene_pipeline_layout,
            vertex={
                "module": self._scene_shader,
                "entry_point": "vs_main",
                "buffers": [
                    {
                        "array_stride": 6 * 4,  # 6 floats
                        "attributes": [
                            {
                                "shader_location": 0,
                                "offset": 0,
                                "format": wgpu.VertexFormat.float32x3,
                            },
                            {
                                "shader_location": 1,
                                "offset": 3 * 4,
                                "format": wgpu.VertexFormat.float32x3,
                            },
                        ],
                    }
                ],
            },
            primitive={
                "topology": wgpu.PrimitiveTopology.triangle_list,
                "cull_mode": wgpu.CullMode.none,
            },
            depth_stencil={
                "format": wgpu.TextureFormat.depth24plus,
                "depth_write_enabled": True,
                "depth_compare": wgpu.CompareFunction.less,
            },
            fragment={
                "module": self._scene_shader,
                "entry_point": "fs_main",
                "targets": [{"format": wgpu.TextureFormat.rgba8unorm}],
            },
        )

        # --- remap pipeline ---
        self._remap_bind_layout = self._device.create_bind_group_layout(
            entries=[
                {
                    "binding": 0,
                    "visibility": wgpu.ShaderStage.FRAGMENT,
                    "texture": {
                        "sample_type": wgpu.TextureSampleType.float,
                        "view_dimension": wgpu.TextureViewDimension.cube,
                    },
                },
                {
                    "binding": 1,
                    "visibility": wgpu.ShaderStage.FRAGMENT,
                    "sampler": {"type": wgpu.SamplerBindingType.filtering},
                },
                {
                    "binding": 2,
                    "visibility": wgpu.ShaderStage.FRAGMENT,
                    "buffer": {"type": wgpu.BufferBindingType.uniform},
                },
            ],
        )
        self._remap_pipeline_layout = self._device.create_pipeline_layout(
            bind_group_layouts=[self._remap_bind_layout],
        )
        self._remap_pipeline = self._device.create_render_pipeline(
            layout=self._remap_pipeline_layout,
            vertex={"module": self._remap_shader, "entry_point": "vs_main"},
            primitive={"topology": wgpu.PrimitiveTopology.triangle_list},
            fragment={
                "module": self._remap_shader,
                "entry_point": "fs_main",
                "targets": [{"format": wgpu.TextureFormat.rgba8unorm}],
            },
        )

        # --- GPU textures ---
        self._cubemap_tex = self._device.create_texture(
            size=(cube_face_size, cube_face_size, 6),
            format=wgpu.TextureFormat.rgba8unorm,
            usage=wgpu.TextureUsage.RENDER_ATTACHMENT | wgpu.TextureUsage.TEXTURE_BINDING,
            dimension=wgpu.TextureDimension.d2,
        )
        self._cubemap_depth = self._device.create_texture(
            size=(cube_face_size, cube_face_size, 6),
            format=wgpu.TextureFormat.depth24plus,
            usage=wgpu.TextureUsage.RENDER_ATTACHMENT,
            dimension=wgpu.TextureDimension.d2,
        )
        self._cubemap_view = self._cubemap_tex.create_view(dimension=wgpu.TextureViewDimension.cube)

        self._output_tex = self._device.create_texture(
            size=(width, height, 1),
            format=wgpu.TextureFormat.rgba8unorm,
            usage=wgpu.TextureUsage.RENDER_ATTACHMENT | wgpu.TextureUsage.COPY_SRC,
        )
        self._output_view = self._output_tex.create_view()

        # Readback buffer — sized for *all* cameras so a single read_buffer()
        # call drains all 6 camera outputs at once, eliminating the per-camera
        # CPU-GPU round-trip that otherwise stalls the GPU pipeline between
        # consecutive camera renders.
        row_pitch = _align(width * 4, 256)
        self._row_pitch = row_pitch
        self._readback_slice_size = row_pitch * height  # bytes for one camera
        self._readback_buf = self._device.create_buffer(
            size=self._readback_slice_size * len(CAMERAS),
            usage=wgpu.BufferUsage.COPY_DST | wgpu.BufferUsage.COPY_SRC,
        )

        # Sampler for cubemap.
        self._cube_sampler = self._device.create_sampler(
            min_filter=wgpu.FilterMode.linear,
            mag_filter=wgpu.FilterMode.linear,
        )

        # Camera uniform buffer (mat4x4, 64 bytes).
        self._camera_ubuf = self._device.create_buffer(
            size=64,
            usage=wgpu.BufferUsage.UNIFORM | wgpu.BufferUsage.COPY_DST,
        )
        self._camera_bind = self._device.create_bind_group(
            layout=self._camera_bind_layout,
            entries=[{"binding": 0, "resource": {"buffer": self._camera_ubuf}}],
        )

        # Remap uniform buffer (4 floats = 16 bytes).
        self._remap_ubuf = self._device.create_buffer(
            size=16,
            usage=wgpu.BufferUsage.UNIFORM | wgpu.BufferUsage.COPY_DST,
        )

        # --- map (road) vertex buffer (filled by set_map_data or shared) ---
        if shared_map_buffers is not None:
            # Use shared map buffers from Place (efficient, no redundant uploads)
            self._road_vbuf = shared_map_buffers.road_vbuf
            self._road_vert_count = shared_map_buffers.road_vert_count
            self._junction_vbuf = shared_map_buffers.junction_vbuf
            self._junction_vert_count = shared_map_buffers.junction_vert_count
            self._tls_pole_vbuf = shared_map_buffers.tls_pole_vbuf
            self._tls_pole_vert_count = shared_map_buffers.tls_pole_vert_count
            self._tls_list = shared_map_buffers.tls_list
        else:
            # Initialize empty buffers (will be filled by set_map_data if called)
            self._road_vbuf: wgpu.GPUBuffer | None = None
            self._road_vert_count = 0
            self._junction_vbuf: wgpu.GPUBuffer | None = None
            self._junction_vert_count = 0
            self._tls_pole_vbuf: wgpu.GPUBuffer | None = None
            self._tls_pole_vert_count = 0
            self._tls_list: list[dict[str, Any]] = []

        # --- ground plane (rebuilt per frame, centred on ego) ---
        self._ground_vbuf: wgpu.GPUBuffer | None = None
        self._ground_vert_count = 0

        _logger.info(
            f"WgpuRenderer initialised ({width}x{height} output, {cube_face_size}x{cube_face_size} cubemap faces)."
        )

    # ---------------------------------------------------------------
    # Renderer ABC
    # ---------------------------------------------------------------

    def render(
        self,
        ego_x: float,
        ego_y: float,
        ego_yaw_deg: float,
        neighbor_vehicles: list[dict[str, Any]],
        traffic_lights: list[dict[str, Any]] | None = None,
    ) -> list[tuple[int, bytes]]:
        ego_yaw = _sumo_angle_to_yaw_rad(ego_yaw_deg)
        cam_pos = np.array([ego_x, ego_y, CAM_HEIGHT], dtype=np.float32)

        # Rebuild ground plane centred on ego each frame.
        if self._ground_vbuf is not None:
            self._ground_vbuf.destroy()
        ground_verts = _build_ground_plane(ego_x, ego_y)
        self._ground_vbuf = self._device.create_buffer_with_data(
            data=ground_verts.tobytes(),
            usage=wgpu.BufferUsage.VERTEX,
        )
        self._ground_vert_count = len(ground_verts)

        # Build dynamic vehicle vertex buffer.
        vehicle_verts = self._build_vehicle_mesh(neighbor_vehicles)
        vehicle_vbuf: wgpu.GPUBuffer | None = None
        vehicle_vert_count = 0
        if vehicle_verts.size > 0:
            vehicle_vbuf = self._device.create_buffer_with_data(
                data=vehicle_verts.tobytes(),
                usage=wgpu.BufferUsage.VERTEX,
            )
            vehicle_vert_count = len(vehicle_verts)

        # Build dynamic TLS signal head vertex buffer.
        tls_signal_vbuf: wgpu.GPUBuffer | None = None
        tls_signal_vert_count = 0
        if self._tls_list:
            active_signals: dict[str, str] = {}
            if traffic_lights:
                for tl in traffic_lights:
                    tls_id = tl.get("id", "")
                    sig = tl.get("signal", "")
                    if tls_id and sig:
                        active_signals[tls_id] = sig
            signal_verts = _build_tls_signal_mesh(self._tls_list, active_signals)
            if signal_verts.size > 0:
                tls_signal_vbuf = self._device.create_buffer_with_data(
                    data=signal_verts.tobytes(),
                    usage=wgpu.BufferUsage.VERTEX,
                )
                tls_signal_vert_count = len(signal_verts)

        # Phase 1 — GPU work: submit cubemap render + fisheye remap for every
        # camera, then immediately enqueue a copy of the output texture into the
        # corresponding slice of the batch readback buffer.  No blocking occurs
        # here; the GPU pipelines all camera passes without CPU interruption.
        for cam_slot, (_cam_id, cam_yaw_deg, cam_pitch_deg, hfov_deg) in enumerate(CAMERAS):
            cam_yaw = ego_yaw + math.radians(cam_yaw_deg)

            # Render 6 cubemap faces and remap to fisheye output texture.
            self._render_cubemap(
                cam_pos,
                cam_yaw,
                vehicle_vbuf,
                vehicle_vert_count,
                tls_signal_vbuf,
                tls_signal_vert_count,
            )
            self._remap_fisheye(hfov_deg, cam_pitch_deg)

            # Enqueue copy of output texture to this camera's slice.
            # Submitted immediately so the GPU can overlap this copy with the
            # next camera's render, but we do not block until all slices are
            # ready (see Phase 2 below).
            enc = self._device.create_command_encoder()
            enc.copy_texture_to_buffer(
                {"texture": self._output_tex, "origin": (0, 0, 0)},
                {
                    "buffer": self._readback_buf,
                    "offset": cam_slot * self._readback_slice_size,
                    "bytes_per_row": self._row_pitch,
                    "rows_per_image": self.height,
                },
                (self.width, self.height, 1),
            )
            self._device.queue.submit([enc.finish()])

        # Phase 2 — single blocking readback: drain all camera slices at once.
        # This replaces the previous 6 individual read_buffer() calls, removing
        # the GPU pipeline stalls that occurred between cameras.
        all_data = self._device.queue.read_buffer(self._readback_buf)

        # Phase 3 — CPU JPEG encode for all camera slices.
        results: list[tuple[int, bytes]] = []
        stride = self.width * 4
        for cam_slot, (cam_id, _, _, _) in enumerate(CAMERAS):
            base = cam_slot * self._readback_slice_size
            rows = [
                all_data[base + y * self._row_pitch : base + y * self._row_pitch + stride] for y in range(self.height)
            ]
            raw = b"".join(rows)
            img = np.frombuffer(raw, dtype=np.uint8).reshape(self.height, self.width, 4)
            bgr = cv2.cvtColor(img, cv2.COLOR_RGBA2BGR)
            ok, buf = cv2.imencode(".jpg", bgr, [cv2.IMWRITE_JPEG_QUALITY, self.jpeg_quality])
            results.append((cam_id, bytes(buf) if ok else b""))

        # Clean up dynamic buffers.
        if vehicle_vbuf is not None:
            vehicle_vbuf.destroy()
        if tls_signal_vbuf is not None:
            tls_signal_vbuf.destroy()

        return results

    # ---------------------------------------------------------------
    # Internal
    # ---------------------------------------------------------------

    def _build_vehicle_mesh(self, neighbors: list[dict[str, Any]]) -> np.ndarray:
        """Build triangle vertices for all nearby vehicles."""
        all_verts: list[np.ndarray] = []
        for nb in neighbors:
            nx = float(nb.get("x", 0.0))
            ny = float(nb.get("y", 0.0))
            n_yaw = _sumo_angle_to_yaw_rad(float(nb.get("angle", 0.0)))
            box = _build_box_vertices(nx, ny, n_yaw)
            all_verts.append(box)
        if not all_verts:
            return np.zeros((0, 6), dtype=np.float32)
        return np.concatenate(all_verts, axis=0)

    def _render_cubemap(
        self,
        cam_pos: np.ndarray,
        cam_yaw: float,
        vehicle_vbuf: wgpu.GPUBuffer | None,
        vehicle_vert_count: int,
        tls_signal_vbuf: wgpu.GPUBuffer | None = None,
        tls_signal_vert_count: int = 0,
    ) -> None:
        """Render the scene into all 6 cubemap faces.

        All 6 faces are batched into a single command encoder and submitted once
        to reduce GPU submission overhead (6 submits → 1 submit per camera).
        """
        sky = SKY_COLOUR
        clear_color = (sky[0] / 255.0, sky[1] / 255.0, sky[2] / 255.0, 1.0)

        # Create single encoder for all 6 cubemap faces (batching optimization)
        encoder = self._device.create_command_encoder()

        for face_idx, (fwd, up) in enumerate(_CUBE_FACE_DIRS):
            # Rotate the cubemap face direction by camera yaw.
            c, s = math.cos(cam_yaw), math.sin(cam_yaw)
            rfwd = (fwd[0] * c - fwd[1] * s, fwd[0] * s + fwd[1] * c, fwd[2])
            rup = (up[0] * c - up[1] * s, up[0] * s + up[1] * c, up[2])

            target = cam_pos + np.array(rfwd, dtype=np.float32)
            view = _look_at(cam_pos, target, np.array(rup, dtype=np.float32))
            vp = (self._cube_proj @ view).astype(np.float32)

            # WGSL mat4x4 is column-major; NumPy is row-major → transpose.
            self._device.queue.write_buffer(self._camera_ubuf, 0, np.ascontiguousarray(vp.T).tobytes())

            color_view = self._cubemap_tex.create_view(
                dimension=wgpu.TextureViewDimension.d2,
                base_array_layer=face_idx,
                array_layer_count=1,
            )
            depth_view = self._cubemap_depth.create_view(
                dimension=wgpu.TextureViewDimension.d2,
                base_array_layer=face_idx,
                array_layer_count=1,
            )

            rpass = encoder.begin_render_pass(
                color_attachments=[
                    {
                        "view": color_view,
                        "load_op": wgpu.LoadOp.clear,
                        "store_op": wgpu.StoreOp.store,
                        "clear_value": clear_color,
                    }
                ],
                depth_stencil_attachment={
                    "view": depth_view,
                    "depth_load_op": wgpu.LoadOp.clear,
                    "depth_store_op": wgpu.StoreOp.store,
                    "depth_clear_value": 1.0,
                },
            )
            rpass.set_pipeline(self._scene_pipeline)
            rpass.set_bind_group(0, self._camera_bind)

            # Draw ground plane (rendered first, below roads at z=-0.01).
            if self._ground_vbuf is not None and self._ground_vert_count > 0:
                rpass.set_vertex_buffer(0, self._ground_vbuf)
                rpass.draw(self._ground_vert_count)

            # Draw junction area polygons (at z=0.0, same level as roads).
            if self._junction_vbuf is not None and self._junction_vert_count > 0:
                rpass.set_vertex_buffer(0, self._junction_vbuf)
                rpass.draw(self._junction_vert_count)

            # Draw road.
            if self._road_vbuf is not None and self._road_vert_count > 0:
                rpass.set_vertex_buffer(0, self._road_vbuf)
                rpass.draw(self._road_vert_count)

            # Draw vehicles.
            if vehicle_vbuf is not None and vehicle_vert_count > 0:
                rpass.set_vertex_buffer(0, vehicle_vbuf)
                rpass.draw(vehicle_vert_count)

            # Draw traffic light poles (static).
            if self._tls_pole_vbuf is not None and self._tls_pole_vert_count > 0:
                rpass.set_vertex_buffer(0, self._tls_pole_vbuf)
                rpass.draw(self._tls_pole_vert_count)

            # Draw traffic light signal heads (dynamic colour).
            if tls_signal_vbuf is not None and tls_signal_vert_count > 0:
                rpass.set_vertex_buffer(0, tls_signal_vbuf)
                rpass.draw(tls_signal_vert_count)

            rpass.end()

        # Submit all 6 faces in a single batch
        self._device.queue.submit([encoder.finish()])

    def _remap_fisheye(self, hfov_deg: float, pitch_deg: float) -> None:
        """Remap cubemap to equidistant fisheye output texture."""
        hfov = math.radians(hfov_deg)
        vfov = hfov * (self.height / self.width)
        pitch = math.radians(pitch_deg)

        params = struct.pack("ffff", hfov, vfov, 0.0, pitch)
        self._device.queue.write_buffer(self._remap_ubuf, 0, params)

        remap_bind = self._device.create_bind_group(
            layout=self._remap_bind_layout,
            entries=[
                {"binding": 0, "resource": self._cubemap_view},
                {"binding": 1, "resource": self._cube_sampler},
                {"binding": 2, "resource": {"buffer": self._remap_ubuf}},
            ],
        )

        encoder = self._device.create_command_encoder()
        rpass = encoder.begin_render_pass(
            color_attachments=[
                {
                    "view": self._output_view,
                    "load_op": wgpu.LoadOp.clear,
                    "store_op": wgpu.StoreOp.store,
                    "clear_value": (0, 0, 0, 1),
                }
            ],
        )
        rpass.set_pipeline(self._remap_pipeline)
        rpass.set_bind_group(0, remap_bind)
        rpass.draw(3)  # full-screen triangle
        rpass.end()

        self._device.queue.submit([encoder.finish()])


def _align(value: int, alignment: int) -> int:
    """Round *value* up to the nearest multiple of *alignment*."""
    return (value + alignment - 1) // alignment * alignment


def create_shared_map_buffers(
    device: wgpu.GPUDevice,
    map_data: BaseMap,
) -> SharedMapBuffers:
    """Create shared GPU buffers for road-network geometry.

    This function should be called once per Place to create buffers that are
    shared across all Edge renderers, avoiding redundant GPU uploads.

    Args:
        device: GPU device to create buffers on.
        map_data: BaseMap instance providing lanes, tls_list, and junctions.

    Returns:
        SharedMapBuffers containing all static geometry buffers.
    """
    lanes = map_data.lanes
    tls_list = map_data.tls_list
    junctions = map_data.junctions
    # Road geometry
    mesh = _build_road_mesh(lanes)
    if mesh.size == 0:
        road_vbuf = None
        road_vert_count = 0
    else:
        road_vbuf = device.create_buffer_with_data(
            data=mesh.tobytes(),
            usage=wgpu.BufferUsage.VERTEX,
        )
        road_vert_count = len(mesh)
        _logger.info("Created shared road mesh buffer: %d vertices.", road_vert_count)

    # Junction geometry
    if junctions:
        junc_mesh = _build_junction_mesh(junctions)
        if junc_mesh.size == 0:
            junction_vbuf = None
            junction_vert_count = 0
        else:
            junction_vbuf = device.create_buffer_with_data(
                data=junc_mesh.tobytes(),
                usage=wgpu.BufferUsage.VERTEX,
            )
            junction_vert_count = len(junc_mesh)
            _logger.info(
                "Created shared junction mesh buffer: %d vertices for %d junctions.",
                junction_vert_count,
                len(junctions),
            )
    else:
        junction_vbuf = None
        junction_vert_count = 0

    # Traffic light poles
    tls_list_copy = list(tls_list) if tls_list else []
    if tls_list_copy:
        pole_mesh = _build_tls_pole_mesh(tls_list_copy)
        if pole_mesh.size == 0:
            tls_pole_vbuf = None
            tls_pole_vert_count = 0
        else:
            tls_pole_vbuf = device.create_buffer_with_data(
                data=pole_mesh.tobytes(),
                usage=wgpu.BufferUsage.VERTEX,
            )
            tls_pole_vert_count = len(pole_mesh)
            _logger.info(
                "Created shared TLS pole mesh buffer: %d vertices for %d traffic lights.",
                tls_pole_vert_count,
                len(tls_list_copy),
            )
    else:
        tls_pole_vbuf = None
        tls_pole_vert_count = 0

    return SharedMapBuffers(
        road_vbuf=road_vbuf,
        road_vert_count=road_vert_count,
        junction_vbuf=junction_vbuf,
        junction_vert_count=junction_vert_count,
        tls_pole_vbuf=tls_pole_vbuf,
        tls_pole_vert_count=tls_pole_vert_count,
        tls_list=tls_list_copy,
    )
