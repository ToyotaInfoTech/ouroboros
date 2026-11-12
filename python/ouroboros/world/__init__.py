# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
The World module

This module contains implementations for managing a distributed cluster.
Currently, it includes a Ray-based cluster implementation.

Usage:
    from cluster import new_world
    world = new_world("ray", min_edges_per_place=2, max_places=5)
"""

from . import local_world, ray_world
from .base import World

__all__ = ["World", "new_world"]

_FACTORY = {
    "ray": ray_world.World,
    "local": local_world.World,
}


def new_world(name: str, *args, **kwargs) -> World:
    """
    Factory method to create a world instance.

    Args:
        name (str): Name of the world type.
        *args: Positional arguments to pass to the world constructor.
        **kwargs: Keyword arguments to pass to the world constructor.
    """
    return _FACTORY[name](*args, **kwargs)
