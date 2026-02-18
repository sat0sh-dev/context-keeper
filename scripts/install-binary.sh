#!/bin/bash
# ContextKeeper Binary Installer
# Usage: curl -sSL https://raw.githubusercontent.com/sat0sh-dev/context-keeper/main/scripts/install-binary.sh | bash

set -e

REPO="sat0sh-dev/context-keeper"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
CONTEXTKEEPER_DIR="$HOME/.contextkeeper"

echo "=== ContextKeeper Installer ==="
echo ""

# Detect OS and architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="macos" ;;
        *)       echo "Unsupported OS: $(uname -s)"; exit 1 ;;
    esac

    case "$(uname -m)" in
        x86_64)  arch="x86_64" ;;
        aarch64) arch="aarch64" ;;
        arm64)   arch="aarch64" ;;
        *)       echo "Unsupported architecture: $(uname -m)"; exit 1 ;;
    esac

    echo "${os}-${arch}"
}

PLATFORM=$(detect_platform)
echo "[1/4] Detected platform: $PLATFORM"

# Get latest release URL
echo "[2/4] Fetching latest release..."
DOWNLOAD_URL=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | \
    grep "browser_download_url.*$PLATFORM" | \
    cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Error: Could not find binary for $PLATFORM"
    echo "Available releases: https://github.com/$REPO/releases"
    exit 1
fi

echo "  Download URL: $DOWNLOAD_URL"

# Download and extract
echo "[3/4] Downloading and installing..."
mkdir -p "$INSTALL_DIR"
mkdir -p "$CONTEXTKEEPER_DIR"

TEMP_DIR=$(mktemp -d)
trap "rm -rf $TEMP_DIR" EXIT

curl -sL "$DOWNLOAD_URL" -o "$TEMP_DIR/context-keeper.tar.gz"
tar xzf "$TEMP_DIR/context-keeper.tar.gz" -C "$TEMP_DIR"
mv "$TEMP_DIR/context-keeper" "$INSTALL_DIR/context-keeper"
chmod +x "$INSTALL_DIR/context-keeper"

echo "  Installed to: $INSTALL_DIR/context-keeper"

# Check if in PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo ""
    echo "  NOTE: Add $INSTALL_DIR to your PATH:"
    echo "    export PATH=\"\$PATH:$INSTALL_DIR\""
    echo ""
fi

# Show next steps
echo "[4/4] Installation complete!"
echo ""
echo "=== Next Steps ==="
echo ""
echo "1. Initialize your project:"
echo "   cd /your/project"
echo "   context-keeper init"
echo ""
echo "2. Add MCP server to ~/.claude.json:"
echo '   {'
echo '     "projects": {'
echo '       "/your/project": {'
echo '         "mcpServers": {'
echo '           "context-keeper": {'
echo '             "type": "stdio",'
echo "             \"command\": \"$INSTALL_DIR/context-keeper\","
echo '             "args": []'
echo '           }'
echo '         }'
echo '       }'
echo '     }'
echo '   }'
echo ""
echo "3. Restart Claude Code and verify with /mcp"
echo ""
echo "Documentation: https://github.com/$REPO"
