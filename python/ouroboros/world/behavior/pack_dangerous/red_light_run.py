# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Red-light run behavior — ignore an upcoming red traffic signal.

Triggers when the vehicle is approaching a red traffic light within a
configurable distance.  Temporarily disables the traffic-light compliance
bit in SUMO's speed mode.
"""

from ..registry import BehaviorBase, BehaviorContext, behavior


@behavior(weight=1.2)
class RedLightRun(BehaviorBase):
    """Intentional red-light violation."""

    name: str
    DURATION_MS = 2000
    MAX_DIST = 30.0  # meters — distance to TLS for activation
    TARGET_SPEED = 8.0  # m/s — enough to enter the intersection from a stop
    TARGET_ACCEL = 3.0  # m/s^2 — moderate launch for stopped vehicles
    LANE_CHANGE_DURATION_MS = 800  # traci changeLane duration for overtake

    def applicable(self, ctx: BehaviorContext) -> bool:
        """Require an upcoming red signal within range.

        Additionally, the path ahead must be clear: either no leader
        vehicle, or an adjacent lane is free for overtaking.
        """
        max_dist = ctx.params.get("max_dist", self.MAX_DIST)
        has_red = False
        for tls in ctx.state.get("next_tls", []):
            signal = tls.get("signal", "")
            dist = tls.get("dist", float("inf"))
            if "r" in signal.lower() and dist < max_dist:
                has_red = True
                break
        if not has_red:
            return False

        leader = ctx.state.get("leader")
        if not leader or (isinstance(leader, (tuple, list)) and not leader[0]):
            return True  # no leader — free to go

        # Leader present — check if an adjacent lane is free for overtaking
        lane_idx = ctx.state.get("lane_idx", -1)
        num_lanes = ctx.state.get("num_lanes", 1)
        if num_lanes <= 1:
            return False  # single lane, cannot overtake

        # Candidate overtake lanes (prefer higher idx = further from curb)
        candidates = []
        if lane_idx + 1 < num_lanes:
            candidates.append(lane_idx + 1)
        if lane_idx - 1 >= 0:
            candidates.append(lane_idx - 1)
        if not candidates:
            return False

        # Collect lane indices occupied by nearby vehicles
        occupied = set()
        for nv in ctx.state.get("neighbor_vehicles", []):
            nv_lane = nv.get("lane_idx")
            if nv_lane is not None:
                occupied.add(nv_lane)

        return any(c not in occupied for c in candidates)

    def begin(self, ctx: BehaviorContext) -> dict:
        # Determine if we need to overtake a leader via lane change
        leader = ctx.state.get("leader")
        has_leader = bool(leader) and (not isinstance(leader, (tuple, list)) or bool(leader[0]))
        overtake_lane: int | None = None
        if has_leader:
            lane_idx = ctx.state.get("lane_idx", 0)
            num_lanes = ctx.state.get("num_lanes", 1)
            if num_lanes > 1:
                # Pick the first free adjacent lane
                occupied = set()
                for nv in ctx.state.get("neighbor_vehicles", []):
                    nv_lane = nv.get("lane_idx")
                    if nv_lane is not None:
                        occupied.add(nv_lane)
                # Prefer higher idx (further from curb)
                candidates = []
                if lane_idx + 1 < num_lanes:
                    candidates.append(lane_idx + 1)
                if lane_idx - 1 >= 0:
                    candidates.append(lane_idx - 1)
                for c in candidates:
                    if c not in occupied:
                        overtake_lane = c
                        break
        return {
            "elapsed_ms": 0.0,
            "has_leader": has_leader,
            "overtake_lane": overtake_lane,
        }

    def tick(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        duration_ms = ctx.params.get("duration_ms", self.DURATION_MS)
        target_speed = ctx.params.get("target_speed", self.TARGET_SPEED)
        target_accel = ctx.params.get("target_accel", self.TARGET_ACCEL)
        if bstate["elapsed_ms"] >= duration_ms:
            return None
        bstate["elapsed_ms"] += ctx.dt_ms

        patch: dict = {
            # When the vehicle has already stopped at the signal, disabling
            # TLS compliance alone is insufficient; request forward motion.
            "target_speed": target_speed,
            "target_accel": target_accel,
            # Disable TLS compliance (bit 3) AND red-light hard-brake (bit 4):
            # default speed_mode = 0b11111 (31); clearing bits 3+4 → 0b00111 (7)
            "speed_mode": 0b00111,
            "tag": self.name,
        }

        # Overtake the leader with a lane change if needed
        if bstate["has_leader"] and bstate["overtake_lane"] is not None:
            change_dur = ctx.params.get("lane_change_duration_ms", self.LANE_CHANGE_DURATION_MS)
            patch["lane_change"] = (bstate["overtake_lane"], change_dur)
            # Disable all lane-change safety so SUMO does not block the move
            patch["lane_change_mode"] = 0b000000000000

        return patch

    def end(self, bstate: dict, ctx: BehaviorContext) -> dict | None:
        return {"restore": True}
