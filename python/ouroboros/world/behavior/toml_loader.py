# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Load behavior definitions from declarative TOML files.

Each ``.toml`` file in a pack directory describes one behavior.  The loader
parses the file, builds a ``Behavior``-compatible class on the fly, and
registers it via the ``@behavior`` decorator.

TOML schema::

    name = "hard_brake"
    weight = 1.0
    applicable = "state['speed'] > 5"
    duration_ms = 800

    [[timeline]]
    at = [0, 800]          # inclusive start, exclusive end (ms)

    [timeline.action]
    target_decel = 8.0
    target_speed = 0.0
    tag = "hard_brake"

    [on_end]
    restore = true

``applicable`` is evaluated with a restricted set of names (only ``state``
and built-in math) to prevent arbitrary code execution.
"""

from __future__ import annotations

import logging
import math
import os
import tomllib
from typing import Any

from .registry import BehaviorContext, behavior

_logger = logging.getLogger(__name__)

# Minimal safe namespace for evaluating ``applicable`` expressions.
_SAFE_GLOBALS: dict[str, Any] = {"__builtins__": {}, "math": math, "abs": abs, "min": min, "max": max}


def _make_applicable_fn(expr: str):
    """Build a function that evaluates the given expression safely.

    Args:
        expr: A Python expression referencing ``state`` (a dict).

    Returns:
        A callable ``(ctx: BehaviorContext) -> bool``.
    """
    compiled = compile(expr, "<toml-applicable>", "eval")

    def _fn(ctx: BehaviorContext) -> bool:
        local_ns = {"state": ctx.state}
        return bool(eval(compiled, _SAFE_GLOBALS, local_ns))  # noqa: S307, RUF100

    return _fn


class _TimelineEntry:
    """One segment of the behavior timeline."""

    __slots__ = ("action", "end_ms", "start_ms")

    def __init__(self, start_ms: float, end_ms: float, action: dict[str, Any]):
        self.start_ms = start_ms
        self.end_ms = end_ms
        self.action = action


def _build_behavior_class(spec: dict[str, Any]) -> type:
    """Construct a class that satisfies the Behavior protocol from TOML data.

    Args:
        spec: Parsed TOML dict for one behavior file.

    Returns:
        A new class registered in the global behavior registry.
    """
    bname: str = spec["name"]
    bweight: float = float(spec.get("weight", 1.0))
    applicable_expr: str = spec.get("applicable", "True")
    duration_ms: float = float(spec.get("duration_ms", 1000))
    on_end: dict[str, Any] = spec.get("on_end", {"restore": True})

    timeline: list[_TimelineEntry] = []
    for entry in spec.get("timeline", []):
        at = entry["at"]
        timeline.append(
            _TimelineEntry(
                start_ms=float(at[0]),
                end_ms=float(at[1]),
                action=dict(entry.get("action", {})),
            )
        )

    applicable_fn = _make_applicable_fn(applicable_expr)

    class _TomlBehavior:
        name = bname
        weight = bweight

        def applicable(self, ctx: BehaviorContext) -> bool:
            return applicable_fn(ctx)

        def begin(self, ctx: BehaviorContext) -> dict:
            return {"elapsed_ms": 0.0}

        def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
            bstate["elapsed_ms"] += ctx.dt_ms
            elapsed = bstate["elapsed_ms"]
            if elapsed >= duration_ms:
                return None
            for seg in timeline:
                if seg.start_ms <= elapsed < seg.end_ms:
                    return dict(seg.action)
            return {}

        def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
            return dict(on_end) if on_end else None

    _TomlBehavior.__name__ = f"TomlBehavior_{bname}"
    _TomlBehavior.__qualname__ = _TomlBehavior.__name__

    return behavior(name=bname, weight=bweight)(_TomlBehavior)


def load_toml_pack(pack_dir: str) -> int:
    """Scan *pack_dir* for ``.toml`` files and register each as a behavior.

    Args:
        pack_dir: Absolute or relative path to a pack directory.

    Returns:
        Number of behaviors successfully loaded.
    """
    count = 0
    if not os.path.isdir(pack_dir):
        _logger.warning("Behavior pack directory does not exist: %s", pack_dir)
        return count

    for fname in sorted(os.listdir(pack_dir)):
        if not fname.endswith(".toml"):
            continue
        fpath = os.path.join(pack_dir, fname)
        try:
            with open(fpath, "rb") as f:
                spec = tomllib.load(f)
            _build_behavior_class(spec)
            count += 1
            if _logger.isEnabledFor(logging.DEBUG):
                _logger.debug("Loaded TOML behavior '%s' from %s", spec.get("name"), fpath)
        except Exception:
            _logger.exception("Failed to load TOML behavior from %s", fpath)
    return count
