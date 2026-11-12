# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Main Runtime Module

Run traffic simulations with Ouroboros, a distributed simulation framework.

Use this command to launch and manage traffic simulations. You can run simulations
locally on your machine or connect to a remote simulator. Optionally enable edge
processes for distributed computing across multiple machines using Ray.

```
Usage:
    main [options] <router-name> local -- <sim-bin> [<local-sim-args>...]
    main [options] <router-name> remote [<remote-sim-args>...]
    main (-h | --help)

Arguments:
    <router-name>       Name of the router to use (e.g., "sumo").
    <sim-bin>           Path to the simulator binary (for local mode).
    <local-sim-args>    Additional arguments for the local simulator.
    <remote-sim-args>   Additional arguments for the remote simulator provided as <key>=<value> pairs.

Options:
    -h --help                           Show this help message.
    -s --sim-addr=<sim-address>         Remote simulator address in the form <host>:<port>.
    -r --ray-addr=<ray-address>         Ray cluster address (e.g., "auto" or "ray://123.45.67.89:10001").
    -l --log-path=<path>                Path to simulation log output file. The file extension specifies the format.
                                        Supported formats: CSV(.csv) or JSONL(.jsonl).
    -o --log-overwrite                  Overwrites the log output file if it already exists. [default: False]
    -v --verbose                        Enable detailed logging output. [default: False]
    -w --world-name=<world-name>        Enable world processes with the specified world type (e.g., "ray" or "local").
    -e --edge-enabled                   Enable edge functionality in edge processes. [default: False]
    -t --throughput-interval=<secs>     Interval in seconds for logging simulation throughput. [default: 5.0]
    --renderer-name=<renderer_name>     Renderer to use for edge processes (e.g., "simple3d").
    --min-edges-per-place=<num>         Minimum number of edges per place when using edge processes. [default: 1]
    --max-edges-per-place=<num>         Maximum number of edges per place when using edge processes. [default: 10]
    --max-places=<num>                  Maximum number of places when using edge processes. [default: 50]
    --num-cpus-per-place=<num>          Number of CPUs to allocate per place. [default: 0.5]
    --num-gpus-per-place=<num>          Number of GPUs to allocate per place. [default: 0.0]
    --rust-log=<level>                  Logging level for Rust components (e.g., "info", "debug"). [default: info]
    --rust-log-style=<style>            Logging style for Rust components (e.g., "auto", "always", "never"). [default: auto]
    --rust-backtrace=<value>            Backtrace setting for Rust components (e.g., "0", "1", and "full"). [default: 0]
    --edge-exec=<path>                  Path to the edge process executable. [default: ./target/debug/ouroedge]
    --edge-config=<path>                Path to the edge configuration file. [default: ./etc/edge_config_base.toml]
    --edge-behavior-config=<path>       Path to the behavior configuration file.
```
"""

import asyncio
import logging
import os
from logging import getLogger
from pprint import pformat
from typing import Any

import yaml

from ouroboros.router import Router, new_router
from ouroboros.world import base, new_world

from .util import configure_global_logging

__docformat__ = "markdown"

OPT2KWARG = {
    "local": ("local", str),
    "remote": ("remote", str),
    "<router-name>": ("router_name", str),
    "<sim-bin>": ("sim_bin", str),
    "<local-sim-args>": ("local_sim_args", list),
    "<remote-sim-args>": ("remote_sim_args", list),
    "--sim-addr": ("sim_addr", str),
    "--ray-addr": ("ray_addr", str),
    "--verbose": ("verbose", bool),
    "--world-name": ("world_name", str),
    "--renderer-name": ("renderer_name", str),
    "--log-path": ("log_path", str),
    "--log-overwrite": ("log_overwrite", bool),
    "--edge-enabled": ("edge_enabled", bool),
    "--throughput-interval": ("throughput_interval", float),
    "--min-edges-per-place": ("min_edges_per_place", int),
    "--max-edges-per-place": ("max_edges_per_place", int),
    "--max-places": ("max_places", int),
    "--num-cpus-per-place": ("num_cpus_per_place", float),
    "--num-gpus-per-place": ("num_gpus_per_place", float),
    "--rust-log": ("rust_log", str),
    "--rust-log-style": ("rust_log_style", str),
    "--rust-backtrace": ("rust_backtrace", str),
    "--edge-exec": ("edge_exec", str),
    "--edge-config": ("edge_config", str),
    "--edge-behavior-config": ("edge_behavior_config", str),
}


def _setup_logging():
    """
    Setup global logging configuration from YAML file.
    """
    config_path = os.path.join("etc", "logging.yaml")
    with open(config_path, encoding="utf-8") as f:
        log_config = yaml.safe_load(f)
    configure_global_logging(log_config)


_setup_logging()
_logger = getLogger(__name__)


def _launch_simulator(router: Router, sim_bin: str, sim_args: list[str]):
    """
    Launch the simulator with the given command.
    Args:
        router (Router): The router instance to use.
        sim_bin (str): Path to the simulator binary.
        sim_args (list[str]): List of arguments for the simulator.
    """
    sim_cmd = [sim_bin, *sim_args]  # Collect sim command
    sim_cmd = [arg.strip() for arg in sim_cmd if arg.strip()]  # Remove empty args
    router.launch_sim(sim_cmd)


def _connect_remote_simulator(router: Router, sim_addr: str, sim_args: list[str]):
    """
    Connect to a remote simulator.

    Args:
        router (Router): The router instance to use.
        sim_addr (str): Remote simulator address in the format <host>:<port>.
    """
    host_port = sim_addr.split(":")
    if len(host_port) != 2:
        _logger.error(f"Invalid remote simulator address format {sim_addr}. Use <host>:<port>.")
        exit(1)
    remote_sim_kwargs = {"host": host_port[0], "port": int(host_port[1])}
    remote_sim_kwargs.update({k: v for k, v in (arg.split("=") for arg in sim_args if "=" in arg)})
    router.connect_to_sim(**remote_sim_kwargs)


def _create_world(world_name, *args, **kwargs) -> base.World:
    """
    Create a world with edge processes.

    Args:
        world_name (str): Name of the world type to create (e.g., "ray" or "local").
        *args: Positional arguments to pass to the world constructor.
        **kwargs: Keyword arguments to pass to the world constructor.
    """

    return new_world(world_name, *args, **kwargs)


def _create_router(
    router_name: str,
    renderer_name: str | None = None,
    log_path: str | None = None,
    log_overwrite: bool = False,
):
    """
    Create a router instance.

    Args:
        router_name (str): Name of the router to create.
        world (base.World | None): World instance to use.
        renderer_name (str | None): Renderer instance to use.
        log_path (str | None): Path to the log output file.
        log_overwrite (bool): Whether to allow overwriting the log file if it exists.
    """
    return new_router(
        router_name,
        log_path=log_path,
        renderer_name=renderer_name,
        log_overwrite=log_overwrite,
    )


async def _main():
    """
    Main entry point for the Ouroboros runtime.
    """
    try:
        import atexit

        from docopt import docopt

        args = docopt(__doc__ or "")
        args: dict[str, Any] = {
            OPT2KWARG[k][0]: OPT2KWARG[k][1](v) for k, v in args.items() if k in OPT2KWARG and v is not None
        }

        if args.pop("verbose"):
            getLogger().setLevel(logging.DEBUG)

        _logger.debug(f"Arguments:\n{pformat(args)}")

        world = None
        world_name: str | None = args.pop("world_name", None)
        renderer_name: str | None = args.pop("renderer_name", None)
        if world_name:
            _logger.info(f"Initializing the world '{world_name}'")
            world = _create_world(
                world_name,
                renderer_name=renderer_name,
                **args,
            )
            _logger.info(f"The world '{world_name}' is initialized.")

        # Create router (See sumo.py for details)
        router = _create_router(
            args.pop("router_name"),
            renderer_name=renderer_name,
            log_path=args.pop("log_path", None),
            log_overwrite=args.pop("log_overwrite"),
        )

        if args["local"]:
            sim_bin, sim_args = str(args.pop("sim_bin")), args.pop("local_sim_args", [])
            _logger.info(f"Launching local simulator: {' '.join([sim_bin, *sim_args])}")
            _launch_simulator(
                router,
                sim_bin,
                sim_args,
            )
        elif args["remote"]:
            sim_addr, sim_args = str(args.pop("sim_addr")), args.pop("remote_sim_args", [])
            _logger.info(f"Connecting to remote simulator: {' '.join([sim_addr, *sim_args])}")
            _connect_remote_simulator(
                router,
                sim_addr,
                sim_args,
            )
        else:
            _logger.error("Either 'local' or 'remote' must be specified.")
            exit(1)

        async def _stop():
            _logger.info("Shutting down Ouroboros runtime.")
            if world:
                await world.stop()
            if router:
                router.stop()

        # Register shutdown procedure
        atexit.register(lambda: asyncio.run(_stop()))

        await router.run(world=world)
    except KeyboardInterrupt:
        _logger.info("Ouroboros runtime interrupted by user.")


if __name__ == "__main__":
    asyncio.run(_main())
