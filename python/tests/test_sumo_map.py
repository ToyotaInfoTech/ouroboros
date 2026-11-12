# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Unit tests for SumoMap.collect() using sumolib-based implementation."""

from unittest.mock import MagicMock, patch

import pytest

from ouroboros.router.sumo_map import SumoMap


def _make_lane(shape: list, width: float) -> MagicMock:
    """Create a mock sumolib Lane with the given shape and width."""
    lane = MagicMock()
    lane.getShape.return_value = shape
    lane.getWidth.return_value = width
    lane.getID.return_value = f"lane_{id(lane)}"
    return lane


def _make_edge(lanes: list) -> MagicMock:
    edge = MagicMock()
    edge.getLanes.return_value = lanes
    return edge


def _make_tls(tls_id: str, connections: list) -> MagicMock:
    tls = MagicMock()
    tls.getID.return_value = tls_id
    tls.getConnections.return_value = connections
    return tls


def _make_node(shape: list, coord: tuple = (0.0, 0.0)) -> MagicMock:
    node = MagicMock()
    node.getShape.return_value = shape
    node.getCoord.return_value = coord
    return node


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture()
def mock_net():
    """Return a mock sumolib Net covering basic geometry."""
    lane_a = _make_lane([(0.0, 0.0), (1.0, 0.0)], width=3.2)
    lane_b = _make_lane([(0.0, 1.0), (1.0, 1.0)], width=2.8)
    lane_empty = _make_lane([], width=3.0)  # should be skipped

    edge1 = _make_edge([lane_a, lane_b])
    edge2 = _make_edge([lane_empty])

    in_lane = MagicMock()
    in_lane.getID.return_value = "edge1_0"
    conn = [in_lane, MagicMock(), 0]  # [inLane, outLane, linkNo]

    tls1 = _make_tls("J0", [conn])
    tls_no_node = _make_tls("ORPHAN", [])  # no matching node — should be skipped

    node_j0 = _make_node([(0, 0), (1, 0), (1, 1), (0, 1)], coord=(5.0, 10.0))
    node_small = _make_node([(0, 0), (1, 1)])  # only 2 points — should be skipped
    node_tri = _make_node([(0, 0), (1, 0), (0.5, 1)])  # exactly 3 points — included

    net = MagicMock()
    net.getEdges.return_value = [edge1, edge2]
    net.getTrafficLights.return_value = [tls1, tls_no_node]
    net.getNodes.return_value = [node_j0, node_small, node_tri]

    def _has_node(nid: str) -> bool:
        return nid == "J0"

    def _get_node(nid: str):
        assert nid == "J0"
        return node_j0

    net.hasNode.side_effect = _has_node
    net.getNode.side_effect = _get_node

    return net


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


class TestSumoMapCollect:
    def _run_collect(self, mock_net) -> SumoMap:
        with (
            patch("ouroboros.router.sumo_map.traci") as mock_traci,
            patch("ouroboros.router.sumo_map.sumolib") as mock_sumolib,
        ):
            mock_traci.simulation.getOption.return_value = "/fake/nagoya.net.xml"
            mock_sumolib.net.readNet.return_value = mock_net
            result = SumoMap.collect()

        mock_traci.simulation.getOption.assert_called_once_with("net-file")
        mock_sumolib.net.readNet.assert_called_once_with(
            "/fake/nagoya.net.xml", withInternal=True
        )
        return result

    def test_returns_sumo_map_instance(self, mock_net):
        result = self._run_collect(mock_net)
        assert isinstance(result, SumoMap)

    def test_lanes_excludes_empty_shapes(self, mock_net):
        result = self._run_collect(mock_net)
        # edge1 has 2 lanes with shapes; edge2 has 1 lane with empty shape
        assert len(result.lanes) == 2

    def test_lane_dict_structure(self, mock_net):
        result = self._run_collect(mock_net)
        for lane in result.lanes:
            assert "shape" in lane
            assert "width" in lane
            assert isinstance(lane["shape"], list)
            assert len(lane["shape"]) > 0
            assert isinstance(lane["width"], float)

    def test_lane_shape_values(self, mock_net):
        result = self._run_collect(mock_net)
        shapes = [tuple(p) for lane in result.lanes for p in lane["shape"]]
        assert (0.0, 0.0) in shapes
        assert (1.0, 0.0) in shapes

    def test_tls_excludes_orphaned_tls(self, mock_net):
        result = self._run_collect(mock_net)
        # "J0" has a node; "ORPHAN" does not and should be skipped
        assert len(result.tls_list) == 1
        assert result.tls_list[0]["id"] == "J0"

    def test_tls_dict_structure(self, mock_net):
        result = self._run_collect(mock_net)
        tls = result.tls_list[0]
        assert set(tls.keys()) == {"id", "x", "y", "controlled_lanes"}
        assert tls["x"] == pytest.approx(5.0)
        assert tls["y"] == pytest.approx(10.0)
        assert tls["controlled_lanes"] == ["edge1_0"]

    def test_junctions_excludes_shapes_under_3_points(self, mock_net):
        result = self._run_collect(mock_net)
        # node_j0 (4 pts) + node_tri (3 pts) included; node_small (2 pts) excluded
        assert len(result.junctions) == 2

    def test_junction_dict_structure(self, mock_net):
        result = self._run_collect(mock_net)
        for junc in result.junctions:
            assert "shape" in junc
            assert isinstance(junc["shape"], list)
            assert len(junc["shape"]) >= 3

    def test_properties_are_accessible(self, mock_net):
        result = self._run_collect(mock_net)
        assert result.lanes is result.lanes  # same reference
        assert result.tls_list is result.tls_list
        assert result.junctions is result.junctions

    def test_empty_network(self):
        """collect() on an empty network returns empty lists without error."""
        empty_net = MagicMock()
        empty_net.getEdges.return_value = []
        empty_net.getTrafficLights.return_value = []
        empty_net.getNodes.return_value = []

        with (
            patch("ouroboros.router.sumo_map.traci") as mock_traci,
            patch("ouroboros.router.sumo_map.sumolib") as mock_sumolib,
        ):
            mock_traci.simulation.getOption.return_value = "/fake/empty.net.xml"
            mock_sumolib.net.readNet.return_value = empty_net
            result = SumoMap.collect()

        assert result.lanes == []
        assert result.tls_list == []
        assert result.junctions == []
