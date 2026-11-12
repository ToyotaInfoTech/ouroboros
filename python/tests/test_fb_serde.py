# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Tests for ouroboros.message.fb_serde module."""

import pytest

from ouroboros.message.fb_serde import _snake2camel, fb_deserialize, fb_serialize


class TestSnake2Camel:
    """Tests for the _snake2camel helper."""

    def test_single_word(self):
        assert _snake2camel("speed") == "Speed"

    def test_two_words(self):
        assert _snake2camel("lane_id") == "LaneId"

    def test_multiple_words(self):
        assert _snake2camel("next_tls_signal") == "NextTlsSignal"

    def test_already_single(self):
        assert _snake2camel("vin") == "Vin"

    def test_empty_string(self):
        assert _snake2camel("") == ""


def _make_sample_state() -> dict:
    """Create a minimal valid state dict for serialization tests."""
    return {
        "vin": "VIN001",
        "ts": 1000,
        "leader": ("VIN002", 15.5),
        "x": 100.0,
        "y": 200.0,
        "lon": 136.9,
        "lat": 35.1,
        "angle": 45.0,
        "speed": 10.0,
        "accel": 1.5,
        "lane_id": "lane_0",
        "lane_idx": 0,
        "lane_pos": 50.0,
        "road_id": "road_0",
        "wait_time": 0.0,
        "dist": 500.0,
        "fuel_cons": 0.5,
        "co2": 0.1,
        "co": 0.01,
        "nox": 0.001,
        "pmx": 0.0001,
        "noise": 60.0,
        "type_id": "car",
        "next_tls": [
            {"id": "tls_0", "idx": 0, "dist": 100.0, "signal": "G"},
        ],
        "neighbor_vehicles": [
            {
                "vin": "VIN003",
                "x": 110.0,
                "y": 210.0,
                "lon": 136.91,
                "lat": 35.11,
                "speed": 8.0,
                "accel": 0.5,
                "road_id": "road_0",
                "lane_id": "lane_0",
                "lane_idx": 0,
                "angle": 44.0,
            },
        ],
    }


class TestFbSerdeRoundTrip:
    """Tests for FlatBuffer serialize/deserialize round-trip."""

    def test_round_trip_basic(self):
        """Serialize then deserialize should reproduce the original state."""
        state = _make_sample_state()
        data = fb_serialize(state)
        assert isinstance(data, (bytes, bytearray))
        assert len(data) > 0

        result = fb_deserialize(data)

        # Scalar fields
        assert result["vin"] == b"VIN001"  # FlatBuffers returns bytes for strings
        assert result["ts"] == 1000
        assert result["speed"] == pytest.approx(10.0)
        assert result["accel"] == pytest.approx(1.5)
        assert result["lon"] == pytest.approx(136.9)
        assert result["lat"] == pytest.approx(35.1)

    def test_round_trip_leader(self):
        """Leader field should round-trip correctly as (vin, dist) tuple."""
        state = _make_sample_state()
        data = fb_serialize(state)
        result = fb_deserialize(data)

        leader_vin, leader_dist = result["leader"]
        assert leader_vin == b"VIN002"
        assert leader_dist == pytest.approx(15.5)

    def test_round_trip_next_tls(self):
        """next_tls list should round-trip correctly."""
        state = _make_sample_state()
        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert len(result["next_tls"]) == 1
        tls = result["next_tls"][0]
        assert tls["id"] == b"tls_0"
        assert tls["idx"] == 0
        assert tls["dist"] == pytest.approx(100.0)
        assert tls["signal"] == b"G"

    def test_round_trip_neighbor_vehicles(self):
        """neighbor_vehicles list should round-trip correctly."""
        state = _make_sample_state()
        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert len(result["neighbor_vehicles"]) == 1
        nv = result["neighbor_vehicles"][0]
        assert nv["vin"] == b"VIN003"
        assert nv["speed"] == pytest.approx(8.0)

    def test_empty_lists(self):
        """Empty next_tls and neighbor_vehicles should serialize/deserialize correctly."""
        state = _make_sample_state()
        state["next_tls"] = []
        state["neighbor_vehicles"] = []

        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert result["next_tls"] == []
        assert result["neighbor_vehicles"] == []

    def test_multiple_neighbors(self):
        """Multiple neighbor vehicles should round-trip correctly."""
        state = _make_sample_state()
        state["neighbor_vehicles"].append(
            {
                "vin": "VIN004",
                "x": 120.0,
                "y": 220.0,
                "lon": 136.92,
                "lat": 35.12,
                "speed": 12.0,
                "accel": -0.5,
                "road_id": "road_1",
                "lane_id": "lane_1",
                "lane_idx": 1,
                "angle": 90.0,
            },
        )

        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert len(result["neighbor_vehicles"]) == 2

    def test_multiple_tls(self):
        """Multiple TLS entries should round-trip correctly."""
        state = _make_sample_state()
        state["next_tls"].append(
            {"id": "tls_1", "idx": 1, "dist": 200.0, "signal": "R"},
        )

        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert len(result["next_tls"]) == 2

    def test_camera_images_round_trip(self):
        """Camera images should round-trip correctly when present."""
        state = _make_sample_state()
        state["camera_images"] = [
            (0, b"\x89PNG\x00\x01\x02\x03"),
            (1, b"\xff\xd8\xff\xe0JFIF"),
        ]

        data = fb_serialize(state)
        result = fb_deserialize(data)

        assert len(result["camera_images"]) == 2
        assert result["camera_images"][0][0] == 0
        assert result["camera_images"][1][0] == 1
