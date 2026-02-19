#!/bin/bash
# ContextKeeper Installation Script

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CLAUDE_SETTINGS="$HOME/.claude/settings.json"
CLAUDE_MD="$HOME/.claude/CLAUDE.md"
LOG_DIR="$HOME/.contextkeeper"

echo "=== ContextKeeper Installer ==="
echo ""

# 1. Build if needed
if [ ! -f "$SCRIPT_DIR/target/release/context-keeper" ]; then
    echo "[1/5] Building ContextKeeper..."
    cd "$SCRIPT_DIR"
    cargo build --release
else
    echo "[1/5] Binary already exists, skipping build"
fi

# 2. Setup command logging directory
echo "[2/5] Setting up command logging..."
mkdir -p "$LOG_DIR"
echo "  Created $LOG_DIR"

# 3. Setup Claude Code Hooks for command logging
echo "[3/5] Configuring Claude Code Hooks..."

mkdir -p "$(dirname "$CLAUDE_SETTINGS")"

HOOKS_CONFIG='{
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "'"$SCRIPT_DIR"'/hooks/log-commands.sh"
          }
        ]
      },
      {
        "matcher": "TodoWrite",
        "hooks": [
          {
            "type": "command",
            "command": "'"$SCRIPT_DIR"'/hooks/save-todos.sh"
          }
        ]
      },
      {
        "matcher": "Edit",
        "hooks": [
          {
            "type": "command",
            "command": "'"$SCRIPT_DIR"'/hooks/track-files.sh"
          }
        ]
      },
      {
        "matcher": "Write",
        "hooks": [
          {
            "type": "command",
            "command": "'"$SCRIPT_DIR"'/hooks/track-files.sh"
          }
        ]
      }
    ]
  }
}'

if [ -f "$CLAUDE_SETTINGS" ]; then
    # Check if hooks already configured
    if grep -q "log-commands.sh" "$CLAUDE_SETTINGS" 2>/dev/null; then
        echo "  Hooks already configured in settings.json"
    else
        echo "  NOTE: Please manually add hooks to $CLAUDE_SETTINGS"
        echo ""
        echo "$HOOKS_CONFIG"
        echo ""
    fi
else
    echo "$HOOKS_CONFIG" > "$CLAUDE_SETTINGS"
    echo "  Created $CLAUDE_SETTINGS with hooks configuration"
fi

# 4. Add to CLAUDE.md for auto-refresh
echo "[4/5] Setting up auto-refresh in CLAUDE.md..."

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
- Recent relevant commands (lunch targets, environment setup)
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

# 5. Show MCP configuration instructions
echo "[5/5] MCP Configuration"
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

echo "=== Example contextkeeper.toml ==="
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

[history]
enabled = true
patterns = [
    "lunch\\s+\\S+",
    "source.*envsetup",
    "export\\s+\\w+=",
]
max_entries = 20
EOF

echo ""
echo "=== Installation Complete ==="
echo ""
echo "Next steps:"
echo "1. Add MCP server config to ~/.claude.json (see above)"
echo "2. Create contextkeeper.toml in your project"
echo "3. Restart Claude Code"
echo "4. Verify with /mcp command"
echo ""
echo "Command history will be logged automatically via Claude Code hooks."
