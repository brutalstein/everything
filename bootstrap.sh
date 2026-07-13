#!/usr/bin/env bash
set -euo pipefail

REPOSITORY="${EVERYTHING_GITHUB_REPOSITORY:-brutalstein/everything}"
RELEASE_BASE="${EVERYTHING_RELEASE_BASE_URL:-https://github.com/$REPOSITORY/releases/latest/download}"
ASSET="everything-source.zip"
CHECKSUM="$ASSET.sha256"
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/everything-bootstrap.XXXXXX")
cleanup() { rm -rf "$TMP_ROOT"; }
trap cleanup EXIT INT TERM

command -v curl >/dev/null 2>&1 || {
  printf '%s\n' "Everything bootstrap requires curl." >&2
  exit 1
}

printf '[everything] Downloading the latest verified source release from %s\n' "$REPOSITORY"
curl --fail --location --retry 4 --retry-all-errors --connect-timeout 20 \
  --proto '=https' --tlsv1.2 --output "$TMP_ROOT/$ASSET" "$RELEASE_BASE/$ASSET"
curl --fail --location --retry 4 --retry-all-errors --connect-timeout 20 \
  --proto '=https' --tlsv1.2 --output "$TMP_ROOT/$CHECKSUM" "$RELEASE_BASE/$CHECKSUM"

EXPECTED=$(awk 'NR == 1 {print $1}' "$TMP_ROOT/$CHECKSUM")
[[ $EXPECTED =~ ^[0-9A-Fa-f]{64}$ ]] || {
  printf '%s\n' "Release checksum is invalid." >&2
  exit 1
}
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL=$(sha256sum "$TMP_ROOT/$ASSET" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL=$(shasum -a 256 "$TMP_ROOT/$ASSET" | awk '{print $1}')
else
  printf '%s\n' "A SHA-256 tool (sha256sum or shasum) is required." >&2
  exit 1
fi
[[ ${ACTUAL,,} == ${EXPECTED,,} ]] || {
  printf '%s\n' "Release checksum verification failed." >&2
  exit 1
}

SOURCE_DIR="$TMP_ROOT/source"
mkdir -p "$SOURCE_DIR"
if command -v unzip >/dev/null 2>&1; then
  unzip -q "$TMP_ROOT/$ASSET" -d "$SOURCE_DIR"
elif [[ $(uname -s) == Darwin ]] && command -v ditto >/dev/null 2>&1; then
  ditto -x -k "$TMP_ROOT/$ASSET" "$SOURCE_DIR"
elif command -v python3 >/dev/null 2>&1; then
  python3 -m zipfile -e "$TMP_ROOT/$ASSET" "$SOURCE_DIR"
else
  printf '%s\n' "unzip, ditto, or Python 3 is required to extract the verified release." >&2
  exit 1
fi

[[ -x "$SOURCE_DIR/setup.sh" ]] || chmod +x "$SOURCE_DIR/setup.sh" 2>/dev/null || true
[[ -f "$SOURCE_DIR/setup.sh" ]] || {
  printf '%s\n' "The release archive does not contain setup.sh." >&2
  exit 1
}
exec "$SOURCE_DIR/setup.sh" "$@"
