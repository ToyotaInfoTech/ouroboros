# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Hard brake behavior — sudden deceleration while moving.

Triggers when the vehicle speed exceeds a threshold.  Applies a strong
deceleration for a short burst (0.5-1.2 s), then restores normal mode.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior(weight=1.5)
class HardBrake(BehaviorBase):
    """Sudden deceleration event."""

    # Defaults (overridable via config params)
    DURATION_MS = 800
    DECEL = 8.0
    MIN_SPEED = 5.0

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Only activate when vehicle is moving above threshold."""
        decel = ctx.params.get("decel", self.DECEL)
        min_speed = ctx.params.get("min_speed", self.MIN_SPEED)
        return ctx.state.get("speed", 0) > min_speed and decel > 0

    def begin(self, ctx: BehaviorContext) -> dict:
        """Initialize elapsed timer."""
        return {"elapsed_ms": 0.0}

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        """Apply hard braking each tick until duration is reached."""
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        decel = ctx.params.get("decel", self.DECEL)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms
        return {
            "target_speed": 0.0,
            "target_decel": decel,
            "tag": self.name,
        }

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        """Restore normal driving parameters."""
        return {"restore": True}
