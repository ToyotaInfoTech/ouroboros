# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Dangerous driving behavior pack.

Importing this module auto-registers all Python-defined behaviors in the
global registry via the ``@behavior`` decorator.
"""

from . import (
    hard_brake,
    jerk_accel,
    lane_depart,
    minor_collision,
    red_light_run,
    sharp_steer,
    wrong_way,
)

__all__ = [
    "hard_brake",
    "jerk_accel",
    "lane_depart",
    "minor_collision",
    "red_light_run",
    "sharp_steer",
    "wrong_way",
]
