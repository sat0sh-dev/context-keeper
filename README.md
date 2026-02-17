# ContextKeeper

**AI-Native Development Context Engine** - Helps AI agents remember your build environment after context compression.

## Target Users

ContextKeeper is designed for developers working with **complex build environments**:

- **AOSP / Android Platform** - Multiple lunch targets, containerized builds
- **ROS / ROS2** - Workspace configurations, launch files
- **Yocto / Embedded Linux** - BitBake targets, layers
- **Multi-container development** - Docker/Podman based workflows

If your project requires explaining build setup repeatedly to AI assistants, ContextKeeper can help.

## The Problem

When working with AI coding assistants (Claude Code, Cursor, etc.) on complex projects:

- AI forgets your build targets after context compression
- You repeatedly explain which container to use
- Environment variables and lunch targets get lost
- "Run this in the container" instructions disappear

## The Solution

ContextKeeper provides a **dynamic, queryable summary** of your development environment via MCP (Model Context Protocol).

Unlike static documentation, ContextKeeper collects **current state** at query time:
- Which containers are actually running right now
- What `lunch` target was used in the last session
- Build targets and their configurations

## Features

| Collector | Type | Description |
|-----------|------|-------------|
| **BuildScript** | Static | Parses config files to extract build targets |
| **Container** | Dynamic | Detects running Podman/Docker containers |
| **History** | Dynamic | Tracks relevant commands via Claude Code Hooks |

## Quick Start

### 1. Build

```bash
git clone https://github.com/user/context-keeper
cd context-keeper
cargo build --release
```

### 2. Configure your project

Create `contextkeeper.toml` in your project root:

```toml
[project]
name = "My AOSP Project"
type = "aosp"

[scripts]
entry_point = "scripts/build.sh"
config_dir = "scripts/config"
config_pattern = "*.conf"

[containers]
runtime = "podman"  # or "docker"

[hints]
default = "Build commands must be executed inside the container."

[history]
enabled = true
patterns = [
    "lunch\\s+\\S+",
    "source.*envsetup",
    "export\\s+\\w+="
]
max_entries = 20
```

### 3. Test locally

```bash
cd /your/project
/path/to/context-keeper --context
```

### 4. Setup with Claude Code

**Option A: Run install script**

```bash
./install.sh
```

**Option B: Manual setup**

1. Add MCP server to `~/.claude.json`:

```json
{
  "projects": {
    "/your/project": {
      "mcpServers": {
        "context-keeper": {
          "type": "stdio",
          "command": "/path/to/context-keeper/target/release/context-keeper",
          "args": [],
          "env": {}
        }
      }
    }
  }
}
```

2. (Optional) Enable command history logging in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/context-keeper/hooks/log-commands.sh"
          }
        ]
      }
    ]
  }
}
```

3. Restart Claude Code and verify with `/mcp`

## AOSP Setup Guide

### Directory Structure

```
your-aosp-project/
├── contextkeeper.toml          # ContextKeeper config
├── manifest/
│   └── scripts/
│       ├── aosp.sh             # Entry point script
│       └── config/
│           ├── emu.conf        # Emulator target config
│           └── device.conf     # Device target config
```

### Config File Format (`*.conf`)

```bash
# emu.conf - Android Emulator target
TARGET_NAME="emu"
TARGET_DESCRIPTION="AAOS Emulator (Car)"
CONTAINER_NAME="aosp-build-env"
LUNCH_TARGET="sdk_car_dev-trunk_staging-userdebug"
CAN_EMULATOR=true
CAN_FLASH=false
```

### contextkeeper.toml for AOSP

```toml
[project]
name = "AOSP Custom Build"
type = "aosp"

[scripts]
entry_point = "manifest/scripts/aosp.sh"
config_dir = "manifest/scripts/config"
config_pattern = "*.conf"

[containers]
runtime = "podman"

[hints]
default = "Build commands must run inside the container. Use './manifest/scripts/aosp.sh' as entry point."

[history]
enabled = true
patterns = [
    "lunch\\s+\\S+",           # lunch target selection
    "source.*envsetup",        # environment setup
    "m\\s+\\S+",               # make shortcut
    "mm\\b",                   # make module
    "mma\\b",                  # make module all
    "podman\\s+exec.*lunch",   # lunch inside container
]
max_entries = 20
```

### Example Output

```markdown
# Development Context (ContextKeeper)

## Project
- **Name:** AOSP Custom Build
- **Type:** aosp

## AI Hints (Important)
> Build commands must run inside the container.

## Available Build Targets

| Target | Description | Container | Lunch Target |
|--------|-------------|-----------|--------------|
| emu | AAOS Emulator | aosp-build-env | sdk_car_dev-... |
| device | Pixel 7a | aosp-build-env | aosp_lynx-... |

## Active Containers
- **aosp-build-env** (podman): Up 5 days

## Recent Relevant Commands
| Time | Command |
|------|---------|
| 2024-01-15T10:30:00Z | `lunch sdk_car_dev-trunk_staging-userdebug` |
```

## Configuration Reference

### `contextkeeper.toml`

| Section | Field | Description |
|---------|-------|-------------|
| `[project]` | `name` | Project display name |
| | `type` | Project type (aosp, ros, yocto, custom) |
| `[scripts]` | `entry_point` | Main build script path |
| | `config_dir` | Directory containing target configs |
| | `config_pattern` | Glob pattern for config files |
| `[containers]` | `runtime` | Container runtime (podman/docker) |
| `[hints]` | `default` | Important instructions for AI |
| `[history]` | `enabled` | Enable command history (true/false) |
| | `patterns` | Regex patterns to match relevant commands |
| | `max_entries` | Maximum history entries to display |

## MCP Tools

| Tool | Description |
|------|-------------|
| `get_dev_context` | Returns full development context as Markdown |

## CLI Usage

```bash
# Output context as Markdown (for testing)
context-keeper --context
context-keeper -c

# Run as MCP server (default, used by Claude Code)
context-keeper
```

## How It Works

```
┌─────────────────┐     MCP Protocol      ┌──────────────────┐
│   Claude Code   │ ◄──────────────────► │  ContextKeeper   │
│                 │                       │                  │
│ "What container │  get_dev_context()   │ - Read config    │
│  should I use?" │ ───────────────────► │ - Check podman   │
│                 │                       │ - Parse history  │
│                 │ ◄─────────────────── │ - Return context │
│ "Use aosp-env"  │   Markdown response  │                  │
└─────────────────┘                       └──────────────────┘
```

## Roadmap

- [x] BuildScript Collector
- [x] Container Collector
- [x] History Collector (via Claude Code Hooks)
- [x] Official Rust MCP SDK (rmcp)
- [ ] `context-keeper init` wizard
- [ ] Git branch/status collector
- [ ] Guardrails (warn before dangerous commands)
- [ ] ROS/ROS2 workspace detection
- [ ] Yocto/BitBake support

## License

MIT

## Contributing

Contributions welcome! Please open an issue first to discuss changes.
