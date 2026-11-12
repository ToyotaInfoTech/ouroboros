# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Per-edge behavior runtime that drives the trigger → tick → end lifecycle.

One ``BehaviorRuntime`` instance is held per ``base.Edge``.  On each simulation
tick the caller invokes ``step(state, dt_sec)`` which returns an action-patch
dict (may be empty) that the router merges into the action sent to SUMO.

Activation logic (per call to ``step``):
1. If a behavior is already running, call its ``tick``.  If ``tick`` returns
   ``None`` the behavior ends (``end`` is called) and cooldown starts.
2. Otherwise, roll a random number.  If it falls below
   ``probability * dt_sec`` **and** cooldown has elapsed, pick a behavior
   weighted-randomly from the enabled set whose ``applicable`` returns True,
   then call ``begin``.
"""

from __future__ import annotations

import logging
import time
from dataclasses import dataclass
from random import Random
from typing import Any

from .registry import REGISTRY, Behavior, BehaviorContext, RegistryEntry

_logger = logging.getLogger(__name__)


@dataclass
class _OngoingBehavior:
    """Tracks an in-progress behavior activation."""

    instance: Behavior
    bstate: dict
    started_at: float


class BehaviorRuntime:
    """Per-edge runtime that manages behavior lifecycle.

    Args:
        edge_id: Unique identifier for the edge / vehicle.
        config: Configuration dict loaded from ``dangerous_behaviors.toml``.
    """

    def __init__(self, edge_id: str, config: dict[str, Any]):
        self._edge_id = edge_id
        self._enabled: bool = config.get("enabled", False)
        self._probability: float = config.get("probability", 0.001)
        self._cooldown_sec: float = config.get("cooldown_sec", 60.0)
        self._enabled_kinds: list[str] = config.get("enabled_kinds", [])
        self._params: dict[str, dict[str, Any]] = config.get("params", {})

        seed = config.get("seed", 0)
        self._rng = Random(seed + hash(edge_id))

        self._ongoing: _OngoingBehavior | None = None
        self._last_end_time: float = 0.0

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def active_behavior_name(self) -> str | None:
        """Return the name of the currently active behavior, or None."""
        return self._ongoing.instance.name if self._ongoing else None

    def step(self, state: dict[str, Any], dt_sec: float) -> dict[str, Any]:
        """Advance one tick and return an action-patch dict (may be empty).

        Args:
            state: Current SUMO vehicle state dict.
            dt_sec: Seconds elapsed since the previous tick.

        Returns:
            Action-patch dict to merge into the SUMO action for this vehicle.
        """
        if not self._enabled:
            return {}

        now = time.monotonic()
        dt_ms = dt_sec * 1000.0
        ctx = BehaviorContext(
            edge_id=self._edge_id,
            state=state,
            dt_sec=dt_sec,
            dt_ms=dt_ms,
            rng=self._rng,
        )

        # --- ongoing behavior ---
        if self._ongoing:
            return self._tick_ongoing(ctx, now)

        # --- try to activate a new behavior ---
        if now - self._last_end_time < self._cooldown_sec:
            return {}

        if self._rng.random() >= self._probability * dt_sec:
            return {}

        return self._try_activate(ctx, now)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _tick_ongoing(self, ctx: BehaviorContext, now: float) -> dict[str, Any]:
        """Drive the currently active behavior forward by one tick."""
        ongoing = self._ongoing
        assert ongoing

        ctx = BehaviorContext(
            edge_id=ctx.edge_id,
            state=ctx.state,
            dt_sec=ctx.dt_sec,
            dt_ms=ctx.dt_ms,
            rng=ctx.rng,
            params=self._params.get(ongoing.instance.name, {}),
        )

        patch = ongoing.instance.tick(ongoing.bstate, ctx)
        if not patch:
            # Behavior finished — call end() for cleanup action
            end_patch = ongoing.instance.end(ongoing.bstate, ctx) or {}
            self._ongoing = None
            self._last_end_time = now
            _logger.debug(f"Behavior '{ongoing.instance.name}' ended for edge '{self._edge_id}'.")
            return end_patch
        return patch

    def _try_activate(self, ctx: BehaviorContext, now: float) -> dict[str, Any]:
        """Pick a behavior weighted-randomly from enabled+applicable set."""
        candidates = self._get_candidates(ctx)
        _logger.debug(f"Found {len(candidates)} candidate behaviors for edge '{self._edge_id}'.")
        if not candidates:
            return {}

        instance = self._weighted_pick(candidates, ctx)
        act_ctx = BehaviorContext(
            edge_id=ctx.edge_id,
            state=ctx.state,
            dt_sec=ctx.dt_sec,
            dt_ms=ctx.dt_ms,
            rng=ctx.rng,
            params=self._params.get(instance.name, {}),
        )
        bstate = instance.begin(act_ctx)
        self._ongoing = _OngoingBehavior(instance=instance, bstate=bstate, started_at=now)
        _logger.debug(f"Behavior '{instance.name}' activated for edge '{self._edge_id}'.")

        # Run the first tick immediately so the activation frame produces
        # an action rather than a blank dict.
        patch = instance.tick(bstate, act_ctx)
        if not patch:
            end_patch = instance.end(bstate, act_ctx) or {}
            self._ongoing = None
            self._last_end_time = now
            return end_patch
        return patch

    def _get_candidates(self, ctx: BehaviorContext) -> list[Behavior]:
        """Return instantiated behaviors that are enabled and applicable."""
        candidates: list[Behavior] = []
        for name in self._enabled_kinds:
            entry: RegistryEntry | None = REGISTRY.get(name)
            if entry is None:
                continue
            instance = entry.cls()
            instance.name = entry.name
            instance.weight = entry.weight
            cand_ctx = BehaviorContext(
                edge_id=ctx.edge_id,
                state=ctx.state,
                dt_sec=ctx.dt_sec,
                dt_ms=ctx.dt_ms,
                rng=ctx.rng,
                params=self._params.get(name, {}),
            )
            if instance.applicable(cand_ctx):
                candidates.append(instance)
        return candidates

    def _weighted_pick(self, candidates: list[Behavior], ctx: BehaviorContext) -> Behavior:
        """Select one behavior from candidates using weighted random."""
        weights = [c.weight for c in candidates]
        total = sum(weights)
        r = self._rng.random() * total
        cumulative = 0.0
        for cand, w in zip(candidates, weights, strict=True):
            cumulative += w
            if r <= cumulative:
                return cand
        return candidates[-1]  # return the last one
