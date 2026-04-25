#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$ROOT/kaspa-miner"
DEST_DIR="$HOME/.local/bin"
DEST="$DEST_DIR/kaspa-miner"

if [[ ! -f "$BIN" ]]; then
  echo "kaspa-miner binary not found next to install-macos.sh"
  exit 1
fi

chmod +x "$BIN"

if command -v xattr >/dev/null 2>&1; then
  xattr -dr com.apple.quarantine "$ROOT" 2>/dev/null || true
fi

mkdir -p "$DEST_DIR"
cp "$BIN" "$DEST"
chmod +x "$DEST"

echo "KASPilot installed at $DEST"
echo "Add this to your shell profile if needed:"
echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
"$DEST" --version
