#!/usr/bin/env bash
# setup_onnxruntime.sh
# Cross-platform ONNX Runtime setup script for Isartor local embedding/SLM
# Usage: source ./setup_onnxruntime.sh [platform] [version]
# Example: source ./setup_onnxruntime.sh macos-arm64 1.18.0

set -e

PLATFORM=${1:-"auto"}
VERSION=${2:-"1.18.0"}

# Detect platform if not specified
detect_platform() {
  unameOut="$(uname -s)"
  archOut="$(uname -m)"
  case "${unameOut}" in
    Linux*)     sys=linux;;
    Darwin*)    sys=macos;;
    MINGW*|MSYS*|CYGWIN*) sys=windows;;
    *)          sys="unknown";;
  esac
  case "${archOut}" in
    x86_64*)    arch=x64;;
    arm64*)     arch=arm64;;
    aarch64*)   arch=arm64;;
    *)          arch="unknown";;
  esac
  if [[ "$sys" == "macos" && "$arch" == "arm64" ]]; then
    echo "macos-arm64"
  elif [[ "$sys" == "linux" && "$arch" == "x64" ]]; then
    echo "linux-x64"
  elif [[ "$sys" == "windows" && "$arch" == "x64" ]]; then
    echo "win-x64"
  else
    echo "unsupported"
  fi
}

if [[ "$PLATFORM" == "auto" ]]; then
  PLATFORM=$(detect_platform)
fi

case "$PLATFORM" in
  macos-arm64)
    FILE="onnxruntime-osx-arm64-${VERSION}.tgz"
    URL="https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}/${FILE}"
    DIR="onnxruntime-osx-arm64-${VERSION}"
    ;;
  linux-x64)
    FILE="onnxruntime-linux-x64-${VERSION}.tgz"
    URL="https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}/${FILE}"
    DIR="onnxruntime-linux-x64-${VERSION}"
    ;;
  win-x64)
    FILE="onnxruntime-win-x64-${VERSION}.zip"
    URL="https://github.com/microsoft/onnxruntime/releases/download/v${VERSION}/${FILE}"
    DIR="onnxruntime-win-x64-${VERSION}"
    ;;
  *)
    echo "Unsupported or unknown platform: $PLATFORM"
    exit 1
    ;;
esac

echo "Downloading $URL ..."
curl -L -o "$FILE" "$URL"

if [[ "$PLATFORM" == win-x64 ]]; then
  unzip -q "$FILE"
else
  tar -xzf "$FILE"
fi

export ORT_STRATEGY=system
export ORT_LIB_LOCATION="$PWD/$DIR/lib"

if [[ "$PLATFORM" == "macos-arm64" ]]; then
  export DYLD_LIBRARY_PATH="$ORT_LIB_LOCATION:$DYLD_LIBRARY_PATH"
  echo "export ORT_STRATEGY=system"
  echo "export ORT_LIB_LOCATION=\"$PWD/$DIR/lib\""
  echo "export DYLD_LIBRARY_PATH=\"$ORT_LIB_LOCATION:[0m$DYLD_LIBRARY_PATH\""
elif [[ "$PLATFORM" == "linux-x64" ]]; then
  export LD_LIBRARY_PATH="$ORT_LIB_LOCATION:$LD_LIBRARY_PATH"
  echo "export ORT_STRATEGY=system"
  echo "export ORT_LIB_LOCATION=\"$PWD/$DIR/lib\""
  echo "export LD_LIBRARY_PATH=\"$ORT_LIB_LOCATION:[0m$LD_LIBRARY_PATH\""
elif [[ "$PLATFORM" == "win-x64" ]]; then
  echo "Set the following in your PowerShell session:"
  echo "$env:ORT_STRATEGY=system"
  echo "$env:ORT_LIB_LOCATION=\"$PWD\\$DIR\\lib\""
  echo "$env:PATH=\"[0m$PWD\\$DIR\\lib;$env:PATH\""
fi

echo "ONNX Runtime $VERSION setup complete for $PLATFORM."
echo "You may want to add the export lines above to your shell profile."
