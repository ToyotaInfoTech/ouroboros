# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Focused tests for behavior-config injection in local_world."""

import pytest

from ouroboros.world import local_world


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

    monkeypatch.setattr(local_world, "Edge", FakeEdge)

    place = local_world.Place(
        "place-1",
        edge_exec="ouroedge",
        edge_config="edge.toml",
        behavior_config={"enabled": True, "seed": 11},
        edge_enabled=False,
    )

    await place.add_edge("veh-1", {"type_id": "car", "status": "running"})

    assert captured["started"] is True
    assert captured["kwargs"]["behavior_config"] == place.behavior_config
    assert "veh-1" in place.edges


def test_create_place_info_passes_loaded_behavior_config(monkeypatch):
    behavior_config = {"enabled": True, "seed": 21}

    class FakePlace:
        def __init__(self, place_id: str, **kwargs):
            self.place_id = place_id
            self.behavior_config = kwargs["behavior_config"]
            self.kwargs = kwargs

    monkeypatch.setattr(local_world, "Place", FakePlace)

    def _fake_load_behavior_config(self, path: str):
        self.behavior_config = behavior_config
        return behavior_config

    monkeypatch.setattr(local_world.base.World, "load_behavior_config", _fake_load_behavior_config)

    world = local_world.World(
        edge_behavior_config="etc/dangerous_behaviors.toml",
        edge_enabled=False,
        renderer_name=None,
        edge_exec="ouroedge",
        edge_config="edge.toml",
        min_edges_per_place=1,
        max_edges_per_place=4,
        max_places=2,
    )

    place_info = world._create_place_info()

    assert world.behavior_config is behavior_config
    assert place_info.place.behavior_config is behavior_config
