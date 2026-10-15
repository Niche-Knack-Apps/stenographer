#!/bin/bash
set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"
APP_NAME="stenographer"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
RELEASES_DIR="../_shared/releases/$APP_NAME"
mkdir -p "$RELEASES_DIR/linux"
cp "target/release/stenographerd" "$RELEASES_DIR/linux/" 2>/dev/null || echo "stenographerd binary not found."
cp "target/release/steno" "$RELEASES_DIR/linux/" 2>/dev/null || echo "steno binary not found."
echo "Build first with: cargo build --release (if binaries missing)"
echo "Release artifacts in $RELEASES_DIR:"
ls -lh "$RELEASES_DIR/linux/" 2>/dev/null || echo "  (none)"
