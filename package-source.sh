#!/usr/bin/env sh
set -eu
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUTPUT=${1:-"$SCRIPT_DIR/everything-source.zip"}
exec python3 "$SCRIPT_DIR/scripts/package_source.py" --output "$OUTPUT"
