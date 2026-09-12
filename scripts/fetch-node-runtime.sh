#!/usr/bin/env bash
# Laedt eine portable Node.js-Laufzeit fuer eine Plattform/Architektur nach
# app/sidecar/node-runtime/<zielordner>/ -- damit die gebaute App eine
# eigene Node.js-Laufzeit mitbringt und Nutzer keine eigene installieren
# muessen (nur Google Chrome bleibt Voraussetzung).
#
# Nutzung: fetch-node-runtime.sh <node-plattform> <zielordner>
#   node-plattform: darwin-arm64 | darwin-x64 | linux-x64 | win-x64
#                   (Node.js' eigene Bezeichnung, siehe nodejs.org/dist)
#   zielordner:     macos-arm64 | macos-x64 | linux-x64 | windows-x64
#                   (unsere eigene Ordnerbezeichnung, siehe
#                   src-tauri/src/lib.rs::bundled_node_path)
set -euo pipefail

NODE_VERSION="20.18.1"
NODE_PLATFORM="$1"
TARGET_DIR="$2"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/app/sidecar/node-runtime/$TARGET_DIR"
mkdir -p "$DEST"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [[ "$NODE_PLATFORM" == win-* ]]; then
  URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-${NODE_PLATFORM}.zip"
  curl -fsSL "$URL" -o "$WORK/node.zip"
  unzip -q "$WORK/node.zip" -d "$WORK"
  cp "$WORK/node-v${NODE_VERSION}-${NODE_PLATFORM}/node.exe" "$DEST/node.exe"
else
  URL="https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-${NODE_PLATFORM}.tar.gz"
  curl -fsSL "$URL" -o "$WORK/node.tar.gz"
  tar -xzf "$WORK/node.tar.gz" -C "$WORK"
  cp "$WORK/node-v${NODE_VERSION}-${NODE_PLATFORM}/bin/node" "$DEST/node"
  chmod +x "$DEST/node"
fi

echo "Node.js $NODE_VERSION ($NODE_PLATFORM) -> $DEST"
