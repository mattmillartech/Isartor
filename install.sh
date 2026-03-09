#!/bin/sh
set -e

REPO="isartor-ai/Isartor"
INSTALL_DIR="/usr/local/bin"
BIN_NAME="isartor"

echo "Installing $BIN_NAME..."

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux*)     OS_NAME="linux" ;;
    Darwin*)    OS_NAME="macos" ;;
    *)          echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)        ARCH_NAME="amd64" ;;
    aarch64|arm64) ARCH_NAME="arm64" ;;
    *)             echo "Unsupported Architecture: $ARCH"; exit 1 ;;
esac

ARTIFACT_NAME="${BIN_NAME}-${OS_NAME}-${ARCH_NAME}"

# Fetch latest release data
LATEST_RELEASE_URL="https://api.github.com/repos/$REPO/releases/latest"
echo "Fetching latest release information..."
DOWNLOAD_URL=$(curl -s $LATEST_RELEASE_URL | grep "browser_download_url.*$ARTIFACT_NAME" | cut -d '"' -f 4)

if [ -z "$DOWNLOAD_URL" ]; then
    echo "Could not find a release artifact for $OS_NAME $ARCH_NAME"
    exit 1
fi

echo "Downloading from $DOWNLOAD_URL..."
TMP_FILE="$(mktemp)"
curl -L -# "$DOWNLOAD_URL" -o "$TMP_FILE"

echo "Installing to $INSTALL_DIR/$BIN_NAME..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$TMP_FILE" "$INSTALL_DIR/$BIN_NAME"
    chmod +x "$INSTALL_DIR/$BIN_NAME"
else
    echo "Requires sudo permissions to write to $INSTALL_DIR"
    sudo mv "$TMP_FILE" "$INSTALL_DIR/$BIN_NAME"
    sudo chmod +x "$INSTALL_DIR/$BIN_NAME"
fi

echo ""
echo "✅ $BIN_NAME installed successfully!"
echo "Run '$BIN_NAME' to start."
