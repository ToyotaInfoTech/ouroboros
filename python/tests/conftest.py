# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Pytest configuration for conditional test collection.

Tests that depend on generated FlatBuffers code (ouroboros.gen) or on
optional modules (ouroboros.cluster / Ray) are skipped when those
modules are not available in the current environment.
"""

import importlib.util

collect_ignore: list[str] = []

if importlib.util.find_spec("ouroboros.gen") is None:
    collect_ignore += ["test_fb_serde.py", "test_rpc_client.py"]

if importlib.util.find_spec("ouroboros.cluster") is None:
    collect_ignore.append("test_ray_cluster.py")
