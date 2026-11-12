# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""
Usage:
    play-jsonl FILE [--logging=<yaml>] [--realtime] [--speed=<x>] -- [<ouroedge>...]

Options:
    FILE                       JSONL file to play (one JSON per line).
    -l --logging=<yaml>           Logging configuration YAML (e.g., etc/logging.yaml).
    -r --realtime                 Replay in real time according to ts differences (default: off = send immediately).
    -s --speed=<x>                Playback speed (1.0 = normal, 2.0 = double speed) [default: 1.0].
"""

import asyncio
import json
import logging
import os
import time
from logging import config as logging_config

from docopt import docopt

from ouroboros.message.fb_serde import fb_deserialize, fb_serialize

try:
    import yaml  # type: ignore
except ImportError:
    yaml = None

import contextlib

from ouroboros.edge import new_peer
from ouroboros.edge.rpc import RpcClient

_logger = logging.getLogger(__name__)


async def start_client_for_vin(clients: dict[str, RpcClient], vin: str, cmdline: list[str]) -> RpcClient:
    client = clients.get(vin)
    if client is None:
        client = RpcClient(new_peer("uds", cmdline))
        await client.start()
        clients[vin] = client
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"started client for vin={vin} cmd={cmdline}")
    return client


async def call_step(client: RpcClient, params: dict) -> dict | None:
    try:
        data = fb_serialize(params)
        return await client.call("step", data)
    except Exception as e:
        _logger.exception(f"call_step failed: {e}")


async def play_jsonl(path: str, realtime: bool, speed: float, cmdline: list[str]) -> None:
    clients: dict[str, RpcClient] = {}
    base_ts: float | None = None
    start_monotonic = time.monotonic()

    async def schedule_call_step(delay_s: float, vin: str, params: dict):
        client = await start_client_for_vin(clients, vin, cmdline)
        if delay_s > 0:
            await asyncio.sleep(delay_s)
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"calling method=step vin={vin} with params={params}")
        response = await call_step(client, params)
        result = response.ResultAsNumpy().tobytes()
        actions = fb_deserialize(result)
        if _logger.isEnabledFor(logging.DEBUG):
            _logger.debug(f"called method=step vin={vin} actions={actions}")
        # ignore actions for now

    # 読み取り（同期IOだが小さいため許容）。必要ならaiofiles化
    with open(path, encoding="utf-8") as f:
        tasks: list[asyncio.Task] = []
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                kvs = json.loads(line)
            except json.JSONDecodeError as e:
                _logger.warning(f"skip invalid json line: {e}")
                continue

            for vin, state in kvs.items():
                ts = state["ts"]  # 秒（float/int想定）
                if vin is None or ts is None:
                    _logger.warning("skip line without vin/ts")
                    continue

                if base_ts is None:
                    base_ts = float(ts)

                delay = 0.0
                if realtime:
                    delta = (float(ts) - base_ts) / max(speed, 1e-6)
                    # 再生開始からの経過に合わせる
                    elapsed = time.monotonic() - start_monotonic
                    delay = max(0.0, delta - elapsed)

                tasks.append(asyncio.create_task(schedule_call_step(delay, vin, state)))

            if tasks:
                await asyncio.gather(*tasks)

    # 停止処理
    for _vin, peer in clients.items():
        with contextlib.suppress(Exception):
            await peer.stop()


def setup_logging(yaml_path: str | None) -> None:
    if not yaml_path:
        return
    if not os.path.exists(yaml_path):
        _logger.warning(f"logging yaml not found: {yaml_path}")
        return
    if yaml is None:
        _logger.warning("PyYAML not available; skip logging config")
        return
    try:
        with open(yaml_path, encoding="utf-8") as f:
            cfg = yaml.safe_load(f)
        logging_config.dictConfig(cfg)
        _logger.info("logging config loaded from yaml")
    except (yaml.YAMLError, OSError) as e:
        _logger.warning(f"failed to load logging config: {e}")


def main():
    args = docopt(__doc__)
    file_path = args["FILE"]
    logging_yaml = args.get("--logging")
    realtime = bool(args.get("--realtime"))
    speed = float(args.get("--speed") or 1.0)
    cmdline = args["<ouroedge>"]
    setup_logging(logging_yaml)

    asyncio.run(play_jsonl(file_path, realtime, speed, cmdline))


if __name__ == "__main__":
    main()
