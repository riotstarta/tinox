#!/usr/bin/env bash
# Builds tinox-lsp and installs it to ~/.cargo/bin/
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "Building tinox-lsp..."
cargo build --release -p tinox-lsp --manifest-path "$SCRIPT_DIR/Cargo.toml"

DEST="$HOME/.cargo/bin/tinox-lsp"
cp "$SCRIPT_DIR/target/release/tinox-lsp" "$DEST"
chmod +x "$DEST"

echo "Installed to $DEST"
echo "You can now configure the path in Eclipse: Window → Preferences → Tinox"
