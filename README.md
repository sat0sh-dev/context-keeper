# ContextKeeper

**AI-Native Development Context Engine** - Helps AI agents remember your build environment after context compression.

## The Problem

When working with AI coding assistants (Claude Code, Cursor, etc.) on complex projects like AOSP, ROS, or embedded Linux:

- AI forgets your build targets after context compression
- You repeatedly explain which container to use
- Environment variables and lunch targets get lost
- "Run this in the container" instructions disappear

## The Solution

ContextKeeper provides a persistent, structured summary of your development environment that AI agents can query anytime via MCP (Model Context Protocol).

## Features

- **BuildScript Collector**: Parses your build scripts and extracts targets, environment variables
- **Container Collector**: Detects running Podman/Docker containers (dynamic state)
- **History Collector**: Tracks relevant commands (lunch, source, export) via Claude Code Hooks
- **MCP Server**: Integrates with Claude Code and other MCP-compatible tools
- **Auto-Refresh**: CLAUDE.md instructs AI to refresh context when needed

### Why Dynamic Collection Matters

Static documentation (CLAUDE.md) becomes stale. ContextKeeper collects **current state** at query time:
- Which containers are actually running right now
- What `lunch` target was used in the last session
- Environment variables that were `source`d

## Quick Start

### 1. Install

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
```

### 3. Test

```bash
cd /your/project
/path/to/context-keeper
```

### 4. Setup with Claude Code

Run the install script:

```bash
./install.sh
```

Or manually:

1. Add MCP server to `~/.claude.json`:

```json
{
  "projects": {
    "/your/project": {
      "mcpServers": {
        "context-keeper": {
          "type": "stdio",
          "command": "/path/to/context-keeper/context-keeper-mcp.sh",
          "args": [],
          "env": {}
        }
      }
    }
  }
}
```

2. Restart Claude Code and verify with `/mcp`

## Configuration Reference

### `contextkeeper.toml`

```toml
[project]
name = "Project Name"           # Project display name
type = "aosp"                   # Project type (aosp, ros, yocto, etc.)

[scripts]
entry_point = "build.sh"        # Main build script
config_dir = "config"           # Directory containing target configs
config_pattern = "*.conf"       # Glob pattern for config files
extract_vars = [                # Variables to extract from configs
    "TARGET_NAME",
    "CONTAINER_NAME",
    "LUNCH_TARGET"
]

[containers]
runtime = "podman"              # Container runtime (podman/docker)

[hints]
default = "Important instructions for AI"

[history]
enabled = true                  # Enable command history tracking
patterns = [                    # Regex patterns to match relevant commands
    "lunch\\s+\\S+",
    "source.*envsetup",
    "export\\s+\\w+="
]
max_entries = 20                # Max history entries to show
```

### Command History Setup

ContextKeeper uses Claude Code Hooks to log commands. The install script configures this automatically, or manually add to `~/.claude/settings.json`:

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

### Config file format (e.g., `emu.conf`)

```bash
TARGET_NAME="emu"
TARGET_DESCRIPTION="Android Emulator"
CONTAINER_NAME="build-env"
LUNCH_TARGET="sdk_car_dev-userdebug"
CAN_EMULATOR=true
CAN_FLASH=false
```

## MCP Tools

| Tool | Description |
|------|-------------|
| `get_dev_context` | Returns full development context as Markdown |

## Roadmap

- [x] Command history tracking (via Claude Code Hooks)
- [ ] Environment variable collector (host-side)
- [ ] Git branch/status collector
- [ ] ROS/ROS2 workspace detection
- [ ] Yocto/BitBake support
- [ ] Guardrails (warn before dangerous commands)

## License

MIT

## Contributing

Contributions welcome! Please open an issue first to discuss changes.
