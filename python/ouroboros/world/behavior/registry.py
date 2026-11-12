# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Behavior registry and protocol definitions.

Every behavior — whether written as a Python class or loaded from TOML —
is represented as an instance that satisfies the ``Behavior`` protocol and
is registered in the global ``REGISTRY`` via the ``@behavior`` decorator.

Example (Python class)::

    @behavior(name="hard_brake", weight=1.0)
    class HardBrake:
        duration_ms = 800
        decel = 8.0

        def applicable(self, ctx: BehaviorContext) -> bool:
            return ctx.state["speed"] > 5.0

        def begin(self, ctx: BehaviorContext) -> dict:
            return {"elapsed_ms": 0}

        def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
            bstate["elapsed_ms"] += ctx.dt_ms
            if bstate["elapsed_ms"] >= self.duration_ms:
                return None  # signals end
            return {"target_decel": self.decel, "target_speed": 0.0,
                    "color": (255, 0, 0, 255), "tag": "hard_brake"}

        def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
            return {"restore": True}
"""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass, field
from random import Random
from typing import Any, ClassVar, Protocol, runtime_checkable

_logger = logging.getLogger(__name__)


# ---------------------------------------------------------------------------
# Context passed to every behavior callback
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BehaviorContext:
    """Immutable snapshot passed to behavior callbacks each tick.

    Attributes:
        edge_id: Unique identifier of the edge / vehicle.
        state: Current SUMO state dict for the vehicle.
        dt_sec: Wall-clock seconds since the last tick.
        dt_ms: Milliseconds since the last tick.
        rng: Per-edge seeded RNG for reproducibility.
        params: Per-behavior parameter overrides from config.
    """

    edge_id: str
    state: dict[str, Any]
    dt_sec: float
    dt_ms: float
    rng: Random
    params: dict[str, Any] = field(default_factory=dict)


# ---------------------------------------------------------------------------
# Behavior protocol
# ---------------------------------------------------------------------------


@runtime_checkable
class Behavior(Protocol):
    """Duck-typed interface that every behavior must satisfy."""

    name: str
    weight: float

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Return True if this behavior may activate given current state."""
        ...

    def begin(self, ctx: BehaviorContext) -> dict:
        """Initialize per-activation mutable state and return it."""
        ...

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        """Advance one tick.  Return action-patch dict, or None to end."""
        ...

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        """Produce a final action-patch (e.g. restore defaults), or None."""
        ...


class BehaviorBase:
    """Base class for typed behavior implementations.

    The ``@behavior`` decorator assigns these class attributes at runtime.
    Declaring them here keeps static analyzers from flagging ``self.name`` /
    ``self.weight`` accesses in concrete behavior classes.
    """

    name: ClassVar[str]
    weight: ClassVar[float]


# ---------------------------------------------------------------------------
# Registry entry and global store
# ---------------------------------------------------------------------------


@dataclass
class RegistryEntry:
    """A behavior class + its default weight."""

    name: str
    weight: float
    cls: type


REGISTRY: dict[str, RegistryEntry] = {}


def _camel_to_snake(cls_name: str) -> str:
    """Convert CamelCase class name to snake_case."""
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", cls_name).lower()


def behavior(name: str | None = None, weight: float = 1.0):
    """Class decorator that registers a behavior in the global registry.

    Args:
        name: Unique behavior identifier (e.g. ``"hard_brake"``).  Derived
              from the class name (CamelCase → snake_case) when ``None``.
        weight: Default relative weight for random selection.

    Returns:
        The original class, unmodified except for ``name`` / ``weight`` attrs.
    """

    def _wrap(cls):
        resolved = name or _camel_to_snake(cls.__name__)
        cls.name = resolved
        cls.weight = weight
        if resolved in REGISTRY:
            _logger.warning("Behavior '%s' already registered; overwriting.", resolved)
        REGISTRY[resolved] = RegistryEntry(name=resolved, weight=weight, cls=cls)
        return cls

    return _wrap


def get_registered_names() -> list[str]:
    """Return sorted list of all registered behavior names."""
    return sorted(REGISTRY.keys())
