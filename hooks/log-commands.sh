#!/bin/bash
# ContextKeeper Command Logger
# Logs Bash commands executed by Claude Code for context recovery

LOG_FILE="${CONTEXTKEEPER_LOG:-$HOME/.contextkeeper/command-history.jsonl}"
LOG_DIR=$(dirname "$LOG_FILE")

# Ensure log directory exists
mkdir -p "$LOG_DIR"

# Read JSON input from stdin
INPUT=$(cat)

# Extract fields
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
TIMESTAMP=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')

# Skip if no command
if [ -z "$COMMAND" ]; then
    exit 0
fi

# Write as JSONL (one JSON object per line)
jq -n -c \
    --arg ts "$TIMESTAMP" \
    --arg cmd "$COMMAND" \
    --arg cwd "$CWD" \
    --arg session "$SESSION_ID" \
    '{timestamp: $ts, command: $cmd, cwd: $cwd, session_id: $session}' >> "$LOG_FILE"

# Rotate log if too large (>1MB)
if [ -f "$LOG_FILE" ] && [ $(stat -f%z "$LOG_FILE" 2>/dev/null || stat -c%s "$LOG_FILE" 2>/dev/null) -gt 1048576 ]; then
    tail -n 500 "$LOG_FILE" > "$LOG_FILE.tmp"
    mv "$LOG_FILE.tmp" "$LOG_FILE"
fi

exit 0
