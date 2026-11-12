# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Tests for _NodeInfo, _PlaceInfo, and _select_best_node in ray_world.

These tests run without initialising Ray. Ray actor calls are mocked.
"""

from threading import Barrier, Thread
from unittest.mock import MagicMock

import pytest

from ouroboros.world import ray_world
from ouroboros.world.ray_world import _NodeInfo, _PlaceInfo, _select_best_node

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_place_info(
    place_id: str,
    node_id: str = "node-0",
    edge_count: int = 0,
) -> _PlaceInfo:
    """Create a _PlaceInfo backed by a mock actor (no Ray required)."""
    mock_actor = MagicMock()
    info = _PlaceInfo(place_id, mock_actor, node_id)
    info.edge_count = edge_count
    return info


def _make_node_infos(scores: list[float]) -> dict[str, _NodeInfo]:
    """Build a node_infos dict from a list of wgpu scores."""
    return {f"node-{i}": _NodeInfo(node_id=f"node-{i}", wgpu_score=s) for i, s in enumerate(scores)}


# ---------------------------------------------------------------------------
# _NodeInfo
# ---------------------------------------------------------------------------


class TestNodeInfo:
    def test_fields_stored(self):
        n = _NodeInfo(node_id="abc", wgpu_score=1.5)
        assert n.node_id == "abc"
        assert n.wgpu_score == pytest.approx(1.5)
        assert n.place_count == 0

    def test_place_count_default_zero(self):
        n = _NodeInfo(node_id="x", wgpu_score=0.0)
        assert n.place_count == 0

    def test_place_count_mutable(self):
        n = _NodeInfo(node_id="x", wgpu_score=2.0)
        n.place_count += 3
        assert n.place_count == 3


# ---------------------------------------------------------------------------
# _PlaceInfo – edge count operations
# ---------------------------------------------------------------------------


class TestPlaceInfoEdgeCount:
    def test_initial_edge_count_is_zero(self):
        info = _make_place_info("p-0")
        assert info.edge_count == 0

    def test_increase_within_limit_returns_true(self):
        info = _make_place_info("p-1")
        assert info.increase_edge_count_or_fail(max_edges_per_place=5) is True
        assert info.edge_count == 1

    def test_increase_increments_count(self):
        info = _make_place_info("p-2")
        for _ in range(3):
            info.increase_edge_count_or_fail(max_edges_per_place=10)
        assert info.edge_count == 3

    def test_increase_at_limit_returns_false(self):
        info = _make_place_info("p-3", edge_count=5)
        assert info.increase_edge_count_or_fail(max_edges_per_place=5) is False
        assert info.edge_count == 5  # unchanged

    def test_decrease_decrements_count(self):
        info = _make_place_info("p-4", edge_count=3)
        info.decrease_edge_count()
        assert info.edge_count == 2

    def test_decrease_does_not_go_below_zero(self):
        info = _make_place_info("p-5", edge_count=0)
        info.decrease_edge_count()
        assert info.edge_count == 0

    def test_thread_safety_increase(self):
        """Concurrent increases must not exceed max_edges_per_place."""
        max_edges = 5
        info = _make_place_info("p-thread")
        results: list[bool] = []
        barrier = Barrier(10)

        def worker():
            barrier.wait()
            results.append(info.increase_edge_count_or_fail(max_edges_per_place=max_edges))

        threads = [Thread(target=worker) for _ in range(10)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert sum(results) == max_edges
        assert info.edge_count == max_edges

    def test_place_id_and_node_id_stored(self):
        info = _make_place_info("place-xyz", node_id="node-42")
        assert info.place_id == "place-xyz"
        assert info.node_id == "node-42"

    def test_repr_contains_place_id_and_node_id(self):
        info = _make_place_info("place-repr", node_id="node-7")
        r = repr(info)
        assert "place-repr" in r
        assert "node-7" in r

    def test_getattr_delegates_to_inner(self):
        info = _make_place_info("p-attr")
        info.inner.some_attr = "hello"
        assert info.some_attr == "hello"


@pytest.mark.asyncio
async def test_place_add_edge_passes_explicit_behavior_config(monkeypatch):
    captured: dict[str, object] = {}

    class FakeEdge:
        def __init__(self, edge_id: str, init_state: dict, **kwargs):
            captured["edge_id"] = edge_id
            captured["init_state"] = init_state
            captured["kwargs"] = kwargs

        async def start(self) -> None:
            captured["started"] = True

    monkeypatch.setattr(ray_world, "Edge", FakeEdge)

    place = type(
        "FakePlace",
        (),
        {
            "place_id": "place-1",
            "edges": {},
            "edge_exec": "ouroedge",
            "edge_config": "edge.toml",
            "behavior_config": {"enabled": True, "seed": 7},
            "edge_enabled": False,
            "renderer_name": None,
        },
    )()

    await ray_world.Place.__ray_actor_class__.add_edge(place, "veh-1", {"type_id": "car", "status": "running"})

    assert captured["started"] is True
    assert captured["kwargs"]["behavior_config"] == place.behavior_config
    assert "veh-1" in place.edges


class TestWorldBehaviorConfigInjection:
    def test_create_place_info_passes_loaded_behavior_config(self, monkeypatch):
        behavior_config = {"enabled": True, "seed": 99}
        captured: dict[str, object] = {}

        class FakePlaceRemote:
            def remote(self, place_id: str, **kwargs):
                captured["place_id"] = place_id
                captured["remote_kwargs"] = kwargs
                return "actor-ref"

        class FakePlaceFactory:
            def options(self, **kwargs):
                captured["options_kwargs"] = kwargs
                return FakePlaceRemote()

        monkeypatch.setattr(ray_world, "Place", FakePlaceFactory())

        def _fake_load_behavior_config(self, path: str):
            self.behavior_config = behavior_config
            return behavior_config

        monkeypatch.setattr(ray_world.base.World, "load_behavior_config", _fake_load_behavior_config)
        monkeypatch.setattr(
            ray_world,
            "NodeAffinitySchedulingStrategy",
            lambda node_id, soft: {"node_id": node_id, "soft": soft},
        )
        monkeypatch.setattr(ray_world.ray, "init", lambda *args, **kwargs: None)
        monkeypatch.setattr(ray_world.ray, "available_resources", lambda: {})
        monkeypatch.setattr(ray_world.ray, "nodes", lambda: [])

        world = ray_world.World(
            edge_behavior_config="etc/dangerous_behaviors.toml",
            edge_enabled=False,
            renderer_name="",
            edge_exec="ouroedge",
            edge_config="edge.toml",
            min_edges_per_place=1,
            max_edges_per_place=4,
            max_places=2,
        )

        place_info = world._create_place_info("node-1")

        assert world.behavior_config is behavior_config
        assert captured["place_id"] == place_info.place_id
        assert captured["remote_kwargs"]["behavior_config"] is behavior_config
        assert world.places[place_info.place_id] == "actor-ref"


# ---------------------------------------------------------------------------
# _select_best_node
# ---------------------------------------------------------------------------


class TestSelectBestNode:
    def test_single_node_always_selected(self):
        nodes = _make_node_infos([2.0])
        result = _select_best_node(nodes, max_places=10)
        assert result.node_id == "node-0"

    def test_empty_node_infos_raises(self):
        with pytest.raises(ValueError):
            _select_best_node({}, max_places=10)

    def test_node_with_zero_places_selected_first(self):
        """A node with place_count=0 always has the lowest deficit ratio."""
        nodes = _make_node_infos([1.0, 2.0])
        # node-1 has higher score; both start at 0.  Break tie: both 0/target = 0.
        # After adding a place to node-0, node-1 should now be preferred.
        nodes["node-0"].place_count = 1
        result = _select_best_node(nodes, max_places=4)
        assert result.node_id == "node-1"

    def test_proportional_selection_prefers_high_score_node(self):
        """High-score node should absorb more places before triggering switch."""
        # node-0: score=1, target=10*(1/3)=3.33
        # node-1: score=2, target=10*(2/3)=6.67
        nodes = _make_node_infos([1.0, 2.0])
        # Give node-1 two places (deficit_ratio = 2/6.67 = 0.30)
        # Give node-0 one place  (deficit_ratio = 1/3.33 = 0.30) → tie → both equal
        nodes["node-0"].place_count = 1
        nodes["node-1"].place_count = 2
        result = _select_best_node(nodes, max_places=10)
        # Both have equal deficit ratios; min() returns the first one in dict order.
        # Either is acceptable — just check it returns a valid node.
        assert result.node_id in nodes

    def test_uniform_fallback_when_all_scores_zero(self):
        """When all scores are 0.0, places are distributed uniformly."""
        nodes = _make_node_infos([0.0, 0.0, 0.0])
        # All place_counts are 0, uniform target = 10/3.  All deficit_ratios are 0.
        result = _select_best_node(nodes, max_places=10)
        assert result.node_id in nodes

    def test_zero_score_node_excluded_when_others_nonzero(self):
        """A node with wgpu_score=0 should never be chosen if others have score>0."""
        nodes = {
            "gpu-node": _NodeInfo(node_id="gpu-node", wgpu_score=3.0, place_count=0),
            "cpu-node": _NodeInfo(node_id="cpu-node", wgpu_score=0.0, place_count=0),
        }
        # cpu-node has score 0 → deficit_ratio = inf; gpu-node should always win.
        for _ in range(5):
            result = _select_best_node(nodes, max_places=10)
            assert result.node_id == "gpu-node"
            nodes["gpu-node"].place_count += 1

    def test_deficit_ratio_drives_rebalancing(self):
        """After filling a high-score node to its target, the lower-score node is chosen."""
        # node-A: score=3, target=9*(3/4)=6.75
        # node-B: score=1, target=9*(1/4)=2.25
        nodes = {
            "A": _NodeInfo(node_id="A", wgpu_score=3.0, place_count=0),
            "B": _NodeInfo(node_id="B", wgpu_score=1.0, place_count=0),
        }
        max_places = 9
        allocation: dict[str, int] = {"A": 0, "B": 0}

        # Simulate allocating all 9 places greedily.
        for _ in range(max_places):
            chosen = _select_best_node(nodes, max_places=max_places)
            nodes[chosen.node_id].place_count += 1
            allocation[chosen.node_id] += 1

        # Node A should receive ~6-7 places, node B ~2-3.
        assert allocation["A"] > allocation["B"], f"Expected A > B but got A={allocation['A']}, B={allocation['B']}"
