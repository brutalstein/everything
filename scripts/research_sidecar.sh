#!/usr/bin/env bash
set -euo pipefail
ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
COMPOSE_FILE="${EVERYTHING_SEARXNG_COMPOSE:-$ROOT/deploy/searxng/compose.yml}"
PORT="${EVERYTHING_SEARXNG_PORT:-8888}"
ACTION="${1:-start}"
STATE_DIR="${EVERYTHING_HOME:-$HOME/.everything}/research/searxng"
SETTINGS_FILE="${EVERYTHING_SEARXNG_SETTINGS:-$STATE_DIR/settings.yml}"
PROJECT_NAME="${EVERYTHING_SEARXNG_PROJECT:-everything-research}"

prepare_settings() {
  mkdir -p "$STATE_DIR"
  chmod 700 "$STATE_DIR" 2>/dev/null || true
  if [[ ! -f "$SETTINGS_FILE" ]]; then
    local template="$ROOT/deploy/searxng/settings.yml"
    [[ -f "$template" ]] || { echo "[everything] SearXNG settings template missing: $template" >&2; return 1; }
    local secret temporary
    if command -v openssl >/dev/null 2>&1; then
      secret=$(openssl rand -hex 32)
    else
      secret=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
    fi
    temporary="$SETTINGS_FILE.tmp.$$"
    sed "s/everything-local-loopback-only/$secret/" "$template" > "$temporary"
    chmod 600 "$temporary"
    mv "$temporary" "$SETTINGS_FILE"
  fi
  export EVERYTHING_SEARXNG_SETTINGS="$SETTINGS_FILE"
  export EVERYTHING_SEARXNG_PORT="$PORT"
  export COMPOSE_PROJECT_NAME="$PROJECT_NAME"
}

compose() {
  if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
    docker compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" "$@"
  elif command -v podman >/dev/null 2>&1 && podman compose version >/dev/null 2>&1; then
    podman compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" "$@"
  elif command -v podman-compose >/dev/null 2>&1; then
    podman-compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" "$@"
  else
    return 127
  fi
}

prepare_settings
case "$ACTION" in
  start)
    set +e
    compose up -d --remove-orphans
    status=$?
    set -e
    if [[ $status -eq 127 ]]; then
      echo "[everything] Docker/Podman bulunamadı; native anahtarsız web sağlayıcıları kullanılacak." >&2
      exit 0
    elif [[ $status -ne 0 ]]; then
      echo "[everything] Yerel SearXNG başlatılamadı; native anahtarsız web sağlayıcılarına geçiliyor." >&2
      exit 0
    fi
    ;;
  stop) compose down --remove-orphans || true; exit 0 ;;
  status) compose ps; exit $? ;;
  *) echo "usage: $0 [start|stop|status]" >&2; exit 2 ;;
esac

if command -v curl >/dev/null 2>&1; then
  for delay in 1 1 2 3 5 8; do
    if curl -fsS --max-time 4 "http://127.0.0.1:${PORT}/search?q=everything&format=json" >/dev/null; then
      echo "[everything] Yerel SearXNG hazır: http://127.0.0.1:${PORT}"
      exit 0
    fi
    sleep "$delay"
  done
fi
echo "[everything] SearXNG container başlatıldı; health-check henüz hazır değil." >&2
exit 0
