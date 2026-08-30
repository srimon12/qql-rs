#!/usr/bin/env sh
# QQL CLI Installer Script for Linux and macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/srimon12/qql-rs/main/scripts/install.sh | sh

set -e

REPO="srimon12/qql-rs"
BINARY_NAME="qql"

echo "🔍 Detecting system architecture and OS..."

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux*)   OS="unknown-linux-gnu" ;;
  darwin*)  OS="apple-darwin" ;;
  *)        echo "❌ Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)   ARCH="x86_64" ;;
  aarch64|arm64)  ARCH="aarch64" ;;
  *)              echo "❌ Unsupported Architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"
echo "✨ Target platform: ${TARGET}"

case "$TARGET" in
  x86_64-unknown-linux-gnu|x86_64-apple-darwin|aarch64-apple-darwin) ;;
  aarch64-unknown-linux-gnu)
    echo "❌ Pre-built binaries are not yet published for ${TARGET}."
    echo "   You can build from source using: cargo install qql-cli --locked"
    exit 1
    ;;
  *)
    echo "❌ Unsupported target platform: ${TARGET}"
    exit 1
    ;;
esac

VERSION="${QQL_VERSION}"
if [ -z "$VERSION" ]; then
  echo "📡 Fetching latest release version..."
  VERSION=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
  if [ -z "$VERSION" ]; then
    VERSION="v0.3.0"
  fi
fi

# Ensure VERSION has 'v' prefix for URL, and strip for filename
TAG_NAME="${VERSION}"
VERSION_NUM="$(echo "$VERSION" | sed 's/^v//')"

TARBALL_NAME="qql-${VERSION_NUM}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG_NAME}/${TARBALL_NAME}"

echo "📦 Downloading QQL CLI ${TAG_NAME} (${TARBALL_NAME})..."
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${TARBALL_NAME}"; then
  echo "❌ Download failed! Check if release ${TAG_NAME} exists for target ${TARGET}."
  exit 1
fi

tar -xzf "${TMP_DIR}/${TARBALL_NAME}" -C "${TMP_DIR}"

BINARY_PATH="$(find "${TMP_DIR}" -type f -name "${BINARY_NAME}" | head -n 1)"
if [ -z "$BINARY_PATH" ]; then
  echo "❌ Binary '${BINARY_NAME}' not found inside archive."
  exit 1
fi

INSTALL_DIR="/usr/local/bin"
if [ ! -w "$INSTALL_DIR" ]; then
  INSTALL_DIR="${HOME}/.qql/bin"
  mkdir -p "$INSTALL_DIR"
fi

mv "${BINARY_PATH}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "✅ Successfully installed qql to ${INSTALL_DIR}/${BINARY_NAME}"

if [ "$INSTALL_DIR" = "${HOME}/.qql/bin" ]; then
  case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      echo ""
      echo "⚠️ Please add ${INSTALL_DIR} to your PATH:"
      echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
      ;;
  esac
fi

echo "🚀 Try running: qql --version"
