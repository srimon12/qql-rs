#!/usr/bin/env sh
# QQL CLI Installer Script for Linux and macOS
# Usage:
#   curl -fsSL https://qql.veristamp.in/install.sh | bash
#   curl -fsSL https://qql.veristamp.in/install.sh | bash -s -- --full
#   curl -fsSL https://qql.veristamp.in/install.sh | bash -s -- --version 0.4.0

set -e

REPO="srimon12/qql-rs"
BINARY_NAME="qql"
EDITION="standard"
VERSION="${QQL_VERSION:-}"
CUSTOM_INSTALL_DIR="${QQL_INSTALL_DIR:-}"

# Parse optional command line flags
while [ $# -gt 0 ]; do
  case "$1" in
    --full)
      EDITION="full"
      shift
      ;;
    --standard)
      EDITION="standard"
      shift
      ;;
    --version|-v)
      VERSION="$2"
      shift 2
      ;;
    --dir|-d)
      CUSTOM_INSTALL_DIR="$2"
      shift 2
      ;;
    --help|-h)
      echo "QQL CLI Installer"
      echo ""
      echo "Usage: install.sh [options]"
      echo ""
      echo "Options:"
      echo "  --full            Install full edition (Standard + ONNX embeddings + embedded edge)"
      echo "  --standard        Install standard edition (default: lean REST/gRPC/record/convert/migrate)"
      echo "  --version, -v <v> Install a specific version (e.g. 0.4.0)"
      echo "  --dir, -d <path>  Install binary to custom directory (default: /usr/local/bin or ~/.local/bin)"
      echo "  --help, -h        Show this help message"
      exit 0
      ;;
    *)
      echo "⚠️ Unknown argument: $1"
      shift
      ;;
  esac
done

if [ -n "$QQL_EDITION" ]; then
  EDITION="$QQL_EDITION"
fi

echo "🔍 Detecting system architecture and OS..."

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
  linux*)   OS="unknown-linux-gnu" ;;
  darwin*)  OS="apple-darwin" ;;
  *)        echo "❌ Unsupported OS: $OS (QQL prebuilt binaries support Linux and macOS)"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)   ARCH="x86_64" ;;
  aarch64|arm64)  ARCH="aarch64" ;;
  *)              echo "❌ Unsupported Architecture: $ARCH (QQL supports x86_64 and aarch64/arm64)"; exit 1 ;;
esac

TARGET="${ARCH}-${OS}"
echo "✨ Target platform: ${TARGET}"

case "$TARGET" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin|aarch64-unknown-linux-gnu) ;;
  *)
    echo "❌ Unsupported target platform: ${TARGET}"
    exit 1
    ;;
esac

if [ -z "$VERSION" ]; then
  echo "📡 Fetching latest release version..."
  # First try non-rate-limited GitHub location redirect header
  VERSION=$(curl -sI "https://github.com/${REPO}/releases/latest" 2>/dev/null | grep -i "^location:" | sed -E 's/.*\/tag\/v?([^ \r\n]+).*/\1/' || true)
  # Fallback to API if redirect did not return a tag
  if [ -z "$VERSION" ]; then
    VERSION=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
  fi
  if [ -z "$VERSION" ]; then
    echo "❌ Could not determine latest release version from GitHub."
    echo "   Set QQL_VERSION explicitly (e.g. QQL_VERSION=0.4.0 sh install.sh) and retry."
    exit 1
  fi
fi

# Clean up version string
VERSION_NUM="$(echo "$VERSION" | sed 's/^v//')"
TAG_NAME="v${VERSION_NUM}"

PREFIX="qql"
if [ "$EDITION" = "full" ]; then
  PREFIX="qql-full"
fi

TARBALL_NAME="${PREFIX}-${VERSION_NUM}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${TAG_NAME}/${TARBALL_NAME}"

echo "📦 Downloading QQL ${EDITION} edition (${TARBALL_NAME})..."
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

if ! curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${TARBALL_NAME}"; then
  echo "❌ Download failed from: ${DOWNLOAD_URL}"
  echo "   Please check if release ${TAG_NAME} exists for ${TARGET}."
  exit 1
fi

tar -xzf "${TMP_DIR}/${TARBALL_NAME}" -C "${TMP_DIR}"

BINARY_PATH="$(find "${TMP_DIR}" -type f -name "${BINARY_NAME}" | head -n 1)"
if [ -z "$BINARY_PATH" ]; then
  echo "❌ Binary '${BINARY_NAME}' not found inside archive."
  exit 1
fi

if [ -n "$CUSTOM_INSTALL_DIR" ]; then
  INSTALL_DIR="$CUSTOM_INSTALL_DIR"
  mkdir -p "$INSTALL_DIR"
elif [ -w "/usr/local/bin" ]; then
  INSTALL_DIR="/usr/local/bin"
elif [ "$(id -u)" -eq 0 ]; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="${HOME}/.local/bin"
  mkdir -p "$INSTALL_DIR"
fi

cp -f "${BINARY_PATH}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

echo "✅ Successfully installed qql (${EDITION} edition) to ${INSTALL_DIR}/${BINARY_NAME}"

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "⚠️ Notice: ${INSTALL_DIR} is not in your PATH."
    echo "   Add it by running:"
    echo "     export PATH=\"${INSTALL_DIR}:\$PATH\""
    echo "   or append that line to your ~/.bashrc or ~/.zshrc."
    ;;
esac

echo ""
echo "🚀 Try running:"
echo "   qql version"
echo "   qql setup"
echo "   qql --help"
