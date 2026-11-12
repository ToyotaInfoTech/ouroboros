# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

import ray

ray.init()

resources = ray.available_resources()
print(resources)
nodes = ray.nodes()
for node in nodes:
    print("Node:", node["NodeManagerAddress"])
    print("Resources total:", node["Resources"])
