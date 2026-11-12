# Python Profiling Guide

This guide explains how to profile Python applications in the Ouroboros project and generate FlameGraphs for performance analysis.

## Prerequisites

The required profiling tool `py-spy` is already included in the project dependencies:
- `pyproject.toml`: py-spy>=0.4.1
- `etc/requirements.txt`: py-spy==0.4.1

## Installation

If py-spy is not yet installed, install it using:

```bash
# Using pip
pip install py-spy

# Or using uv
uv pip install py-spy
```

## Profiling Tools

Three profiling scripts are available in the `bin/` directory:

### 1. profile-app - Profile from Start

Profile an application from the beginning of execution.

**Usage:**
```bash
profile-app <output-file> -- <python-command> [args...]
```

**Examples:**
```bash
# Generate SVG FlameGraph
bin/profile-app flamegraph.svg -- python -m ouroboros.main sumo local -- bin/run-sim

# Generate Speedscope format (upload to https://www.speedscope.app/)
bin/profile-app profile.speedscope -- python -m ouroboros.main sumo remote

# Generate raw JSON data
bin/profile-app profile.json -- python -m ouroboros.main sumo local -- bin/run-sim
```

**Supported output formats:**
- `.svg` - FlameGraph (viewable in web browser)
- `.speedscope` - Speedscope format (upload to speedscope.app)
- `.json` - Raw JSON profiling data

### 2. profile-live - Attach to Running Process

Profile an already running Python process.

**Usage:**
```bash
profile-live <pid> <output-file> [duration]
```

**Examples:**
```bash
# Find the process ID
ps aux | grep python

# Profile for 30 seconds (default)
bin/profile-live 12345 flamegraph.svg

# Profile for 60 seconds
bin/profile-live 12345 flamegraph.svg 60
```

### 3. profile-top - Interactive Top View

Display real-time profiling information similar to `top`.

**Usage:**
```bash
profile-top <pid>
```

**Example:**
```bash
# Find the process ID
ps aux | grep python

# Start interactive profiling view
bin/profile-top 12345
```

Press `Ctrl+C` to exit.

## Viewing FlameGraphs

### SVG FlameGraphs

SVG FlameGraphs can be opened directly in a web browser:

```bash
# Using Firefox
firefox flamegraph.svg

# Using Chrome
google-chrome flamegraph.svg

# Using default browser
xdg-open flamegraph.svg
```

### Speedscope Format

1. Generate a `.speedscope` file:
   ```bash
   bin/profile-app profile.speedscope -- python -m ouroboros.main ...
   ```

2. Upload to https://www.speedscope.app/

3. Analyze the interactive visualization

## Understanding FlameGraphs

- **Width**: Represents the time spent in a function (wider = more time)
- **Height**: Represents the call stack depth
- **Color**: Usually random, used to differentiate functions
- **Interactive**: Click on boxes to zoom in, search for function names

### Key Areas to Look For:

1. **Wide plateaus** - Functions consuming significant CPU time
2. **Tall stacks** - Deep call chains that might be optimizable
3. **Repeated patterns** - Potential for caching or optimization

## Advanced Options

### Sampling Rate

Increase sampling rate for more detailed profiling:

```bash
# Default is 100 Hz
py-spy record --rate 100 --output profile.svg -- python script.py

# Higher rate (200 Hz) for more detail
py-spy record --rate 200 --output profile.svg -- python script.py
```

### Native Extensions

Profile native extensions (C/Rust):

```bash
py-spy record --native --output profile.svg -- python script.py
```

### Subprocesses

Profile all subprocesses (enabled by default in our scripts):

```bash
py-spy record --subprocesses --output profile.svg -- python script.py
```

## Troubleshooting

### Permission Denied

py-spy may require elevated privileges:

```bash
sudo -E bin/profile-app flamegraph.svg -- python -m ouroboros.main ...
```

Or add ptrace capability:

```bash
sudo setcap cap_sys_ptrace+ep $(which python)
```

### Process Not Found

Ensure the Python process is running:

```bash
ps aux | grep python
pgrep -f "python.*ouroboros"
```

### High Overhead

If profiling causes too much slowdown, reduce the sampling rate:

```bash
py-spy record --rate 50 --output profile.svg -- python script.py
```

## Example Workflow

Complete profiling workflow:

```bash
# 1. Generate FlameGraph
bin/profile-app flamegraph.svg -- python -m ouroboros.main sumo local -- bin/run-sim

# 2. Open in browser
firefox flamegraph.svg

# 3. Identify bottlenecks

# 4. Optimize code

# 5. Profile again to verify improvements
bin/profile-app flamegraph-optimized.svg -- python -m ouroboros.main sumo local -- bin/run-sim

# 6. Compare results
```

## Resources

- [py-spy Documentation](https://github.com/benfred/py-spy)
- [FlameGraph Guide](https://www.brendangregg.com/flamegraphs.html)
- [Speedscope](https://www.speedscope.app/)
