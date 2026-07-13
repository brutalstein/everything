#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
WORKSPACE="$PWD"
MODEL="${EVERYTHING_MODEL:-auto}"
NO_LAUNCH=0
NO_SERVICE=0
NO_VERIFY=0

usage() {
  cat <<'EOF'
Everything smart one-command setup

Usage: ./setup.sh [--workspace PATH] [--model TAG|auto] [--no-launch] [--no-service] [--no-verify]

The setup is idempotent: it installs/updates dependencies, chooses a local model,
builds and verifies the product, installs the background scheduler, pulls the
model, runs health checks, and opens Everything.
EOF
}

while (($#)); do
  case "$1" in
    --workspace) WORKSPACE=${2:?missing workspace}; shift 2 ;;
    --model) MODEL=${2:?missing model}; shift 2 ;;
    --no-launch) NO_LAUNCH=1; shift ;;
    --no-service) NO_SERVICE=1; shift ;;
    --no-verify) NO_VERIFY=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

memory_gib() {
  case "$(uname -s)" in
    Darwin) awk -v bytes="$(sysctl -n hw.memsize)" 'BEGIN { printf "%d", bytes/1073741824 }' ;;
    Linux) awk '/MemTotal:/ { printf "%d", $2/1048576 }' /proc/meminfo ;;
    *) echo 8 ;;
  esac
}

gpu_gib() {
  if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null \
      | awk 'BEGIN {m=0} {if ($1>m) m=$1} END {printf "%d", m/1024}'
  else
    echo 0
  fi
}

disk_gib() {
  local probe=${OLLAMA_MODELS:-$HOME/.ollama}
  while [[ ! -e "$probe" && $probe != "/" ]]; do probe=$(dirname "$probe"); done
  df -Pk "$probe" | awk 'NR == 2 {printf "%d", $4/1048576}'
}

choose_model() {
  local memory gpu disk os
  memory=$(memory_gib)
  gpu=$(gpu_gib)
  disk=$(disk_gib)
  os=$(uname -s)

  if (( disk < 5 )); then
    printf '%s\n' "Everything needs at least 5 GiB free for a practical local coding model (detected ${disk} GiB)." >&2
    return 1
  fi
  if { [[ $os == Darwin ]] && (( memory >= 24 )); } || (( gpu >= 16 )); then
    (( disk >= 16 )) && { echo "qwen2.5-coder:14b"; return; }
  fi
  if (( gpu >= 8 || memory >= 16 )); then
    (( disk >= 9 )) && { echo "qwen2.5-coder:7b"; return; }
  fi
  echo "qwen2.5-coder:3b"
}

if [[ $MODEL == auto ]]; then
  MODEL=$(choose_model)
  printf '[everything] Selected %s from detected RAM, GPU memory, platform, and free disk capacity.\n' "$MODEL"
fi

args=(--workspace "$WORKSPACE" --model "$MODEL" --install-deps --pull-model)
(( NO_LAUNCH )) && args+=(--no-launch)
(( NO_SERVICE )) && args+=(--no-service)
(( NO_VERIFY )) && args+=(--no-verify)
exec "$ROOT/install.sh" "${args[@]}"
