# Deployment

## Development Environment

This project can be evaluated on a small multi-node test cluster.
Files are deployed with `bin/ouro-sync` (rsync-based), and remote task execution uses the `REMOTE=<host>` environment variable with `cargo make`.

### Host Configuration

Remote hosts are defined in `etc/hosts.toml`.
Each host has an address, SSH user, remote working directory, and one or more roles (`cloud`, `head`, `work`).

`etc/hosts.toml` and `examples/dot.ssh/config` contain sample host names (`ourocloud`, `ouroworld`, `ouroplace-N`) and documentation-only IP addresses.
Replace them with your own hosts. In the commands below, `<cloud-host>` stands for the host name you gave to the `cloud` role.
The `cup`, `zup` and `oup` tasks need the credentials described in [Security Considerations](SECURITY_CONSIDERATIONS.md) to be present in the environment of that host.

### Host Provisioning

Provision every host yourself; no provisioning automation is included. Each node needs:

| Role | Requirements |
| --- | --- |
| all | SSH access, the repository directory (`~/ouroboros`, created by `bin/ouro-sync`), `uv`, and the Rust toolchain if you build on the host |
| `head` and `work` | SUMO, Python 3.13 with the environment created by `uv sync`, and the `ouroedge` binary |
| `head` | The FlatBuffers compiler (`flatc`) when regenerating code |
| `cloud` | Docker, `zenohd`, `cargo-make`, and the `ourodaemon` binary |

### File Sync (rsync-based)

After building locally (including cross-compilation via `cargo zigbuild`), sync files to remote hosts:

```bash
# Sync to a specific host (uses role-based filter rules automatically)
bin/ouro-sync <cloud-host>

# Sync to all hosts with a given role
bin/ouro-sync --role cloud

# Sync to all hosts
bin/ouro-sync --role all

# Preview without transferring
bin/ouro-sync <cloud-host> --dry-run
```

Sync rules are defined per role in `etc/sync-<role>.rules` (rsync filter format).
Custom rules can be specified with `--rules <file>`.

### Remote Task Execution

Prepend `REMOTE=<host>` to any `cargo make` task to execute it on a remote host via SSH:

```bash
REMOTE=<cloud-host> cargo make cup    # Start containers on <cloud-host>
REMOTE=<cloud-host> cargo make zup    # Start Zenoh on <cloud-host>
REMOTE=<cloud-host> cargo make oup    # Start ourodaemon on <cloud-host>
REMOTE=<cloud-host> cargo make stat   # Show status on <cloud-host>
```

Without `REMOTE`, tasks run locally as before.

## Starting the Cluster

Start/refresh the cluster with:

```bash
cd ouroboros
source setenv ray                        # Select Ray cluster configs
cargo make rup                           # Ray cluster up (local)
REMOTE=<cloud-host> cargo make cup          # TimescaleDB + Grafana up (remote)
REMOTE=<cloud-host> cargo make zup          # Zenoh up (remote)
REMOTE=<cloud-host> cargo make oup          # ourodaemon up (remote)
cargo make stat                          # Check local status
REMOTE=<cloud-host> cargo make stat         # Check remote status
```

The WebUI and metrics servers listen on loopback by default. To reach them from other hosts, set `OURO_WEBUI_BIND`, `OURO_METRICS_BIND` and `OURO_WEBUI_TOKEN` on the cloud host first; see [Security Considerations](SECURITY_CONSIDERATIONS.md).
