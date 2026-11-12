# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Behavior injection framework for simulated edge vehicles.

This package provides a DSL-based system for injecting realistic abnormal
driving behaviors (hard braking, signal violations, lane departures, etc.)
into the simulation at a configurable probability.

Behaviors are defined as Python classes or declarative TOML files in
sub-packages called *packs* (e.g. ``pack_dangerous/``).  The runtime
evaluates one behavior at a time per edge, respecting cooldown windows
and per-vehicle seeded randomness for reproducibility.

Public API used by ``world.base.Edge``:

    from ouroboros.world.behavior import BehaviorRuntime, load_behavior_config

    load_behavior_config("etc/dangerous_behaviors.toml")  # once at startup
    runtime = BehaviorRuntime(edge_id)
"""

from __future__ import annotations

import logging
import os
import tomllib
from typing import Any

from .runtime import BehaviorRuntime

_logger = logging.getLogger(__name__)

# Module-level configuration shared by all BehaviorRuntime instances.
_config: dict[str, Any] = {"enabled": False}


def get_config() -> dict[str, Any]:
    """Return the current module-level behavior configuration."""
    return _config


def register_packs(config: dict[str, Any]) -> None:
    """Register behavior packs declared in *config* into the global REGISTRY.

    This is separated from :func:`load_behavior_config` so that worker
    processes (e.g. Ray actors) can populate the registry from a config dict
    that was serialized by the driver, without needing access to the original
    file on disk.

    Also updates the module-level ``_config`` so that :func:`get_config`
    returns a consistent value inside the worker.

    Args:
        config: Parsed behavior configuration dict, typically the value
            returned by :func:`load_behavior_config` in the driver process.
    """
    global _config
    _config = config

    packs = config.get("packs", [])
    for pack_name in packs:
        if pack_name == "pack_dangerous":
            # Import triggers the @behavior decorators
            from . import pack_dangerous as _  # noqa: F401

            _logger.info(f"Registered Python behavior pack: {pack_name}")
        else:
            # Try loading as a TOML pack directory
            from .toml_loader import load_toml_pack

            pack_dir = os.path.join(os.path.dirname(__file__), pack_name)
            n = load_toml_pack(pack_dir)
            _logger.info(f"Loaded {n} TOML behaviors from pack: {pack_name}")


def load_behavior_config(path: str | None) -> dict[str, Any]:
    """Load behavior configuration from a TOML file and register packs.

    This should be called once at startup (e.g. in ``World.__init__``).
    Subsequent calls overwrite the previous configuration.

    Args:
        path: Path to the behavior configuration file.

    Returns:
        The parsed configuration dict.
    """
    global _config

    if not path:
        _logger.info("Behavior config is not specified; behaviors disabled.")
        _config = {"enabled": False}
        return _config

    if not os.path.isfile(path):
        _logger.error(f"Behavior config not found at '{path}', exiting.")
        exit(-1)

    with open(path, "rb") as f:
        _config = tomllib.load(f)

    _logger.info(f"Loaded behavior config from {path} (enabled={_config.get('enabled', False)}).")

    register_packs(_config)

    return _config


__all__ = ["BehaviorRuntime", "get_config", "load_behavior_config", "register_packs"]
