# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Lane departure behavior — gradual lateral drift within the lane.

Requires SUMO sublane model (``--lateral-resolution``).  Moves the vehicle
sideways by a configurable offset over a short period, then returns to center.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior(weight=0.8)
class LaneDepart(BehaviorBase):
    """Gradual lateral drift simulating inattention or drowsiness."""

    DRIFT_PHASE_MS = 1600
    RETURN_PHASE_MS = 1000
    MAX_LAT_OFFSET = 2.0  # meters
    MIN_LEADER_GAP = 10.0  # meters

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Activate at moderate speed on a valid lane with enough headway."""
        leader = ctx.state.get("leader")
        min_gap = ctx.params.get("min_leader_gap", self.MIN_LEADER_GAP)

        if not leader:
            leader_ok = True
        elif isinstance(leader, (tuple, list)) and len(leader) >= 2:
            # SUMO style: (leader_id, gap)
            leader_ok = leader[1] >= min_gap
        elif isinstance(leader, dict):
            # Alternate style: {"vin": ..., "dist": ...}
            leader_ok = leader.get("dist", float("inf")) >= min_gap
        else:
            leader_ok = False

        return ctx.state.get("speed", 0) > 5.0 and bool(ctx.state.get("lane_id")) and leader_ok

    def begin(self, ctx: BehaviorContext) -> dict:
        max_offset = ctx.params.get("max_lat_offset", self.MAX_LAT_OFFSET)
        direction = ctx.rng.choice([-1, 1])
        target_offset = direction * ctx.rng.uniform(0.3, max_offset)
        return {"elapsed_ms": 0.0, "target_offset": target_offset, "phase": "drift"}

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        drift_ms = ctx.params.get("drift_phase_ms", self.DRIFT_PHASE_MS)
        return_ms = ctx.params.get("return_phase_ms", self.RETURN_PHASE_MS)

        if bstate["phase"] == "drift":
            if bstate["elapsed_ms"] >= drift_ms:
                bstate["phase"] = "return"
                bstate["elapsed_ms"] = 0.0
                return {"sublane_lat": 0.0, "tag": self.name}
            bstate["elapsed_ms"] += ctx.dt_ms
            progress = min(bstate["elapsed_ms"] / drift_ms, 1.0)
            lat = bstate["target_offset"] * progress
            return {
                "sublane_lat": lat,
                "color": (128, 0, 255, 255),
                "tag": self.name,
            }

        # return phase — drift back to center
        if bstate["elapsed_ms"] >= return_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms
        progress = min(bstate["elapsed_ms"] / return_ms, 1.0)
        lat = bstate["target_offset"] * (1.0 - progress)
        return {
            "sublane_lat": lat,
            "tag": self.name,
        }

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"sublane_lat": 0.0, "restore": True}
