# How to run demo

## Prerequisites

- Cluster nodes
  - Minimum 5 nodes (1 simulator, 3 workers, and 1 server)
    - Hosts and roles are defined in `etc/hosts.toml`; see [Deployment](DEPLOYMENT.md)
- Git
- Docker and Docker Compose
- `cargo-make` installed
  - Install via Cargo:

    ```bash
    cargo install cargo-make@0.37.24
    ```

- SSH local forward

    ```shell
    Host ouroworld
        Hostname 203.0.113.10
        User ubuntu
        IdentityFile ~/.ssh/id_ed25519
        LocalForward 3000 198.51.100.10:3000 # Grafana
        LocalForward 5901 localhost:5901
        LocalForward 8265 localhost:8265 # Ray Dashboard
        LocalForward 8080 198.51.100.10:8080 # Ouroboros Daemon Metrics Serer
    ```

## Clone the repository

```bash
git clone git@github.com:ToyotaInfoTech/ouroboros.git
cd ouroboros
```

## Build

```bash
cargo build --release
```

For cross-compilation to Linux x86_64 (from macOS):

```bash
cargo zigbuild --workspace --release --target x86_64-unknown-linux-gnu
```

## Provision the hosts (first time only)

Install the prerequisites listed in [Deployment](DEPLOYMENT.md#host-provisioning) on every node.

## Sync Files to Remote Hosts

```bash
bin/ouro-sync --role all             # Sync to all hosts (uses role-based rules)
bin/ouro-sync <cloud-host>              # Sync to a specific host
bin/ouro-sync <cloud-host> --dry-run    # Preview without transferring
```

## Start up the cluster

```bash
REMOTE=<cloud-host> cargo make cup      # Start containers (TimescaleDB, Grafana)
REMOTE=<cloud-host> cargo make zup      # Start Zenoh daemon
REMOTE=<cloud-host> cargo make oup      # Start ourodaemon
REMOTE=<cloud-host> cargo make stat     # Check remote cluster status
REMOTE=<cloud-host> cargo make olog     # Tail ourodaemon log
```

Without `REMOTE=`, tasks run locally:

```bash
cargo make cup                       # Start local containers
cargo make stat                      # Check local status
```

## Start the demo

```bash
. setenv
```

or

```bash
source setenv
```

```bash
run-launch -ew local --renderer-name wgpu # Start the demo at the simulator node
```

- See <http://localhost:8265/#/overview> for Ray Dashboard
- See <http://localhost:3000> for Grafana Dashboard
- See <http://localhost:8080/metrics> for Ouroboros Daemon Metrics Server
- See <http://localhost:3001/> for Ouroboros Daemon Dashboard

### Cleanup DB tables

```bash
cd container
cargo make dclear
```

### Drop DB tables

```bash
cd container
cargo make ddrop
```

### Stop the cluster

```bash
REMOTE=<cloud-host> cargo make adown    # Stop all remote services
cargo make adown                     # Stop all local services
```
