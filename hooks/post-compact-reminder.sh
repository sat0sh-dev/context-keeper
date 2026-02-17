#!/bin/bash
# ContextKeeper: Post-Compression Recovery Reminder
# Triggered by SessionStart hook with "compact" matcher

echo ""
echo "=============================================="
echo " Context Compression Detected"
echo "=============================================="
echo ""
echo "Development environment context may be stale."
echo ""

# Quick status checks
if command -v podman &> /dev/null; then
    CONTAINERS=$(podman ps --format "{{.Names}}" 2>/dev/null | head -3)
    if [ -n "$CONTAINERS" ]; then
        echo "Active containers: $CONTAINERS"
    fi
fi

if command -v adb &> /dev/null; then
    DEVICES=$(adb devices 2>/dev/null | grep -v "^List" | grep -v "^$" | wc -l)
    if [ "$DEVICES" -gt 0 ]; then
        echo "ADB devices connected: $DEVICES"
    fi
fi

if command -v git &> /dev/null && git rev-parse --is-inside-work-tree &> /dev/null; then
    BRANCH=$(git branch --show-current 2>/dev/null)
    if [ -n "$BRANCH" ]; then
        echo "Git branch: $BRANCH"
    fi
fi

echo ""
echo ">>> Call 'get_dev_context' MCP tool to restore full context <<<"
echo ""

exit 0
