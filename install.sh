#!/usr/bin/env bash
set -euo pipefail

REPO="LuganoPlanB/swissledger-foundry"
INSTALL_DIR="${HOME}/.local/bin"
VERSION="${1:-latest}"

# --- detect platform ---
case "$(uname -s)" in
  Linux)  os=linux ;;
  Darwin) os=darwin ;;
  *)      echo "unsupported OS: $(uname -s)"; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64)  arch=x86_64 ;;
  arm64|aarch64) arch=arm64 ;;
  *)       echo "unsupported arch: $(uname -m)"; exit 1 ;;
esac

suffix="${os}-${arch}"

# --- resolve download URL ---
if [ "$VERSION" = "latest" ]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${VERSION}"
fi

# --- install ---
mkdir -p "$INSTALL_DIR"

echo "swissledger-foundry installer"
echo "  platform: ${suffix}"
echo "  version:  ${VERSION}"
echo "  dest:     ${INSTALL_DIR}"
echo ""

for tool in forge cast anvil chisel; do
  asset="swissledger-${tool}_${suffix}"
  url="${base}/${asset}"
  dest="${INSTALL_DIR}/swissledger-${tool}"

  echo "downloading ${asset}..."
  curl -fsSL --retry 3 --progress-bar "$url" -o "$dest"
  chmod +x "$dest"
done

echo ""
echo "installed:"
for tool in forge cast anvil chisel; do
  echo "  ${INSTALL_DIR}/swissledger-${tool}"
done

# --- PATH hint ---
if ! echo "$PATH" | tr ':' '\n' | grep -qxF "$INSTALL_DIR"; then
  echo ""
  echo "Add to your shell profile:"
  echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
fi
