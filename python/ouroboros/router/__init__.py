# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
router module

This module contains implementations for different types of routers.
Currently, it includes a SUMO-based router implementation.
Usage:
    from router import new_router
    router = new_router("sumo", log_out=..., world=...)
"""

from ouroboros.world.map import BaseMap

from .base import Router
from .sumo import SumoRouter
from .sumo_map import SumoMap

__all__ = ["BaseMap", "Router", "SumoMap", "new_map", "new_router"]

_ROUTER_FACTORY = {
    "sumo": SumoRouter,
}

_MAP_FACTORY: dict[str, type[SumoMap]] = {
    "sumo": SumoMap,
}


def new_router(name: str, *args, **kwargs) -> Router:
    """
    Factory method to create a router instance.

    Args:
        name (str): Name of the router type.
        *args: Positional arguments to pass to the router constructor.
        **kwargs: Keyword arguments to pass to the router constructor.
    """
    return _ROUTER_FACTORY[name](*args, **kwargs)


def new_map(name: str) -> BaseMap:
    """
    Factory method to collect a road-network map from an active simulator session.

    The simulator must already be connected (e.g. via ``SumoRouter.connect_to_sim``)
    before calling this function.

    Args:
        name (str): Name of the map source. Currently only ``"sumo"`` is supported.

    Returns:
        BaseMap: Populated map instance ready to pass to ``world.set_map_data()``.
    """
    cls = _MAP_FACTORY[name]
    return cls.collect()
