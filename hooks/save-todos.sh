#!/bin/bash
# ContextKeeper: Save TodoWrite state for context recovery
# This hook captures Claude's TodoWrite calls and saves the current todos
# for automatic recovery after context compression.

CONTEXTKEEPER_DIR="${HOME}/.contextkeeper"
TODOS_FILE="${CONTEXTKEEPER_DIR}/current-todos.json"

# Ensure directory exists
mkdir -p "$CONTEXTKEEPER_DIR"

# Only process if this is a TodoWrite call
if [ -n "$CLAUDE_TOOL_INPUT" ]; then
    # Save the todos with timestamp
    jq -n \
        --argjson todos "$CLAUDE_TOOL_INPUT" \
        --arg timestamp "$(date -Iseconds)" \
        '{saved_at: $timestamp, todos: $todos.todos}' \
        > "$TODOS_FILE" 2>/dev/null

    # Fallback if jq fails or isn't available
    if [ $? -ne 0 ]; then
        echo "{\"saved_at\": \"$(date -Iseconds)\", \"raw_input\": $CLAUDE_TOOL_INPUT}" > "$TODOS_FILE"
    fi
fi
