#!/bin/bash
# ContextKeeper: Track recently edited files
# This hook captures Edit/Write tool calls and maintains a list of
# recently modified files for context recovery.

CONTEXTKEEPER_DIR="${HOME}/.contextkeeper"
FILES_FILE="${CONTEXTKEEPER_DIR}/recent-files.json"
MAX_FILES=20

# Ensure directory exists
mkdir -p "$CONTEXTKEEPER_DIR"

# Extract file path from tool input
if [ -n "$CLAUDE_TOOL_INPUT" ]; then
    FILE_PATH=$(echo "$CLAUDE_TOOL_INPUT" | jq -r '.file_path // .path // empty' 2>/dev/null)

    if [ -n "$FILE_PATH" ]; then
        TIMESTAMP=$(date -Iseconds)

        # Initialize file if it doesn't exist
        if [ ! -f "$FILES_FILE" ]; then
            echo '{"files": []}' > "$FILES_FILE"
        fi

        # Add new file and keep only recent entries
        jq --arg path "$FILE_PATH" \
           --arg time "$TIMESTAMP" \
           --argjson max "$MAX_FILES" \
           '.files = ([{path: $path, time: $time}] + [.files[] | select(.path != $path)])[0:$max]' \
           "$FILES_FILE" > "${FILES_FILE}.tmp" 2>/dev/null && \
           mv "${FILES_FILE}.tmp" "$FILES_FILE"
    fi
fi
