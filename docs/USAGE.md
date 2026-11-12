# Usage

## Running

### Basic Usage

```bash
# Set up environment variables (choose local or ray mode)
source setenv local   # local development (default)
source setenv ray     # Ray cluster deployment

# Run SUMO-based simulation
./bin/run-sim

# Launch edge/daemon processes locally
./bin/run-launch

# Launch on remote infrastructure
./bin/run-remote-launch
```

`WORLD_MODE` controls which config files are used (`etc/*_${WORLD_MODE}.toml`) and is set by `source setenv [local|ray]`.

## Tools

### JSONL Workflow

The platform provides three tools for experiment logging, filtering, and replay:

#### 1. Log Generation

Record events during simulation by adding a log flag when launching:

Output format (one JSON event per line):

```json
{"2": {"vin": "2", "ts": 2.0, "speed": 11.81, "lane_id": "200239781#8_0", ...}}
```

#### 2. Log Filtering (`filter-jsonl`)

Extract specific events:

```bash
./bin/filter-jsonl -i input.jsonl -o filtered.jsonl
```

Features:

- Stream processing for memory efficiency
- Condition-based filtering
- VIN-specific extraction

#### 3. Log Replay (`play-jsonl`)

Replay events in chronological order:

```bash
# Basic replay
./bin/play-jsonl -i input.jsonl

# Real-time replay
./bin/play-jsonl -i input.jsonl -r

# Accelerated replay (2x speed)
./bin/play-jsonl -i input.jsonl -r --speed 2.0
```

### Best Practices

1. Enable logging during simulation runs
2. Filter large logs before analysis
3. Use replay for validation and visualization
4. Refer to `AGENTS.md` for agent-specific guidelines
