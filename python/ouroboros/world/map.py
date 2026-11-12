# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Abstract road-network map representation.

``BaseMap`` is the common interface that World, Place, and Renderer use to
consume road-network geometry.  Concrete subclasses (e.g. ``SumoMap``) are
responsible for collecting or constructing the actual geometry from their
respective source.

The ``new_map`` factory mirrors the ``new_world`` / ``new_renderer`` /
``new_router`` pattern: callers create a map by name without depending on a
concrete implementation.

Typical usage::

    # Collect map from a live TraCI session (after traci.init / traci.start)
    from ouroboros.router import new_map
    sumo_map = new_map("sumo")

    world.set_map_data(sumo_map)
    renderer.set_map_data(sumo_map)
"""

from abc import ABC, abstractmethod
from typing import Any

__all__ = ["BaseMap"]


class BaseMap(ABC):
    """Abstract representation of static road-network geometry.

    Concrete implementations collect geometry from a specific source
    (TraCI, file, replay log, etc.) and expose it through this interface.

    All three properties are read-only; the map is treated as immutable
    once constructed.
    """

    @property
    @abstractmethod
    def lanes(self) -> list[dict[str, Any]]:
        """Lane centreline shapes and widths.

        Returns:
            List of dicts, each with:
                - ``"shape"``: list of ``(x, y)`` tuples
                - ``"width"``: float, lane width in metres
        """

    @property
    @abstractmethod
    def tls_list(self) -> list[dict[str, Any]]:
        """Traffic light positions and controlled lane info.

        Returns:
            List of dicts, each with:
                - ``"id"``: str, traffic light identifier
                - ``"x"``: float
                - ``"y"``: float
                - ``"controlled_lanes"``: list of str
        """

    @property
    @abstractmethod
    def junctions(self) -> list[dict[str, Any]]:
        """Junction boundary polygons.

        Returns:
            List of dicts, each with:
                - ``"shape"``: list of ``(x, y)`` tuples
        """
