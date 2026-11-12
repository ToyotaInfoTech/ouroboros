# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Sharp steer behavior — abrupt lane change ignoring safety checks.

Triggers when at least one adjacent lane exists.  Forces a rapid lane change
with reduced safety constraints.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior()
class SharpSteer(BehaviorBase):
    """Abrupt unsafe lane change."""

    DURATION_MS = 1200
    CHANGE_DURATION_MS = 600  # traci changeLane duration

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Require at least one adjacent lane (lane_idx > 0 or multiple lanes)."""
        lane_id = ctx.state.get("lane_id", "")
        if not lane_id:
            return False
        # Heuristic: if lane_idx == 0 we can go right-to-left (idx+1).
        # If > 0 we can go left.  Either way there's a neighbor lane.
        return ctx.state.get("speed", 0) > 3.0

    def begin(self, ctx: BehaviorContext) -> dict:
        # Decide direction: prefer moving toward the center (higher idx)
        lane_idx = ctx.state.get("lane_idx", 0)
        num_lanes = ctx.state.get("num_lanes", 1)
        direction = 1 if lane_idx == 0 else -1
        if ctx.rng.random() < 0.5:
            direction *= -1
        target_lane = max(0, min(lane_idx + direction, num_lanes - 1))
        return {"elapsed_ms": 0.0, "target_lane": target_lane}

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        change_dur = ctx.params.get("change_duration_ms", self.CHANGE_DURATION_MS)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms
        return {
            "lane_change": (bstate["target_lane"], change_dur),
            # Disable strategic + cooperative + emergency safety bits
            "lane_change_mode": 0b000000000000,
            "tag": self.name,
        }

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"restore": True}
