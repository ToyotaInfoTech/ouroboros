# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

# SUMOの実行コマンド
sumo_binary = "sumo-gui"  # GUI版、CLIなら "sumo"
sumo_config = "triangle.sumocfg"  # ネットワーク・ルート設定ファイル

if __name__ == "__main__":
    import traci

    traci.start([
        sumo_binary,
        "-c", sumo_config,
        "-d", "400",
        "--start",
    ])

    # 無限ループでシミュレーション
    while traci.simulation.getMinExpectedNumber() > 0:
        traci.simulationStep()
        for vid in traci.vehicle.getIDList():
            road_id = traci.vehicle.getRoadID(vid)
            if traci.vehicle.getRouteIndex(vid) == len(traci.vehicle.getRoute(vid)) - 1:
                pos = traci.vehicle.getLanePosition(vid)
                lane_id = traci.vehicle.getLaneID(vid)
                lane_len = traci.lane.getLength(lane_id)
                if lane_len - pos < 5.0:
                    # ルート終端付近に到達したらルート再設定
                    traci.vehicle.moveTo(vid, "E0_0", 0.0)
                    traci.vehicle.setRouteID(vid, "tri")
                    traci.vehicle.setSpeed(vid, -1)
                    print(f"Vehicle {vid} route replaced at edge {road_id}")
