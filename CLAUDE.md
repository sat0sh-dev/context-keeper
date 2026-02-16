# ContextKeeper Development

## Project Overview
ContextKeeper is an AI-Native Development Context Engine that helps AI agents remember build environments after context compression.

## Build
```bash
cargo build --release
```

## Test
```bash
cd /path/to/project/with/contextkeeper.toml
/path/to/context-keeper
```

## Key Files
- `src/main.rs` - Main implementation with collectors
- `context-keeper-mcp.sh` - MCP server wrapper
- `install.sh` - Installation script
