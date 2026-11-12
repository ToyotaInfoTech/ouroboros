# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
SUMO road-network map implementation.

``SumoMap`` collects static road-network geometry by reading the SUMO net file
directly via ``sumolib``, bypassing per-element TraCI round trips that are
prohibitively expensive on large networks.  The ``collect`` class method is the
primary entry point; it must be called **after** ``traci.init()`` or
``traci.start()`` so that the net-file path can be resolved from the active
simulation options.

Typical usage::

    router.connect_to_sim(host, port)          # establishes TraCI session
    await router.run(world=world)              # collects map internally and runs
"""

import logging
from typing import Any

import sumolib
import traci

from ouroboros.world.map import BaseMap

__all__ = ["SumoMap"]

_logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# SumoMap
# ---------------------------------------------------------------------------


class SumoMap(BaseMap):
    """Static road-network snapshot read from the SUMO net file via sumolib.

    Instances are immutable; create a new ``SumoMap`` if the network
    changes (which is uncommon for SUMO scenarios).
    """

    def __init__(
        self,
        lanes: list[dict[str, Any]],
        tls_list: list[dict[str, Any]],
        junctions: list[dict[str, Any]],
        geo_proj: Any | None = None,
        geo_proj_offset: tuple[float, float] = (0.0, 0.0),
    ) -> None:
        """
        Args:
            lanes: Lane centreline shapes and widths.
            tls_list: Traffic light positions and controlled lanes.
            junctions: Junction boundary polygons.
            geo_proj: A ``pyproj.Proj`` instance for local XY→lonlat conversion,
                or ``None`` to fall back to TraCI ``convertGeo``.
            geo_proj_offset: (x_off, y_off) netOffset from the SUMO location
                block, applied before projecting.
        """
        self._lanes = lanes
        self._tls_list = tls_list
        self._junctions = junctions
        self._geo_proj = geo_proj
        self._geo_proj_offset = geo_proj_offset

    # ------------------------------------------------------------------
    # BaseMap interface
    # ------------------------------------------------------------------

    @property
    def geo_proj(self) -> Any | None:
        """``pyproj.Proj`` instance for local XY→lonlat conversion, or ``None``."""
        return self._geo_proj

    @property
    def geo_proj_offset(self) -> tuple[float, float]:
        """Net offset (x_off, y_off) to subtract before projecting."""
        return self._geo_proj_offset

    @property
    def lanes(self) -> list[dict[str, Any]]:
        """Lane centreline shapes and widths."""
        return self._lanes

    @property
    def tls_list(self) -> list[dict[str, Any]]:
        """Traffic light positions and controlled lane info."""
        return self._tls_list

    @property
    def junctions(self) -> list[dict[str, Any]]:
        """Junction boundary polygons."""
        return self._junctions

    # ------------------------------------------------------------------
    # Factory
    # ------------------------------------------------------------------

    @classmethod
    def collect(cls) -> "SumoMap":
        """Read static map data from the SUMO net file via sumolib.

        Resolves the net-file path from the active TraCI session options and
        parses the network with ``sumolib``.  This replaces per-element TraCI
        queries (which require one round trip per lane/junction) with a single
        file parse, dramatically reducing startup time on large networks.

        Must be called after ``traci.init()`` or ``traci.start()``.

        Returns:
            SumoMap: Populated map instance.
        """
        net_file = traci.simulation.getOption("net-file")
        _logger.info("Reading network from '%s'.", net_file)

        net = sumolib.net.readNet(net_file, withInternal=True)

        # -- geo projection -----------------------------------------------
        # Extract pyproj parameters once so the router can do XY→lonlat locally
        # instead of paying a TraCI round-trip per vehicle per step.
        geo_proj: Any | None = None
        geo_proj_offset: tuple[float, float] = (0.0, 0.0)
        if net.hasGeoProj():
            try:
                geo_proj = net.getGeoProj()
                geo_proj_offset = net.getLocationOffset()
                _logger.info("Geo projection available; local XY→lonlat enabled.")
            except Exception as exc:  # pyproj not installed or projection error
                _logger.warning("Could not initialise geo projection: %s. TraCI convertGeo will be used.", exc)

        # -- lanes --------------------------------------------------------
        lanes: list[dict[str, Any]] = []
        for edge in net.getEdges(withInternal=True):
            for lane in edge.getLanes():
                shape = lane.getShape()
                if shape:
                    lanes.append({"shape": list(shape), "width": lane.getWidth()})

        # -- traffic lights -----------------------------------------------
        tls_list: list[dict[str, Any]] = []
        for tls in net.getTrafficLights():
            tls_id = tls.getID()
            if not net.hasNode(tls_id):
                _logger.debug("Traffic light '%s' has no matching junction node; skipping.", tls_id)
                continue
            x, y = net.getNode(tls_id).getCoord()
            controlled_lanes = [conn[0].getID() for conn in tls.getConnections()]
            tls_list.append({"id": tls_id, "x": x, "y": y, "controlled_lanes": controlled_lanes})

        # -- junctions ----------------------------------------------------
        junctions: list[dict[str, Any]] = []
        for node in net.getNodes():
            shape = node.getShape()
            if len(shape) >= 3:
                junctions.append({"shape": list(shape)})

        _logger.info(
            "Collected SumoMap: %d lanes, %d traffic lights, %d junctions.",
            len(lanes),
            len(tls_list),
            len(junctions),
        )
        return cls(
            lanes=lanes, tls_list=tls_list, junctions=junctions, geo_proj=geo_proj, geo_proj_offset=geo_proj_offset
        )
