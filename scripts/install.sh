#!/usr/bin/env bash
# Isartor Installer Script for macOS & Linux
# Fetches the latest release from GitHub and installs the correct binary

set -e

ORG="isartor-ai"
REPO="isartor"
API_URL="https://api.github.com/repos/$ORG/$REPO/releases/latest"
INSTALL_DIR="/usr/local/bin"
LOCAL_BIN="$HOME/.local/bin"

# Colors
GREEN="\033[1;32m"
YELLOW="\033[1;33m"
RED="\033[1;31m"
BLUE="\033[1;34m"
RESET="\033[0m"

function info() { echo -e "${BLUE}[INFO]${RESET} $1"; }
function success() { echo -e "${GREEN}[SUCCESS]${RESET} $1"; }
function warn() { echo -e "${YELLOW}[WARN]${RESET} $1"; }
function error() { echo -e "${RED}[ERROR]${RESET} $1"; }

info "Detecting OS and architecture..."
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        PLATFORM="unknown-linux-musl";;
    Darwin)
        PLATFORM="apple-darwin";;
    *)
        error "Unsupported OS: $OS"; exit 1;;
esac

case "$ARCH" in
    x86_64|amd64)
        ARCH_NAME="x86_64";;
    arm64|aarch64)
        ARCH_NAME="aarch64";;
    *)
        error "Unsupported architecture: $ARCH"; exit 1;;
esac

ASSET_NAME="${ARCH_NAME}-${PLATFORM}.tar.gz"

info "Fetching latest release info from GitHub..."
RELEASE_JSON=$(curl -fsSL "$API_URL")
TAG=$(echo "$RELEASE_JSON" | grep '"tag_name"' | head -1 | cut -d '"' -f4)
ASSET_URL=$(echo "$RELEASE_JSON" | grep 'browser_download_url' | grep "$ASSET_NAME" | cut -d '"' -f4)

if [ -z "$ASSET_URL" ]; then
    error "Could not find a release asset for $ASSET_NAME"; exit 1
fi

info "Downloading $ASSET_NAME..."
TMP_DIR=$(mktemp -d)
cd "$TMP_DIR"
curl -fsSL -o "$ASSET_NAME" "$ASSET_URL"

info "Extracting..."
tar -xzf "$ASSET_NAME"

if [ ! -f isartor ]; then
    error "isartor binary not found in archive."; exit 1
fi

# Try to install to /usr/local/bin, fallback to ~/.local/bin
if [ -w "$INSTALL_DIR" ]; then
    DEST="$INSTALL_DIR"
else
    warn "$INSTALL_DIR not writable, using $LOCAL_BIN instead. Add it to your PATH if needed."
    mkdir -p "$LOCAL_BIN"
    DEST="$LOCAL_BIN"
fi

info "Installing isartor to $DEST..."
mv isartor "$DEST/isartor"
chmod +x "$DEST/isartor"

cd ~
rm -rf "$TMP_DIR"

success "Isartor installed!"
echo -e "\nRun: ${GREEN}isartor --version${RESET} to verify installation."
