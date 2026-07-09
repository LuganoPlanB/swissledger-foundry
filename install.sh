#!/usr/bin/env bash
set -euo pipefail

REPO="LuganoPlanB/swissledger-foundry"
TOOLS=(forge cast anvil chisel)
VERSION="${1:-latest}"

# --- detect platform ---
OS_NAME="$(uname -s)"
ARCH_NAME="$(uname -m)"

case "$OS_NAME" in
  Linux)  os=linux ;;
  Darwin) os=darwin ;;
  *)      echo "ERROR: unsupported OS: $OS_NAME"; exit 1 ;;
esac

case "$ARCH_NAME" in
  x86_64)        arch=x86_64 ;;
  arm64|aarch64) arch=arm64 ;;
  *)             echo "ERROR: unsupported architecture: $ARCH_NAME"; exit 1 ;;
esac

suffix="${os}-${arch}"

# --- pick install directory ---
if [ "$(id -u)" -eq 0 ]; then
  INSTALL_DIR="/usr/local/bin"
  echo "running as root → installing to ${INSTALL_DIR}"
elif [ -d "$HOME/bin" ]; then
  INSTALL_DIR="$HOME/bin"
  echo "found ~/bin → installing to ${INSTALL_DIR}"
else
  INSTALL_DIR="$HOME/.local/bin"
  echo "default → installing to ${INSTALL_DIR}"
fi

mkdir -p "$INSTALL_DIR"

# --- resolve download URL ---
if [ "$VERSION" = "latest" ]; then
  RELEASE_URL="https://github.com/${REPO}/releases/latest"
  DOWNLOAD_URL="${RELEASE_URL}/download"
  echo ""
  echo "resolving latest release..."
  RESOLVED=$(curl -fsSL -o /dev/null -w '%{url_effective}' "$RELEASE_URL")
  RESOLVED_TAG="${RESOLVED##*/}"
  echo "  latest = ${RESOLVED_TAG}"
else
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}"
  RESOLVED_TAG="$VERSION"
  echo ""
  echo "using pinned version: ${VERSION}"
fi

# --- download and install ---
echo ""
echo "platform: ${suffix}"
echo "version:  ${RESOLVED_TAG}"
echo "dest:     ${INSTALL_DIR}"
echo ""

DOWNLOADED=0
FAILED=0

for tool in "${TOOLS[@]}"; do
  asset="swissledger-${tool}_${suffix}"
  url="${DOWNLOAD_URL}/${asset}"
  dest="${INSTALL_DIR}/swissledger-${tool}"

  echo -n "  ${tool}"
  if curl -fsSL --retry 3 --progress-bar "$url" -o "$dest"; then
    chmod +x "$dest"
    echo "  ✔  ${dest}"
    DOWNLOADED=$((DOWNLOADED + 1))
  else
    echo "  ✘  failed to download ${asset}"
    FAILED=$((FAILED + 1))
  fi
done

# --- summary ---
echo ""
echo "---"
echo "installed ${DOWNLOADED} / ${#TOOLS[@]} tools → ${INSTALL_DIR}"
if [ "$FAILED" -gt 0 ]; then
  echo "failed  ${FAILED} — check that release ${RESOLVED_TAG} has ${suffix} assets"
  exit 1
fi

# --- PATH hint ---
if ! echo "$PATH" | tr ':' '\n' | grep -qxF "$INSTALL_DIR"; then
  echo ""
  echo "⚠  ${INSTALL_DIR} is not in your PATH."
  echo "   Add this to your shell profile (~/.bashrc, ~/.zshrc):"
  echo ""
  echo "     export PATH=\"${INSTALL_DIR}:\$PATH\""
  echo ""
fi

echo ""
echo "try: swissledger-forge --help"
