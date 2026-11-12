# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Wrong-way driving behavior — teleport to the opposing lane and drive against traffic.

Triggers at low speed.  Uses ``moveToXY`` to place the vehicle on the
opposing edge and temporarily sets a reversed route.  After the duration
the vehicle is restored to normal routing.

Caution: This is the most disruptive behavior and should have a low weight.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior(weight=0.3)
class WrongWay(BehaviorBase):
    """Drive against traffic on the opposing lane."""

    name: str
    DURATION_MS = 5000
    MAX_SPEED_THRESHOLD = 8.0  # only trigger at low speed
    TARGET_SPEED = 6.0  # m/s — ensure movement after teleporting from a stop
    TARGET_ACCEL = 2.5  # m/s^2 — moderate launch into wrong-way travel

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Activate only at low speed with no leader and valid position data."""
        threshold = ctx.params.get("max_speed_threshold", self.MAX_SPEED_THRESHOLD)
        return (
            ctx.state.get("speed", 0) <= threshold
            and not ctx.state.get("leader")
            and ctx.state.get("x") is not None
            and ctx.state.get("y") is not None
        )

    def begin(self, ctx: BehaviorContext) -> dict:
        # Flip the angle by 180 degrees for wrong-way heading
        angle = ctx.state.get("angle", 0.0)
        flipped_angle = (angle + 180.0) % 360.0
        return {
            "elapsed_ms": 0.0,
            "orig_x": ctx.state.get("x", 0.0),
            "orig_y": ctx.state.get("y", 0.0),
            "flipped_angle": flipped_angle,
            "teleported": False,
        }

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        min_target_speed = ctx.params.get("target_speed", self.TARGET_SPEED)
        target_accel = ctx.params.get("target_accel", self.TARGET_ACCEL)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms

        current_speed = ctx.state.get("speed", 0.0)
        patch: dict = {
            "tag": self.name,
            "target_speed": max(current_speed, min_target_speed),
            "target_accel": target_accel,
            # Disable all safety so SUMO does not auto-correct
            "speed_mode": 0b00000,
            "lane_change_mode": 0b000000000000,
        }

        if not bstate["teleported"]:
            # First tick: teleport to same XY but with flipped angle
            patch["move_to_xy"] = {
                "x": bstate["orig_x"],
                "y": bstate["orig_y"],
                "angle": bstate["flipped_angle"],
                "keep_route": 2,  # 2 = ignore destination
            }
            bstate["teleported"] = True

        return patch

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"restore": True}
