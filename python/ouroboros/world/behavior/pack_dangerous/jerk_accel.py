# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Jerk acceleration behavior — sudden burst of speed from low speed / standstill.

Triggers when vehicle is nearly stopped (e.g. at a traffic light).  Temporarily
raises max speed and sets high acceleration for a short period.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior()
class JerkAccel(BehaviorBase):
    """Sudden aggressive acceleration event."""

    DURATION_MS = 2400
    ACCEL = 6.0
    BOOST_SPEED = 30.0  # m/s — temporary max speed override
    MAX_SPEED_THRESHOLD = 2.0  # only trigger when nearly stopped

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Only activate at low speed (near standstill)."""
        threshold = ctx.params.get("max_speed_threshold", self.MAX_SPEED_THRESHOLD)
        has_green_tls = any("g" in str(tls.get("signal", "")).lower() for tls in ctx.state.get("next_tls", []))
        return ctx.state.get("speed", 0) <= threshold and not ctx.state.get("leader") and has_green_tls

    def begin(self, ctx: BehaviorContext) -> dict:
        return {"elapsed_ms": 0.0}

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        accel = ctx.params.get("accel", self.ACCEL)
        boost_speed = ctx.params.get("boost_speed", self.BOOST_SPEED)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms
        return {
            "target_speed": boost_speed,
            "target_accel": accel,
            "max_speed": boost_speed,
            # Disable safe-speed check (bit 0)
            "speed_mode": 0b11110,
            "tag": self.name,
        }

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"restore": True}
