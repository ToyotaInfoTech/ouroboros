# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Minor collision behavior — tailgate the leader vehicle to provoke a low-speed bump.

Triggers when a leader vehicle is within a short gap and relative speed is
small.  Disables the collision-avoidance bit in SUMO's speed mode and sets
the ego vehicle speed slightly above the leader's, inducing a minor
rear-end contact.  SUMO continues the simulation after contact (vehicles
are not removed).
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior(weight=0.5)
class MinorCollision(BehaviorBase):
    """Low-speed rear-end collision provocation."""

    DURATION_MS = 1500
    MAX_LEADER_GAP = 10.0  # meters
    MAX_RELATIVE_SPEED = 5.0  # m/s
    SPEED_BOOST = 3.0  # m/s above leader speed

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Require a close leader with small speed difference.

        The ``leader`` field from SUMO is a tuple ``(leader_id, gap)``
        or the fallback ``("", -1.0)``.
        """
        leader = ctx.state.get("leader")
        if not leader:
            return False
        # leader is (vin: str, gap: float)
        leader_vin, gap = leader
        if not leader_vin:
            return False
        max_gap = ctx.params.get("max_leader_gap", self.MAX_LEADER_GAP)
        if gap > max_gap or gap < 0:
            return False
        return ctx.state.get("speed", 0) > 2.0

    def begin(self, ctx: BehaviorContext) -> dict:
        return {"elapsed_ms": 0.0}

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        speed_boost = ctx.params.get("speed_boost", self.SPEED_BOOST)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms

        current_speed = ctx.state.get("speed", 0)
        return {
            "target_speed": current_speed + speed_boost,
            # Disable collision avoidance (bit 1)
            "speed_mode": 0b11101,
            "tag": self.name,
        }

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"restore": True}
