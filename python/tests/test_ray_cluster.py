# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

import pytest
import ray

from ouroboros.cluster import ray_cluster


@pytest.mark.asyncio
async def test_edge():
    edge = ray_cluster.EdgeLiaison("edge-1", {"position": (0, 0), "speed": 0.0})
    await edge.start()
    result = await edge.step({"position": (1, 1), "speed": 5.0})
    print(f"Edge step result: {result}")


@pytest.mark.asyncio
async def test_place():
    ray.init(
        runtime_env={
            "env_vars": {
                "PYTHONPATH": "python"  # or absolute path
            }
        }
    )
    place = ray_cluster.Place.remote("place-1")
    ray.get(place.add_edge.remote("edge-1", {"position": (0, 0), "speed": 0.0}))

    for _ in range(3):
        updates = {"edge-1": {"position": (2, 2), "speed": 10.0}}
        actions = ray.get(place.step.remote(updates))
        print(f"Place step actions: {actions}")
