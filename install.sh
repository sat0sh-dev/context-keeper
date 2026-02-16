#!/bin/bash
# ContextKeeper Installation Script

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLAUDE_CONFIG="$HOME/.claude.json"
CLAUDE_MD="$HOME/.claude/CLAUDE.md"

echo "=== ContextKeeper Installer ==="
echo ""

# 1. Build if needed
if [ ! -f "$SCRIPT_DIR/target/release/context-keeper" ]; then
    echo "[1/4] Building ContextKeeper..."
    cd "$SCRIPT_DIR"
    cargo build --release
else
    echo "[1/4] Binary already exists, skipping build"
fi

# 2. Add to CLAUDE.md for auto-refresh
echo "[2/4] Setting up auto-refresh in CLAUDE.md..."

mkdir -p "$(dirname "$CLAUDE_MD")"

CLAUDE_MD_CONTENT='# ContextKeeper Auto-Refresh

## Important: Context Recovery

When any of the following occur, call the `get_dev_context` MCP tool:
- Start of a new conversation
- After context compression
- When environment details are unclear
- When user mentions "build", "container", "target", or "environment"
- When you are unsure which container or build target to use

This ensures you always have accurate information about:
- Available build targets and their configurations
- Running containers
- Correct commands to use
'

if [ -f "$CLAUDE_MD" ]; then
    if ! grep -q "ContextKeeper" "$CLAUDE_MD"; then
        echo "" >> "$CLAUDE_MD"
        echo "$CLAUDE_MD_CONTENT" >> "$CLAUDE_MD"
        echo "  Added ContextKeeper instructions to existing CLAUDE.md"
    else
        echo "  CLAUDE.md already contains ContextKeeper instructions"
    fi
else
    echo "$CLAUDE_MD_CONTENT" > "$CLAUDE_MD"
    echo "  Created CLAUDE.md with ContextKeeper instructions"
fi

# 3. Show MCP configuration instructions
echo "[3/4] MCP Configuration"
echo ""
echo "Add the following to your project in ~/.claude.json:"
echo ""
echo '  "mcpServers": {'
echo '    "context-keeper": {'
echo '      "type": "stdio",'
echo "      \"command\": \"$SCRIPT_DIR/context-keeper-mcp.sh\","
echo '      "args": [],'
echo '      "env": {}'
echo '    }'
echo '  }'
echo ""

# 4. Create example contextkeeper.toml
echo "[4/4] Example configuration"
echo ""
echo "Create contextkeeper.toml in your project root:"
echo ""
cat << 'EOF'
[project]
name = "My Project"
type = "custom"

[scripts]
entry_point = "build.sh"
config_dir = "config"
config_pattern = "*.conf"

[containers]
runtime = "podman"

[hints]
default = "Build commands should run in the container."
EOF

echo ""
echo "=== Installation Complete ==="
echo ""
echo "Next steps:"
echo "1. Add MCP server config to ~/.claude.json (see above)"
echo "2. Create contextkeeper.toml in your project"
echo "3. Restart Claude Code"
echo "4. Verify with /mcp command"
