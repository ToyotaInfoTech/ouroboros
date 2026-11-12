#!/usr/bin/env python3
# SPDX-FileCopyrightText: Copyright 2026 TOYOTA MOTOR CORPORATION
# SPDX-License-Identifier: Apache-2.0

"""Parse etc/hosts.toml and output host information for shell scripts.

Usage:
    parse-hosts.py info <hostname>    # Print: user@address remote_dir role1,role2
    parse-hosts.py role <role>        # Print: hostname1 hostname2 ...
    parse-hosts.py list               # Print all hostnames
"""

import sys
import tomllib
from pathlib import Path


def load_hosts() -> dict:
    """Load hosts from etc/hosts.toml relative to this script's project root.

    Returns:
        dict: Parsed TOML hosts configuration.
    """
    root = Path(__file__).resolve().parent.parent
    hosts_file = root / "etc" / "hosts.toml"
    if not hosts_file.exists():
        print(f"Error: {hosts_file} not found", file=sys.stderr)
        sys.exit(1)
    with open(hosts_file, "rb") as f:
        return tomllib.load(f)


def cmd_info(hosts: dict, hostname: str) -> None:
    """Print host info in the format: user@address remote_dir role1,role2"""
    host = hosts.get("hosts", {}).get(hostname)
    if not host:
        print(f"Error: host '{hostname}' not found in hosts.toml", file=sys.stderr)
        sys.exit(1)
    user = host["user"]
    address = host["address"]
    remote_dir = host["remote_dir"]
    roles = ",".join(host.get("roles", []))
    print(f"{user}@{address} {remote_dir} {roles}")


def cmd_role(hosts: dict, role: str) -> None:
    """Print hostnames matching a given role, space-separated."""
    matched = []
    for name, host in hosts.get("hosts", {}).items():
        if role == "all" or role in host.get("roles", []):
            matched.append(name)
    if not matched:
        print(f"Error: no hosts found for role '{role}'", file=sys.stderr)
        sys.exit(1)
    print(" ".join(matched))


def cmd_list(hosts: dict) -> None:
    """Print all hostnames, space-separated."""
    names = list(hosts.get("hosts", {}).keys())
    print(" ".join(names))


def main() -> None:
    if len(sys.argv) < 2:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(1)

    command = sys.argv[1]
    hosts = load_hosts()

    if command == "info":
        if len(sys.argv) < 3:
            print("Usage: parse-hosts.py info <hostname>", file=sys.stderr)
            sys.exit(1)
        cmd_info(hosts, sys.argv[2])
    elif command == "role":
        if len(sys.argv) < 3:
            print("Usage: parse-hosts.py role <role>", file=sys.stderr)
            sys.exit(1)
        cmd_role(hosts, sys.argv[2])
    elif command == "list":
        cmd_list(hosts)
    else:
        print(f"Unknown command: {command}", file=sys.stderr)
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
