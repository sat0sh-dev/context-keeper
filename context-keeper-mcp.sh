#!/bin/bash
# ContextKeeper MCP Server Wrapper
# Handles MCP protocol in shell, calls Rust binary for context collection

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BINARY="$SCRIPT_DIR/target/release/context-keeper"

# Fallback to debug binary if release doesn't exist
if [ ! -f "$BINARY" ]; then
    BINARY="$SCRIPT_DIR/target/debug/context-keeper"
fi

while IFS= read -r line; do
    method=$(echo "$line" | jq -r '.method // empty')
    id=$(echo "$line" | jq -r '.id // empty')

    case "$method" in
        "initialize")
            echo '{"jsonrpc":"2.0","result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"context-keeper","version":"0.1.0"}},"id":'$id'}'
            ;;
        "tools/list")
            echo '{"jsonrpc":"2.0","result":{"tools":[{"name":"get_dev_context","description":"Get current development environment context including build targets, containers, and configuration. Call this when context is unclear or after context compression.","inputSchema":{"type":"object","properties":{}}}]},"id":'$id'}'
            ;;
        "tools/call")
            tool_name=$(echo "$line" | jq -r '.params.name // empty')
            if [ "$tool_name" = "get_dev_context" ]; then
                # Run context-keeper binary and capture output
                if [ -f "$BINARY" ]; then
                    context=$("$BINARY" 2>/dev/null | jq -Rs .)
                else
                    context='"ContextKeeper binary not found. Run: cargo build --release"'
                fi
                echo '{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":'$context'}]},"id":'$id'}'
            fi
            ;;
    esac
done
