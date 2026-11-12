# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Tests for the dangerous-driving behavior framework.

Covers the registry, runtime lifecycle, TOML loader, and individual behavior
applicability / tick / end contracts.
"""

from __future__ import annotations

import random
from typing import Any

import pytest

from ouroboros.world.behavior.registry import (
    REGISTRY,
    BehaviorContext,
    behavior,
    get_registered_names,
)
from ouroboros.world.behavior.runtime import BehaviorRuntime as _RT

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_state(**overrides: Any) -> dict[str, Any]:
    """Build a minimal SUMO-like state dict."""
    base = {
        "vin": "veh_0",
        "status": "running",
        "speed": 10.0,
        "accel": 0.0,
        "angle": 90.0,
        "lane_id": "E0_0",
        "lane_idx": 0,
        "lane_pos": 50.0,
        "road_id": "E0",
        "x": 100.0,
        "y": 200.0,
        "lon": 0.0,
        "lat": 0.0,
        "leader": ("veh_1", 5.0),
        "next_tls": [{"id": "tls_0", "idx": 0, "dist": 20.0, "signal": "r"}],
        "neighbor_vehicles": [],
        "type_id": "car",
    }
    base.update(overrides)
    return base


def _enabled_config(**overrides: Any) -> dict[str, Any]:
    """Return a config dict with behaviors enabled at 100% for deterministic tests."""
    cfg: dict[str, Any] = {
        "enabled": True,
        "probability": 1000.0,  # guarantee activation every tick
        "cooldown_sec": 0,
        "seed": 12345,
        "enabled_kinds": [
            "hard_brake",
            "jerk_accel",
            "sharp_steer",
            "lane_depart",
            "red_light_run",
            "wrong_way",
            "minor_collision",
        ],
        "params": {},
    }
    cfg.update(overrides)
    return cfg


# ---------------------------------------------------------------------------
# Ensure pack is registered
# ---------------------------------------------------------------------------


@pytest.fixture(autouse=True, scope="module")
def _register_pack():
    """Import the pack so @behavior decorators fire."""
    from ouroboros.world.behavior import pack_dangerous as _  # noqa: F401


# ---------------------------------------------------------------------------
# Registry tests
# ---------------------------------------------------------------------------


class TestRegistry:
    def test_all_7_behaviors_registered(self):
        expected = {
            "hard_brake",
            "jerk_accel",
            "sharp_steer",
            "lane_depart",
            "red_light_run",
            "wrong_way",
            "minor_collision",
        }
        assert expected.issubset(set(get_registered_names()))

    def test_custom_behavior_registration(self):
        @behavior(name="_test_dummy", weight=0.5)
        class _Dummy:
            def applicable(self, ctx):
                return True

            def begin(self, ctx):
                return {}

            def tick(self, bstate, ctx):
                return None

            def end(self, bstate, ctx):
                return None

        assert "_test_dummy" in REGISTRY
        entry = REGISTRY["_test_dummy"]
        assert entry.weight == 0.5
        # cleanup
        del REGISTRY["_test_dummy"]


# ---------------------------------------------------------------------------
# Runtime tests
# ---------------------------------------------------------------------------


class TestRuntime:
    def test_disabled_returns_empty(self):
        rt = _RT("edge_0", {"enabled": False})
        patch = rt.step(_make_state(), dt_sec=1.0)
        assert patch == {}

    def test_activation_produces_action(self):
        """With probability=1000 and cooldown=0, a behavior must fire."""
        cfg = _enabled_config()
        rt = _RT("edge_test", cfg)
        state = _make_state(speed=15.0)
        patch = rt.step(state, dt_sec=1.0)
        assert patch  # non-empty
        assert "tag" in patch  # every behavior sets a tag

    def test_cooldown_prevents_immediate_reactivation(self):
        cfg = _enabled_config(cooldown_sec=9999)
        rt = _RT("edge_cd", cfg)
        state = _make_state(speed=15.0)

        # Activate and run until end
        for _ in range(5000):
            patch = rt.step(state, dt_sec=0.001)
            if patch.get("restore"):
                break

        # Now in cooldown — should return empty
        patch = rt.step(state, dt_sec=0.001)
        assert patch == {}

    def test_end_returns_restore(self):
        """Drive a behavior to completion and verify restore is emitted."""
        cfg = _enabled_config(
            enabled_kinds=["hard_brake"],
            params={"hard_brake": {"duration_ms": 10}},
        )
        rt = _RT("edge_end", cfg)
        state = _make_state(speed=15.0)

        restore_seen = False
        for _ in range(200):
            patch = rt.step(state, dt_sec=1.0)
            if patch.get("restore"):
                restore_seen = True
                break
        assert restore_seen


# ---------------------------------------------------------------------------
# Individual behavior applicability tests
# ---------------------------------------------------------------------------


class TestBehaviorApplicability:
    def _ctx(self, state: dict) -> BehaviorContext:
        return BehaviorContext(
            edge_id="test",
            state=state,
            dt_sec=1.0,
            dt_ms=1000.0,
            rng=random.Random(0),
        )

    def test_hard_brake_requires_speed(self):
        inst = REGISTRY["hard_brake"].cls()
        assert inst.applicable(self._ctx(_make_state(speed=10.0)))
        assert not inst.applicable(self._ctx(_make_state(speed=1.0)))

    def test_jerk_accel_requires_low_speed(self):
        inst = REGISTRY["jerk_accel"].cls()
        # Needs low speed, no leader, and an upcoming green TLS.
        green_tls = [{"id": "t", "idx": 0, "dist": 10, "signal": "g"}]
        assert inst.applicable(self._ctx(_make_state(speed=0.5, leader=None, next_tls=green_tls)))
        assert not inst.applicable(self._ctx(_make_state(speed=10.0, leader=None, next_tls=green_tls)))

    def test_jerk_accel_blocked_by_leader(self):
        inst = REGISTRY["jerk_accel"].cls()
        green_tls = [{"id": "t", "idx": 0, "dist": 10, "signal": "g"}]
        assert not inst.applicable(self._ctx(_make_state(speed=0.5, leader=("veh_1", 5.0), next_tls=green_tls)))

    def test_jerk_accel_blocked_by_non_green_tls(self):
        inst = REGISTRY["jerk_accel"].cls()
        assert not inst.applicable(
            self._ctx(_make_state(speed=0.5, leader=None, next_tls=[{"id": "t", "idx": 0, "dist": 10, "signal": "r"}]))
        )
        assert not inst.applicable(self._ctx(_make_state(speed=0.5, leader=None, next_tls=[])))

    def test_red_light_run_requires_red_tls(self):
        inst = REGISTRY["red_light_run"].cls()
        # Red signal within range — free adjacent lane available (num_lanes=2, no neighbors)
        assert inst.applicable(
            self._ctx(_make_state(next_tls=[{"id": "t", "idx": 0, "dist": 15, "signal": "r"}], num_lanes=2))
        )
        assert not inst.applicable(self._ctx(_make_state(next_tls=[{"id": "t", "idx": 0, "dist": 15, "signal": "g"}])))
        assert not inst.applicable(self._ctx(_make_state(next_tls=[])))

    def test_red_light_run_requests_launch_from_stop(self):
        inst = REGISTRY["red_light_run"].cls()
        ctx = self._ctx(_make_state(speed=0.0, next_tls=[{"id": "t", "idx": 0, "dist": 5, "signal": "r"}]))

        patch = inst.tick(inst.begin(ctx), ctx)

        assert patch is not None
        assert patch["speed_mode"] == 0b00111
        assert patch["target_speed"] > 0.0
        assert patch["target_accel"] > 0.0

    def test_wrong_way_requires_low_speed_and_position(self):
        inst = REGISTRY["wrong_way"].cls()
        assert inst.applicable(self._ctx(_make_state(speed=3.0, x=10, y=20, leader=None)))
        assert not inst.applicable(self._ctx(_make_state(speed=20.0, leader=None)))
        assert not inst.applicable(self._ctx(_make_state(speed=3.0, x=10, y=20, leader=("veh_1", 5.0))))

    def test_wrong_way_requests_motion_from_stop(self):
        inst = REGISTRY["wrong_way"].cls()
        ctx = self._ctx(_make_state(speed=0.0, x=10.0, y=20.0))

        patch = inst.tick(inst.begin(ctx), ctx)

        assert patch is not None
        assert patch["speed_mode"] == 0b00000
        assert patch["lane_change_mode"] == 0b000000000000
        assert patch["move_to_xy"]["angle"] == (ctx.state["angle"] + 180.0) % 360.0
        assert patch["target_speed"] > 0.0
        assert patch["target_accel"] > 0.0

    def test_minor_collision_requires_leader(self):
        inst = REGISTRY["minor_collision"].cls()
        assert inst.applicable(self._ctx(_make_state(leader=("l", 5.0), speed=8.0)))
        assert not inst.applicable(self._ctx(_make_state(leader=None, speed=8.0)))
        assert not inst.applicable(self._ctx(_make_state(leader=("", -1.0), speed=8.0)))


# ---------------------------------------------------------------------------
# TOML loader tests
# ---------------------------------------------------------------------------


class TestTomlLoader:
    def test_load_nonexistent_dir(self):
        from ouroboros.world.behavior.toml_loader import load_toml_pack

        count = load_toml_pack("/nonexistent/path")
        assert count == 0

    def test_load_toml_behavior(self, tmp_path):
        from ouroboros.world.behavior.toml_loader import load_toml_pack

        toml_content = b"""
name = "test_toml_beh"
weight = 2.0
applicable = "state.get('speed', 0) > 1"
duration_ms = 100

[[timeline]]
at = [0, 100]

[timeline.action]
tag = "test_toml_beh"
target_speed = 0.0
"""
        (tmp_path / "test_beh.toml").write_bytes(toml_content)
        count = load_toml_pack(str(tmp_path))
        assert count == 1
        assert "test_toml_beh" in REGISTRY

        # Verify the loaded behavior works
        inst = REGISTRY["test_toml_beh"].cls()
        ctx = BehaviorContext(
            edge_id="t",
            state={"speed": 5.0},
            dt_sec=0.01,
            dt_ms=10.0,
            rng=random.Random(0),
        )
        assert inst.applicable(ctx)
        bstate = inst.begin(ctx)
        patch = inst.tick(bstate, ctx)
        assert patch is not None
        assert patch.get("tag") == "test_toml_beh"

        # cleanup
        del REGISTRY["test_toml_beh"]


# ---------------------------------------------------------------------------
# register_packs — worker-process registration from a config dict
# ---------------------------------------------------------------------------


class TestRegisterPacks:
    """Verify that register_packs() populates REGISTRY from a config dict.

    These tests simulate what happens in a Ray worker process: the driver
    serialises ``behavior_config`` and the worker receives it as a plain dict
    (no file access needed).  Calling ``register_packs`` must populate
    ``REGISTRY`` so that ``BehaviorRuntime._get_candidates`` can find entries.
    """

    def test_register_packs_populates_registry(self):
        """register_packs() with pack_dangerous registers all 7 behaviors."""
        from ouroboros.world.behavior import get_config, register_packs

        config = {
            "enabled": True,
            "probability": 1.0,
            "cooldown_sec": 0,
            "seed": 0,
            "enabled_kinds": ["hard_brake"],
            "packs": ["pack_dangerous"],
            "params": {},
        }
        register_packs(config)

        assert "hard_brake" in REGISTRY
        assert "jerk_accel" in REGISTRY
        # Module-level _config should also reflect the supplied dict
        assert get_config() is config

    def test_register_packs_no_packs_key(self):
        """register_packs() with no 'packs' key does not raise and updates _config."""
        from ouroboros.world.behavior import get_config, register_packs

        config = {"enabled": False}
        register_packs(config)
        assert get_config() is config

    def test_behavior_runtime_activates_after_register_packs(self):
        """BehaviorRuntime activates behaviors after register_packs() is called."""
        from ouroboros.world.behavior import register_packs

        config = {
            "enabled": True,
            "probability": 1000.0,
            "cooldown_sec": 0,
            "seed": 0,
            "enabled_kinds": ["hard_brake"],
            "packs": ["pack_dangerous"],
            "params": {},
        }
        register_packs(config)

        rt = _RT("test_edge", config)
        state = _make_state(speed=15.0)
        patch = rt.step(state, dt_sec=1.0)
        # hard_brake should activate and produce a non-empty patch
        assert patch, f"Expected non-empty patch after register_packs, got: {patch}"

    def test_base_place_init_registers_packs(self, monkeypatch):
        """base.Place.__init__ calls register_packs when explicit behavior_config is given."""
        import ouroboros.world.behavior as _beh_module
        from ouroboros.world import base

        calls: list[dict] = []

        def _fake_register_packs(config):
            calls.append(config)

        monkeypatch.setattr(_beh_module, "register_packs", _fake_register_packs)
        # Patch the name imported into base so that the patched version is used
        monkeypatch.setattr(base, "register_packs", _fake_register_packs)

        class _FakePlace(base.Place):
            async def add_edge(self, edge_id, init_state):
                pass

        config = {
            "enabled": True,
            "probability": 1.0,
            "cooldown_sec": 0,
            "seed": 0,
            "enabled_kinds": ["hard_brake"],
            "packs": ["pack_dangerous"],
            "params": {},
        }
        _FakePlace("place_test", behavior_config=config)
        assert len(calls) == 1, "register_packs should be called exactly once"
        assert calls[0] is config

    def test_base_place_init_none_config_does_not_call_register_packs(self, monkeypatch):
        """base.Place.__init__ with behavior_config=None does NOT call register_packs."""
        from ouroboros.world import base

        calls: list = []

        monkeypatch.setattr(base, "register_packs", lambda cfg: calls.append(cfg))

        class _FakePlace(base.Place):
            async def add_edge(self, edge_id, init_state):
                pass

        _FakePlace("place_no_config", behavior_config=None)
        assert calls == [], "register_packs must NOT be called when behavior_config is None"
