# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Renderer abstraction for generating surround-view camera images from
simulation state.

A Renderer is owned by a Place and shared across all Edges within
that Place.  Road-network geometry is provided at construction time
via SharedMapBuffers and produces 6 JPEG-encoded camera images per
call to render.

Camera layout:
    6 cameras at 60-degree yaw intervals, each with 120-degree horizontal
    FOV and -5-degree pitch, providing full 360-degree coverage with no
    blind spots.
"""

from abc import ABC, abstractmethod
from typing import Any

from ..gen.peer_generated import CameraIdEnum

# ---------------------------------------------------------------------------
# Shared constants (reused by concrete renderer implementations)
# ---------------------------------------------------------------------------

# Camera definitions: (camera_id, yaw_deg, pitch_deg, hfov_deg)
CAMERAS: list[tuple[int, float, float, float]] = [
    (CameraIdEnum.Front, 0.0, -5.0, 120.0),
    (CameraIdEnum.FrontLeft, 60.0, -5.0, 120.0),
    (CameraIdEnum.FrontRight, -60.0, -5.0, 120.0),
    (CameraIdEnum.RearLeft, 120.0, -5.0, 120.0),
    (CameraIdEnum.RearRight, -120.0, -5.0, 120.0),
    (CameraIdEnum.Rear, 180.0, -5.0, 120.0),
]

# Default vehicle box dimensions (length, width, height) in metres.
DEFAULT_LENGTH = 4.5
DEFAULT_WIDTH = 1.8
DEFAULT_HEIGHT = 1.5

# Camera mount height in metres (roof centre).
CAM_HEIGHT = 1.5

# Per-face colours (RGB, 0-255).
FACE_COLOURS = [
    (220, 180, 180),  # front  - light grey-blue
    (200, 160, 160),  # rear
    (200, 170, 140),  # left
    (200, 170, 140),  # right
    (210, 200, 200),  # top
    (100, 100, 100),  # bottom
]

# Road surface colour (RGB).
ROAD_COLOUR = (50, 50, 55)
# Lane edge / marking colour (RGB).
LANE_EDGE_COLOUR = (200, 200, 200)
# Sky colour (RGB).
SKY_COLOUR = (135, 190, 235)
# Ground plane colour (RGB) – visible terrain that is not road.
GROUND_COLOUR = (115, 140, 90)
# Vehicle box edge colour (RGB).
BOX_EDGE_COLOUR = (80, 60, 60)

# Traffic light pole colour (RGB).
TLS_POLE_COLOUR = (80, 80, 80)
# Traffic light pole dimensions (metres).
TLS_POLE_WIDTH = 0.15
TLS_POLE_HEIGHT = 3.5
# Traffic light signal head dimensions (metres).
TLS_HEAD_SIZE = 0.4
TLS_HEAD_Z = 3.2  # bottom of signal head above ground
# Signal colour mapping: SUMO signal char → RGB.
TLS_SIGNAL_COLOURS = {
    "r": (220, 30, 30),
    "R": (220, 30, 30),
    "g": (30, 200, 30),
    "G": (30, 200, 30),
    "y": (230, 210, 30),
    "Y": (230, 210, 30),
    "o": (128, 128, 128),
    "O": (128, 128, 128),
    "s": (128, 128, 128),
    "u": (128, 128, 128),
}
# Default signal colour when state is unknown.
TLS_DEFAULT_COLOUR = (128, 128, 128)


# ---------------------------------------------------------------------------
# Abstract base class
# ---------------------------------------------------------------------------


class Renderer(ABC):
    """Place-level renderer that produces surround-view camera images.

    One instance per Place.  Road-network geometry is provided at
    construction time via SharedMapBuffers.  Each call to render
    generates images for a single
    ego viewpoint.

    Args:
        width: Output image width in pixels.
        height: Output image height in pixels.
        jpeg_quality: JPEG compression quality (0-100).
        max_range: Maximum render distance from ego in metres.
    """

    def __init__(
        self,
        width: int = 640,
        height: int = 360,
        jpeg_quality: int = 60,
        max_range: float = 80.0,
    ):
        self.width = width
        self.height = height
        self.jpeg_quality = jpeg_quality
        self.max_range = max_range

    @abstractmethod
    def render(
        self,
        ego_x: float,
        ego_y: float,
        ego_yaw_deg: float,
        neighbor_vehicles: list[dict[str, Any]],
        traffic_lights: list[dict[str, Any]] | None = None,
    ) -> list[tuple[int, bytes]]:
        """Render all 6 cameras for a single ego viewpoint.

        Args:
            ego_x: Ego vehicle X position (SUMO coords).
            ego_y: Ego vehicle Y position (SUMO coords).
            ego_yaw_deg: Ego heading in SUMO convention (0 = north, CW).
            neighbor_vehicles: List of nearby vehicle dicts, each with
                at least "x", "y", "angle" keys.
            traffic_lights: Optional list of upcoming traffic light dicts,
                each with "id" (str) and "signal" (str) keys.  Used to
                colour the signal heads of previously registered TLS
                positions.

        Returns:
            List of (camera_id, jpeg_bytes) tuples (length 6).
        """


# ---------------------------------------------------------------------------
# Factory
# ---------------------------------------------------------------------------

__all__ = ["Renderer", "new_renderer"]

_FACTORY: dict[str, type] = {}


def _register_renderer(name: str, cls: type) -> None:
    """Register a renderer class under the given name."""
    _FACTORY[name] = cls


def new_renderer(name: str, *args, **kwargs) -> Renderer:
    """Create a renderer instance by name.

    Args:
        name: Registered renderer name (e.g. "wgpu").

    Raises:
        KeyError: If name is not a registered renderer.
    """
    _lazy_register()
    return _FACTORY[name](*args, **kwargs)


def _lazy_register() -> None:
    """Import and register available renderers on first factory use."""
    if _FACTORY:
        return
    try:
        from .wgpu_renderer import WgpuRenderer

        _register_renderer("wgpu", WgpuRenderer)
    except ImportError:
        pass
