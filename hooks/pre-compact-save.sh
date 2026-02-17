#!/bin/bash
# PreCompact Hook: Save work state before context compression
#
# This script is executed by Claude Code before context compression.
# It saves the current git state so it can be recovered after compression.
#
# Install in ~/.claude/settings.json:
# {
#   "hooks": {
#     "PreCompact": [{
#       "matcher": "*",
#       "hooks": [{
#         "type": "command",
#         "command": "bash ~/.contextkeeper/hooks/pre-compact-save.sh"
#       }]
#     }]
#   }
# }

set -e

# Configuration
CONTEXTKEEPER_DIR="${HOME}/.contextkeeper"
WORK_STATE_FILE="${CONTEXTKEEPER_DIR}/work-state.json"

# Ensure directory exists
mkdir -p "${CONTEXTKEEPER_DIR}"

# Get current working directory (where Claude Code is running)
CWD="${PWD}"

# Collect working files from all git repos
collect_working_files() {
    find "${CWD}" -maxdepth 3 -name '.git' -type d 2>/dev/null | while read gitdir; do
        repo=$(dirname "$gitdir")
        git -C "$repo" diff --name-only 2>/dev/null | while read file; do
            # Output relative path
            rel_repo="${repo#${CWD}/}"
            echo "${rel_repo}/${file}"
        done
    done | head -20
}

# Get timestamp
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Collect files
FILES_JSON=$(collect_working_files | jq -R -s -c 'split("\n") | map(select(length > 0))')

# Create work state JSON
cat > "${WORK_STATE_FILE}" << EOF
{
  "saved_at": "${TIMESTAMP}",
  "trigger": "pre_compact",
  "task_summary": "",
  "working_files": ${FILES_JSON},
  "notes": "Auto-saved before context compression",
  "todos": []
}
EOF

# Output message (will be shown to Claude)
echo "[ContextKeeper] Work state saved before compression:" >&2
echo "  - Files tracked: $(echo "${FILES_JSON}" | jq 'length')" >&2
echo "  - Saved at: ${TIMESTAMP}" >&2
echo "  - Use get_dev_context('minimal') to recover after compression" >&2
