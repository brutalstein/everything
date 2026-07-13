#!/usr/bin/env bash
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
MODEL="qwen2.5-coder:7b"
WORKSPACE="$PWD"
INSTALL_DIR="${EVERYTHING_INSTALL_DIR:-$HOME/.local/share/everything}"
BIN_DIR="${EVERYTHING_BIN_DIR:-$HOME/.local/bin}"
INSTALL_DEPS=0
PULL_MODEL=0
LAUNCH=1
RUN_VERIFY=1
INSTALL_SERVICE=1
VERSION="0.3.0"
RUST_TOOLCHAIN="1.97.0"
SERVICE_PORT="${EVERYTHING_SERVICE_PORT:-}"
OAUTH_PORT="${EVERYTHING_OAUTH_PORT:-}"
SERVICE_PORT_EXPLICIT=0
OAUTH_PORT_EXPLICIT=0
RESEARCH_SIDECAR_STATUS="fallback"
[[ -n "$SERVICE_PORT" ]] && SERVICE_PORT_EXPLICIT=1
[[ -n "$OAUTH_PORT" ]] && OAUTH_PORT_EXPLICIT=1

usage() {
  cat <<'USAGE'
Everything MVP installer

Usage: ./install.sh [options]
  --workspace PATH   Workspace opened by the desktop app (default: current directory)
  --model NAME       Ollama model tag (default: qwen2.5-coder:7b)
  --install-dir PATH Installation directory
  --install-deps     Install missing Rust, Node.js/npm, and Ollama when supported
  --pull-model       Run `ollama pull` for the selected model
  --no-verify        Skip workspace test/typecheck gates before installation
  --no-launch        Install without opening the desktop app
  --no-service       Do not install the persistent background scheduler service
  -h, --help         Show this help
USAGE
}

while (($#)); do
  case "$1" in
    --workspace) WORKSPACE=${2:?missing workspace}; shift 2 ;;
    --model) MODEL=${2:?missing model}; shift 2 ;;
    --install-dir) INSTALL_DIR=${2:?missing install directory}; shift 2 ;;
    --install-deps) INSTALL_DEPS=1; shift ;;
    --pull-model) PULL_MODEL=1; shift ;;
    --no-verify) RUN_VERIFY=0; shift ;;
    --no-launch) LAUNCH=0; shift ;;
    --no-service) INSTALL_SERVICE=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

WORKSPACE=$(mkdir -p "$WORKSPACE" && CDPATH= cd -- "$WORKSPACE" && pwd)
[[ $WORKSPACE != *$'\n'* && $WORKSPACE != *'"'* ]] || { echo "Workspace path contains unsupported control characters" >&2; exit 2; }

SETUP_STATE_DIR="$HOME/.everything/setup"
SETUP_LOCK_DIR="$SETUP_STATE_DIR/install.lock"
SETUP_LOCK_OWNER="$SETUP_LOCK_DIR/pid"
PORT_STATE_FILE="$SETUP_STATE_DIR/service-ports.env"
LOCK_OWNED=0
mkdir -p "$SETUP_STATE_DIR"

state_value() {
  local key=$1
  local file=$2
  [[ -f "$file" ]] || return 1
  awk -F= -v wanted="$key" '$1 == wanted { value = substr($0, index($0, "=") + 1); print value; found = 1; exit } END { if (!found) exit 1 }' "$file"
}

if [[ -z "$SERVICE_PORT" ]]; then SERVICE_PORT=$(state_value SERVICE_PORT "$PORT_STATE_FILE" 2>/dev/null || printf '3472'); fi
if [[ -z "$OAUTH_PORT" ]]; then OAUTH_PORT=$(state_value OAUTH_PORT "$PORT_STATE_FILE" 2>/dev/null || printf '43821'); fi
for port_name in SERVICE_PORT OAUTH_PORT; do
  port_value=${!port_name}
  [[ $port_value =~ ^[0-9]+$ ]] || { echo "[everything] $port_name must be numeric" >&2; exit 2; }
  (( port_value >= 1024 && port_value <= 65535 )) || { echo "[everything] $port_name must be between 1024 and 65535" >&2; exit 2; }
done
[[ $SERVICE_PORT != "$OAUTH_PORT" ]] || { echo "[everything] Service and OAuth ports must be different" >&2; exit 2; }

acquire_setup_lock() {
  if mkdir "$SETUP_LOCK_DIR" 2>/dev/null; then
    printf '%s\n' "$$" > "$SETUP_LOCK_OWNER"
    LOCK_OWNED=1
    return 0
  fi
  local owner=""
  owner=$(cat "$SETUP_LOCK_OWNER" 2>/dev/null || true)
  if [[ $owner =~ ^[0-9]+$ ]] && kill -0 "$owner" >/dev/null 2>&1; then
    echo "[everything] Another installation is already running (pid=$owner)" >&2
    return 1
  fi
  echo "[everything] Recovering a stale installation lock" >&2
  rm -rf "$SETUP_LOCK_DIR"
  mkdir "$SETUP_LOCK_DIR"
  printf '%s\n' "$$" > "$SETUP_LOCK_OWNER"
  LOCK_OWNED=1
}
acquire_setup_lock || exit 1
SMOKE_PID=""
INSTALL_BACKUP=""
INSTALL_SWITCHED=0
INSTALL_COMPLETE=0
cleanup_all() {
  local status=$?
  if [[ -n "${SMOKE_PID:-}" ]]; then
    kill "$SMOKE_PID" >/dev/null 2>&1 || true
    wait "$SMOKE_PID" >/dev/null 2>&1 || true
    SMOKE_PID=""
  fi
  if (( INSTALL_SWITCHED && ! INSTALL_COMPLETE )); then
    if type stop_background_service >/dev/null 2>&1; then
      stop_background_service >/dev/null 2>&1 || true
    fi
    rm -rf "$INSTALL_DIR"
    if [[ -n "${INSTALL_BACKUP:-}" && -d "$INSTALL_BACKUP" ]]; then
      mv "$INSTALL_BACKUP" "$INSTALL_DIR" >/dev/null 2>&1 || true
      if [[ $(uname -s) == "Darwin" ]]; then
        launchctl kickstart -k "gui/$(id -u)/dev.everything.runtime" >/dev/null 2>&1 || true
      elif command -v systemctl >/dev/null 2>&1; then
        systemctl --user restart everythingd.service >/dev/null 2>&1 || true
      fi
    fi
  fi
  if (( LOCK_OWNED )); then rm -rf "$SETUP_LOCK_DIR"; LOCK_OWNED=0; fi
  return "$status"
}
trap cleanup_all EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
exec > >(tee -a "$SETUP_STATE_DIR/install.log") 2>&1
printf '%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ) workspace=$WORKSPACE model=$MODEL" > "$SETUP_STATE_DIR/current-stage"

log() { printf '\n[everything] %s\n' "$*"; printf '%s\n' "$*" > "$SETUP_STATE_DIR/current-stage"; }
fail() { printf '\n[everything] ERROR: %s\n' "$*" >&2; exit 1; }
retry() {
  local attempts=$1; shift
  local delay_seconds=2
  local attempt
  for ((attempt=1; attempt<=attempts; attempt++)); do
    "$@" && return 0
    (( attempt == attempts )) && return 1
    log "Retrying after a transient failure ($attempt/$attempts): $*"
    sleep "$delay_seconds"
    delay_seconds=$((delay_seconds * 2))
  done
}

download_official_script() {
  local url=$1
  local destination=$2
  retry 3 curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
    --max-time 300 --output "$destination" "$url" || fail "Could not download $url"
  [[ -s "$destination" ]] || fail "Downloaded installer is empty: $url"
  local size
  size=$(wc -c < "$destination" | tr -d ' ')
  (( size <= 5 * 1024 * 1024 )) || fail "Downloaded installer is unexpectedly large: $url"
  head -c 256 "$destination" | grep -qE '(^#!|bin/(ba)?sh|Copyright|Rustup|Ollama|NodeSource)' \
    || fail "Downloaded installer does not look like a shell script: $url"
  chmod 700 "$destination"
}

run_as_root() {
  if (( EUID == 0 )); then "$@"
  elif command -v sudo >/dev/null 2>&1; then sudo "$@"
  elif command -v doas >/dev/null 2>&1; then doas "$@"
  else
    echo "[everything] Root privileges are required for: $*" >&2
    return 126
  fi
}

install_with_package_manager() {
  local package=$1
  if command -v brew >/dev/null 2>&1; then brew install "$package"
  elif command -v apt-get >/dev/null 2>&1; then run_as_root apt-get update && run_as_root apt-get install -y "$package"
  elif command -v dnf >/dev/null 2>&1; then run_as_root dnf install -y "$package"
  elif command -v pacman >/dev/null 2>&1; then run_as_root pacman -Sy --needed --noconfirm "$package"
  elif command -v zypper >/dev/null 2>&1; then run_as_root zypper --non-interactive install "$package"
  else return 1
  fi
}

ensure_dependencies() {
  if ! command -v rustup >/dev/null 2>&1; then
    (( INSTALL_DEPS )) || fail "rustup is missing. Re-run with --install-deps or install rustup."
    command -v curl >/dev/null 2>&1 || install_with_package_manager curl || fail "curl is required to install Rust"
    log "Installing rustup"
    local rustup_script
    rustup_script=$(mktemp "${TMPDIR:-/tmp}/everything-rustup.XXXXXX")
    download_official_script "https://sh.rustup.rs" "$rustup_script"
    sh "$rustup_script" -y --profile minimal --default-toolchain none
    rm -f "$rustup_script"
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
  fi

  command -v rustup >/dev/null 2>&1 || fail "Rust installation did not provide rustup"
  log "Preparing Rust $RUST_TOOLCHAIN with rustfmt and clippy"
  rustup toolchain install "$RUST_TOOLCHAIN" --profile minimal --component rustfmt --component clippy     || fail "Rust $RUST_TOOLCHAIN could not be installed"
  export RUSTUP_TOOLCHAIN="$RUST_TOOLCHAIN"
  command -v cargo >/dev/null 2>&1 || fail "Rust installation did not provide cargo"
  rustc --version | grep -Fq "rustc $RUST_TOOLCHAIN"     || fail "Expected Rust $RUST_TOOLCHAIN, found $(rustc --version)"

  if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    (( INSTALL_DEPS )) || fail "Node.js/npm is missing. Re-run with --install-deps or install Node.js 22+."
    log "Installing Node.js and npm"
    if command -v brew >/dev/null 2>&1; then
      brew install node@22 || brew install node
      export PATH="$(brew --prefix node@22 2>/dev/null)/bin:$PATH"
    else
      install_with_package_manager nodejs || fail "No supported package manager could install Node.js"
      command -v npm >/dev/null 2>&1 || install_with_package_manager npm || true
    fi
  fi

  local node_major
  node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
  if (( node_major < 22 )) && (( INSTALL_DEPS )); then
    log "Upgrading Node.js to the 22.x LTS line"
    if command -v brew >/dev/null 2>&1; then
      brew install node@22 || brew upgrade node@22
      export PATH="$(brew --prefix node@22)/bin:$PATH"
    elif command -v apt-get >/dev/null 2>&1; then
      local nodesource_script
      nodesource_script=$(mktemp "${TMPDIR:-/tmp}/everything-nodesource.XXXXXX")
      download_official_script "https://deb.nodesource.com/setup_22.x" "$nodesource_script"
      run_as_root bash "$nodesource_script"
      rm -f "$nodesource_script"
      run_as_root apt-get install -y nodejs
    elif command -v dnf >/dev/null 2>&1; then
      local nodesource_script
      nodesource_script=$(mktemp "${TMPDIR:-/tmp}/everything-nodesource.XXXXXX")
      download_official_script "https://rpm.nodesource.com/setup_22.x" "$nodesource_script"
      run_as_root bash "$nodesource_script"
      rm -f "$nodesource_script"
      run_as_root dnf install -y nodejs
    fi
    hash -r
    node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
  fi
  (( node_major >= 22 )) || fail "Node.js 22+ is required (found $(node -v))."
  local npm_major
  npm_major=$(npm --version | awk -F. '{print $1}')
  if (( npm_major < 10 )) && (( INSTALL_DEPS )); then
    log "Upgrading npm to the supported 10.x line"
    npm install --global npm@10 >/dev/null 2>&1 || run_as_root npm install --global npm@10
    hash -r
    npm_major=$(npm --version | awk -F. '{print $1}')
  fi
  (( npm_major >= 10 )) || fail "npm 10+ is required (found $(npm --version))."

  if (( RUN_VERIFY )); then
    local python_ready=0
    if command -v python3 >/dev/null 2>&1       && python3 -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)' >/dev/null 2>&1; then
      python_ready=1
    fi
    if (( ! python_ready )); then
      (( INSTALL_DEPS )) || fail "Python 3.11+ is missing or outdated. Re-run with --install-deps or use --no-verify."
      log "Installing Python 3.11+"
      if command -v brew >/dev/null 2>&1; then
        brew install python@3.12 || brew upgrade python@3.12 || brew install python
        export PATH="$(brew --prefix python@3.12 2>/dev/null)/bin:$PATH"
      else
        install_with_package_manager python3 || fail "No supported package manager could install Python 3.11+"
      fi
      hash -r
    fi
    command -v python3 >/dev/null 2>&1 || fail "Python installation did not provide python3"
    python3 -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 11) else 1)'       || fail "Python 3.11+ is required (found $(python3 --version 2>&1))."
  fi

  if ! command -v ollama >/dev/null 2>&1 && (( INSTALL_DEPS )); then
    log "Installing Ollama"
    if command -v brew >/dev/null 2>&1; then
      brew install ollama
    else
      command -v curl >/dev/null 2>&1 || install_with_package_manager curl || fail "curl is required to install Ollama"
      local ollama_script
      ollama_script=$(mktemp "${TMPDIR:-/tmp}/everything-ollama.XXXXXX")
      download_official_script "https://ollama.com/install.sh" "$ollama_script"
      sh "$ollama_script"
      rm -f "$ollama_script"
    fi
  fi

  if (( INSTALL_DEPS )) && [[ $(uname -s) == "Linux" ]]; then
    if ! command -v bwrap >/dev/null 2>&1; then
      log "Installing Bubblewrap for workspace process isolation"
      install_with_package_manager bubblewrap || log "Bubblewrap is unavailable; command execution will remain allowlist-only"
    fi
    if ! command -v secret-tool >/dev/null 2>&1; then
      log "Installing the OS secret-vault client"
      if command -v apt-get >/dev/null 2>&1; then
        run_as_root apt-get install -y libsecret-tools || true
      elif command -v dnf >/dev/null 2>&1; then
        run_as_root dnf install -y libsecret || true
      elif command -v pacman >/dev/null 2>&1; then
        run_as_root pacman -Sy --needed --noconfirm libsecret || true
      elif command -v zypper >/dev/null 2>&1; then
        run_as_root zypper --non-interactive install libsecret-tools || true
      fi
    fi
  fi
}

xml_escape() {
  printf '%s' "$1" | sed \
    -e 's/&/\&amp;/g' \
    -e 's/</\&lt;/g' \
    -e 's/>/\&gt;/g' \
    -e 's/"/\&quot;/g' \
    -e "s/'/\&apos;/g"
}

systemd_escape() {
  local value=$1
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//%/%%}
  printf '%s' "$value"
}

desktop_exec_escape() {
  local value=$1
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//\`/\\\`}
  value=${value//\$/\\\$}
  printf '"%s"' "$value"
}

assert_port_available() {
  local port=$1
  node - "$port" <<'NODE'
const net = require('net');
const port = Number(process.argv[2]);
const socket = net.createConnection({host: '127.0.0.1', port});
const timer = setTimeout(() => { socket.destroy(); process.exit(0); }, 750);
socket.once('connect', () => { clearTimeout(timer); socket.destroy(); process.exit(1); });
socket.once('error', () => { clearTimeout(timer); process.exit(0); });
NODE
}

find_free_loopback_port() {
  local excluded=${1:-0}
  node - "$excluded" <<'NODE'
const net = require('net');
const excluded = Number(process.argv[2]);
let attempts = 0;
function bind() {
  const server = net.createServer();
  server.unref();
  server.once('error', () => {
    if (++attempts >= 8) process.exit(1);
    bind();
  });
  server.listen({host: '127.0.0.1', port: 0, exclusive: true}, () => {
    const address = server.address();
    const port = typeof address === 'object' && address ? address.port : 0;
    server.close(() => {
      if (!port || port === excluded) {
        if (++attempts >= 8) process.exit(1);
        bind();
      } else {
        console.log(port);
      }
    });
  });
}
bind();
NODE
}

persist_service_ports() {
  local temporary="$PORT_STATE_FILE.tmp.$$"
  umask 077
  {
    printf 'SERVICE_PORT=%s\n' "$SERVICE_PORT"
    printf 'OAUTH_PORT=%s\n' "$OAUTH_PORT"
    printf 'WORKSPACE=%s\n' "$WORKSPACE"
  } > "$temporary"
  chmod 600 "$temporary"
  mv "$temporary" "$PORT_STATE_FILE"
}

prepare_service_ports() {
  (( INSTALL_SERVICE )) || return 0
  stop_background_service
  sleep 0.25

  if ! assert_port_available "$SERVICE_PORT"; then
    (( SERVICE_PORT_EXPLICIT == 0 )) || fail "Port $SERVICE_PORT is already in use; choose a free EVERYTHING_SERVICE_PORT."
    local previous=$SERVICE_PORT
    SERVICE_PORT=$(find_free_loopback_port "$OAUTH_PORT") || fail "Could not allocate a free loopback service port"
    log "Service port $previous was occupied; selected $SERVICE_PORT and persisted it"
  fi
  if ! assert_port_available "$OAUTH_PORT"; then
    (( OAUTH_PORT_EXPLICIT == 0 )) || fail "OAuth callback port $OAUTH_PORT is already in use; choose a free EVERYTHING_OAUTH_PORT."
    local previous=$OAUTH_PORT
    OAUTH_PORT=$(find_free_loopback_port "$SERVICE_PORT") || fail "Could not allocate a free loopback OAuth port"
    log "OAuth callback port $previous was occupied; selected $OAUTH_PORT. Register http://127.0.0.1:$OAUTH_PORT/v1/connectors/oauth/callback with connector providers."
  fi
  [[ $SERVICE_PORT != "$OAUTH_PORT" ]] || fail "Service and OAuth callback ports resolved to the same value"
  persist_service_ports
}

stop_background_service() {
  if [[ $(uname -s) == "Darwin" ]]; then
    launchctl bootout "gui/$(id -u)/dev.everything.runtime" >/dev/null 2>&1 || true
  elif command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop everythingd.service >/dev/null 2>&1 || true
  fi
  local pid_file="$HOME/.everything/everythingd.pid"
  if [[ -f "$pid_file" ]]; then
    local previous_pid
    previous_pid=$(cat "$pid_file" 2>/dev/null || true)
    if [[ $previous_pid =~ ^[0-9]+$ ]]; then
      kill "$previous_pid" >/dev/null 2>&1 || true
      for _ in $(seq 1 20); do
        kill -0 "$previous_pid" >/dev/null 2>&1 || break
        sleep 0.1
      done
    fi
    rm -f "$pid_file"
  fi
}

write_service_wrapper() {
  local wrapper=$1
  mkdir -p "$(dirname "$wrapper")"
  {
    printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail'
    printf 'export EVERYTHING_WORKSPACE=%q\n' "$WORKSPACE"
    printf 'export EVERYTHING_HOME=%q\n' "${EVERYTHING_HOME:-$HOME/.everything}"
    printf 'exec %q --workspace %q --listen %q --oauth-listen %q\n' \
      "$INSTALL_DIR/bin/everythingd" "$WORKSPACE" "127.0.0.1:$SERVICE_PORT" "127.0.0.1:$OAUTH_PORT"
  } > "$wrapper.tmp"
  chmod 700 "$wrapper.tmp"
  mv "$wrapper.tmp" "$wrapper"
}

install_background_service() {
  (( INSTALL_SERVICE )) || return 0
  local log_dir="$HOME/.everything/logs"
  local service_wrapper="$HOME/.everything/bin/everythingd-service"
  mkdir -p "$log_dir"

  stop_background_service
  sleep 0.25
  assert_port_available "$SERVICE_PORT" || fail "Port $SERVICE_PORT is already owned by another process. Set EVERYTHING_SERVICE_PORT to a free loopback port."
  assert_port_available "$OAUTH_PORT" || fail "OAuth callback port $OAUTH_PORT is already owned by another process. Set EVERYTHING_OAUTH_PORT to a free loopback port."
  write_service_wrapper "$service_wrapper"

  if [[ $(uname -s) == "Darwin" ]]; then
    local agents="$HOME/Library/LaunchAgents"
    local plist="$agents/dev.everything.runtime.plist"
    local escaped_wrapper escaped_stdout escaped_stderr escaped_path
    mkdir -p "$agents"
    escaped_wrapper=$(xml_escape "$service_wrapper")
    escaped_stdout=$(xml_escape "$log_dir/everythingd.log")
    escaped_stderr=$(xml_escape "$log_dir/everythingd-error.log")
    escaped_path=$(xml_escape "$PATH")
    cat > "$plist.tmp" <<EOF_PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.everything.runtime</string>
  <key>ProgramArguments</key><array><string>$escaped_wrapper</string></array>
  <key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>ThrottleInterval</key><integer>5</integer>
  <key>ProcessType</key><string>Background</string>
  <key>StandardOutPath</key><string>$escaped_stdout</string>
  <key>StandardErrorPath</key><string>$escaped_stderr</string>
  <key>EnvironmentVariables</key><dict><key>PATH</key><string>$escaped_path</string></dict>
</dict></plist>
EOF_PLIST
    mv "$plist.tmp" "$plist"
    launchctl bootstrap "gui/$(id -u)" "$plist"
    launchctl enable "gui/$(id -u)/dev.everything.runtime"
    launchctl kickstart -k "gui/$(id -u)/dev.everything.runtime"
  elif command -v systemctl >/dev/null 2>&1 && systemctl --user show-environment >/dev/null 2>&1; then
    local units="$HOME/.config/systemd/user"
    local unit="$units/everythingd.service"
    local escaped_wrapper escaped_workspace escaped_home escaped_path
    mkdir -p "$units"
    escaped_wrapper=$(systemd_escape "$service_wrapper")
    escaped_workspace=$(systemd_escape "$WORKSPACE")
    escaped_home=$(systemd_escape "${EVERYTHING_HOME:-$HOME/.everything}")
    escaped_path=$(systemd_escape "$PATH")
    cat > "$unit.tmp" <<EOF_UNIT
[Unit]
Description=Everything local autonomous runtime
After=default.target network-online.target
Wants=network-online.target
StartLimitIntervalSec=60
StartLimitBurst=10

[Service]
Type=simple
ExecStart="$escaped_wrapper"
Restart=on-failure
RestartSec=2
Environment="PATH=$escaped_path"
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths="$escaped_workspace" "$escaped_home"
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
UMask=0077

[Install]
WantedBy=default.target
EOF_UNIT
    mv "$unit.tmp" "$unit"
    systemctl --user daemon-reload
    systemctl --user enable --now everythingd.service
  else
    local autostart_dir="$HOME/.config/autostart"
    local desktop="$autostart_dir/everything-runtime.desktop"
    local desktop_exec
    mkdir -p "$autostart_dir"
    desktop_exec=$(desktop_exec_escape "$service_wrapper")
    cat > "$desktop.tmp" <<EOF_DESKTOP
[Desktop Entry]
Type=Application
Version=1.0
Name=Everything Runtime
Comment=Local autonomous scheduler and connector runtime
Exec=$desktop_exec
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF_DESKTOP
    mv "$desktop.tmp" "$desktop"
    chmod 600 "$desktop"
    nohup "$service_wrapper" >>"$log_dir/everythingd.log" 2>>"$log_dir/everythingd-error.log" &
    printf '%s\n' "$!" > "$HOME/.everything/everythingd.pid"
    log "No user service manager was detected; installed an XDG login autostart fallback"
  fi

  EVERYTHING_EXPECTED_WORKSPACE="$WORKSPACE" EVERYTHING_SERVICE_PORT="$SERVICE_PORT" node <<'NODE'
const expectedWorkspace = process.env.EVERYTHING_EXPECTED_WORKSPACE;
const port = process.env.EVERYTHING_SERVICE_PORT;
const deadline = Date.now() + 30000;
(async () => {
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/v1/info`);
      if (response.ok) {
        const info = await response.json();
        if (info.service === 'everythingd' && info.workspace === expectedWorkspace) process.exit(0);
      }
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  console.error('persistent everythingd service did not become healthy for the selected workspace');
  process.exit(1);
})();
NODE
}

ensure_ollama_running() {
  command -v ollama >/dev/null 2>&1 || return 1
  if ollama list >/dev/null 2>&1; then return 0; fi
  log "Starting Ollama"
  if [[ $(uname -s) == "Darwin" ]] && command -v brew >/dev/null 2>&1; then
    brew services start ollama >/dev/null 2>&1 || open -a Ollama >/dev/null 2>&1 || true
  elif command -v systemctl >/dev/null 2>&1; then
    run_as_root systemctl enable --now ollama >/dev/null 2>&1 || systemctl --user enable --now ollama >/dev/null 2>&1 || true
  fi
  if ! ollama list >/dev/null 2>&1; then
    nohup ollama serve >"${TMPDIR:-/tmp}/everything-ollama.log" 2>&1 &
  fi
  for _ in $(seq 1 60); do ollama list >/dev/null 2>&1 && return 0; sleep 1; done
  return 1
}

model_required_mib() {
  local normalized=${1,,}
  case "$normalized" in
    *32b*) echo 28000 ;;
    *14b*) echo 14000 ;;
    *9b*) echo 9000 ;;
    *7b*|*8b*) echo 7500 ;;
    *3b*|*4b*) echo 4200 ;;
    *1.5b*|*2b*) echo 2600 ;;
    *0.5b*|*1b*) echo 1600 ;;
    *) echo 10000 ;;
  esac
}

ensure_model_disk_capacity() {
  local required_mib available_mib probe
  required_mib=$(model_required_mib "$MODEL")
  probe=${OLLAMA_MODELS:-$HOME/.ollama}
  while [[ ! -e "$probe" && $probe != "/" ]]; do probe=$(dirname "$probe"); done
  available_mib=$(df -Pk "$probe" | awk 'NR == 2 {printf "%d", $4 / 1024}')
  [[ $available_mib =~ ^[0-9]+$ ]] || fail "Could not determine free disk capacity for Ollama models"
  (( available_mib >= required_mib )) || fail "Model $MODEL needs approximately ${required_mib} MiB free, but only ${available_mib} MiB is available on the model filesystem"
}

run_live_model_smoke() {
  (( RUN_VERIFY && PULL_MODEL )) || return 0
  log "Running a live local-model readiness test"
  local port
  port=$(node - <<'NODE'
const net = require('net');
const server = net.createServer();
server.listen(0, '127.0.0.1', () => { console.log(server.address().port); server.close(); });
NODE
)
  "$INSTALL_DIR/bin/everythingd" --workspace "$WORKSPACE" --listen "127.0.0.1:$port" --oauth-listen "127.0.0.1:0" \
    >"$INSTALL_DIR/model-smoke.log" 2>&1 &
  SMOKE_PID=$!
  if ! python3 "$ROOT/scripts/smoke_ollama.py" --base-url "http://127.0.0.1:$port" --model "$MODEL" --mode Fast; then
    fail "The model was pulled, but the live Everything/Ollama readiness test failed. See $INSTALL_DIR/model-smoke.log"
  fi
  kill "$SMOKE_PID" >/dev/null 2>&1 || true
  wait "$SMOKE_PID" >/dev/null 2>&1 || true
  SMOKE_PID=""
}


start_research_sidecar() {
  RESEARCH_SIDECAR_STATUS="fallback"
  if [[ ${EVERYTHING_RESEARCH_SIDECAR:-auto} == "off" ]]; then
    RESEARCH_SIDECAR_STATUS="disabled"
    log "Local SearXNG sidecar disabled; native keyless research providers remain available"
    return 0
  fi
  local helper="$INSTALL_DIR/scripts/research_sidecar.sh"
  if [[ ! -x "$helper" ]]; then
    log "Local SearXNG helper is unavailable; native keyless research providers remain available"
    return 0
  fi
  log "Starting the optional loopback SearXNG research sidecar"
  EVERYTHING_SEARXNG_PORT="${EVERYTHING_SEARXNG_PORT:-8888}" "$helper" start || true
  if command -v curl >/dev/null 2>&1 && curl -fsS --max-time 4 "http://127.0.0.1:${EVERYTHING_SEARXNG_PORT:-8888}/search?q=everything&format=json" >/dev/null 2>&1; then
    RESEARCH_SIDECAR_STATUS="ready"
  else
    RESEARCH_SIDECAR_STATUS="fallback"
  fi
}

run_runtime_doctor() {
  log "Running the complete native runtime doctor"
  local report="$INSTALL_DIR/runtime-doctor.json"
  local temporary="$report.tmp.$$"
  "$INSTALL_DIR/bin/everything-cli" --workspace "$WORKSPACE" --json doctor > "$temporary" \
    || fail "Native runtime doctor could not complete"
  EVERYTHING_DOCTOR_REPORT="$temporary" EVERYTHING_REQUIRE_MODEL="$PULL_MODEL" node <<'NODE'
const fs = require('fs');
const report = JSON.parse(fs.readFileSync(process.env.EVERYTHING_DOCTOR_REPORT, 'utf8'));
const required = ['model', 'graph', 'state-store', 'memory', 'skills', 'connectors', 'scheduler', 'tool-sandbox', 'data-directory', 'research'];
const ids = new Set((report.checks || []).map((check) => check.check_id));
const missing = required.filter((id) => !ids.has(id));
if (missing.length) throw new Error(`runtime doctor omitted required checks: ${missing.join(', ')}`);
const failed = (report.checks || []).filter((check) => check.status === 'failed');
if (failed.length) {
  throw new Error(`runtime doctor reported failed components: ${failed.map((check) => `${check.check_id}: ${check.detail}`).join('; ')}`);
}
if (process.env.EVERYTHING_REQUIRE_MODEL === '1') {
  const model = report.checks.find((check) => check.check_id === 'model');
  if (!model || model.status === 'failed') throw new Error(`configured local model is not ready: ${model?.detail || 'missing model check'}`);
}
NODE
  EVERYTHING_DOCTOR_REPORT="$temporary" node <<'NODE'
const fs = require('fs');
const report = JSON.parse(fs.readFileSync(process.env.EVERYTHING_DOCTOR_REPORT, 'utf8'));
for (const check of report.checks || []) {
  if (check.status === 'degraded') {
    console.error(`[everything] WARNING: ${check.label}: ${check.detail}`);
    if (check.remediation) console.error(`[everything]          ${check.remediation}`);
  }
}
NODE
  chmod 600 "$temporary"
  mv "$temporary" "$report"
}

write_install_manifest() {
  local manifest="$INSTALL_DIR/install-manifest.json"
  local state_manifest="$SETUP_STATE_DIR/last-success.json"
  local git_revision="unknown"
  if command -v git >/dev/null 2>&1; then
    git_revision=$(git -C "$ROOT" rev-parse --verify HEAD 2>/dev/null || printf 'unknown')
  fi
  EVERYTHING_MANIFEST="$manifest" \
  EVERYTHING_STATE_MANIFEST="$state_manifest" \
  EVERYTHING_VERSION="$VERSION" \
  EVERYTHING_WORKSPACE="$WORKSPACE" \
  EVERYTHING_INSTALL_DIR="$INSTALL_DIR" \
  EVERYTHING_MODEL="$MODEL" \
  EVERYTHING_SERVICE_PORT="$SERVICE_PORT" \
  EVERYTHING_OAUTH_PORT="$OAUTH_PORT" \
  EVERYTHING_INSTALL_SERVICE="$INSTALL_SERVICE" \
  EVERYTHING_RUN_VERIFY="$RUN_VERIFY" \
  EVERYTHING_PULL_MODEL="$PULL_MODEL" \
  EVERYTHING_GIT_REVISION="$git_revision" \
  EVERYTHING_RESEARCH_SIDECAR_STATUS="$RESEARCH_SIDECAR_STATUS" \
  EVERYTHING_DOCTOR_REPORT="$INSTALL_DIR/runtime-doctor.json" node <<'NODE'
const fs = require('fs');
const { execFileSync } = require('child_process');
const commandVersion = (command, args = ['--version']) => {
  try { return execFileSync(command, args, {encoding: 'utf8', timeout: 5000}).trim().split(/\r?\n/)[0]; }
  catch { return null; }
};
const exists = (command) => {
  try { execFileSync(process.platform === 'win32' ? 'where' : 'sh', process.platform === 'win32' ? [command] : ['-lc', `command -v ${command}`], {stdio: 'ignore', timeout: 3000}); return true; }
  catch { return false; }
};
const doctor = (() => {
  try { return JSON.parse(fs.readFileSync(process.env.EVERYTHING_DOCTOR_REPORT, 'utf8')); }
  catch { return null; }
})();
const manifest = {
  schema_version: 3,
  product: 'Everything',
  version: process.env.EVERYTHING_VERSION,
  installed_at: new Date().toISOString(),
  source_revision: process.env.EVERYTHING_GIT_REVISION,
  workspace: process.env.EVERYTHING_WORKSPACE,
  install_dir: process.env.EVERYTHING_INSTALL_DIR,
  model: process.env.EVERYTHING_MODEL,
  service: {
    installed: process.env.EVERYTHING_INSTALL_SERVICE === '1',
    base_url: `http://127.0.0.1:${process.env.EVERYTHING_SERVICE_PORT}`,
    oauth_callback: `http://127.0.0.1:${process.env.EVERYTHING_OAUTH_PORT}/v1/connectors/oauth/callback`,
    health_check_passed: true
  },
  verification: {
    full_gates: process.env.EVERYTHING_RUN_VERIFY === '1',
    native_release_build: true,
    daemon_smoke: true,
    model_pulled: process.env.EVERYTHING_PULL_MODEL === '1',
    live_model_smoke: process.env.EVERYTHING_RUN_VERIFY === '1' && process.env.EVERYTHING_PULL_MODEL === '1',
    runtime_doctor: Boolean(doctor),
    runtime_doctor_status: doctor?.overall_status || null,
    runtime_doctor_checks: Array.isArray(doctor?.checks) ? doctor.checks.length : 0
  },
  capabilities: {
    process_sandbox: exists('bwrap') || exists('sandbox-exec'),
    os_secret_vault: exists('secret-tool') || exists('security'),
    persistent_scheduler: process.env.EVERYTHING_INSTALL_SERVICE === '1',
    local_research_sidecar: process.env.EVERYTHING_RESEARCH_SIDECAR_STATUS || 'fallback'
  },
  toolchain: {
    node: commandVersion('node'),
    npm: commandVersion('npm'),
    cargo: commandVersion('cargo'),
    rustc: commandVersion('rustc'),
    ollama: commandVersion('ollama')
  }
};
for (const destination of [process.env.EVERYTHING_MANIFEST, process.env.EVERYTHING_STATE_MANIFEST]) {
  fs.mkdirSync(require('path').dirname(destination), {recursive: true, mode: 0o700});
  const temporary = `${destination}.tmp-${process.pid}`;
  fs.writeFileSync(temporary, `${JSON.stringify(manifest, null, 2)}\n`, {mode: 0o600});
  fs.renameSync(temporary, destination);
}
NODE
}

ensure_dependencies

log "Building native runtime"
cd "$ROOT"
if (( RUN_VERIFY )); then
  log "Running native workspace format, lint, and test gates"
  cargo fmt --all -- --check
  cargo clippy --locked --workspace --all-targets -- -D warnings
  cargo test --locked --workspace --all-targets
fi
cargo build --release --locked --workspace

log "Building desktop application"
cd "$ROOT/apps/everything-app"
retry 3 npm ci
retry 3 node node_modules/electron/install.js
if [[ $(uname -s) == "Darwin" ]]; then
  SOURCE_ELECTRON_BIN="$ROOT/apps/everything-app/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"
else
  SOURCE_ELECTRON_BIN="$ROOT/apps/everything-app/node_modules/electron/dist/electron"
fi
[[ -x "$SOURCE_ELECTRON_BIN" ]] || fail "Electron platform runtime was not installed correctly"
npm run typecheck
npm run build
npm audit --omit=dev --audit-level=high

if (( RUN_VERIFY )); then
  log "Running Python SDK tests"
  (
    set -e
    trap 'rm -rf "$ROOT/.venv-mvp"' EXIT
    cd "$ROOT/python/everything_control"
    rm -rf "$ROOT/.venv-mvp"
    if ! python3 -m venv "$ROOT/.venv-mvp"; then
      if (( INSTALL_DEPS )) && command -v apt-get >/dev/null 2>&1; then
        log "Installing Python venv support"
        run_as_root apt-get update
        run_as_root apt-get install -y python3-venv
        python3 -m venv "$ROOT/.venv-mvp"
      else
        fail "Python virtual environment creation failed. Install python3-venv or re-run with --install-deps."
      fi
    fi
    "$ROOT/.venv-mvp/bin/python" -m pip install --upgrade pip
    "$ROOT/.venv-mvp/bin/python" -m pip install -e '.[dev]'
    "$ROOT/.venv-mvp/bin/python" -m pytest
    "$ROOT/.venv-mvp/bin/python" -m build
  )
  cd "$ROOT"
  python3 scripts/smoke_mvp.py --require-built-ui
fi

log "Installing Everything into $INSTALL_DIR"
rm -rf "$INSTALL_DIR.tmp"
mkdir -p "$INSTALL_DIR.tmp/bin" "$INSTALL_DIR.tmp/app" "$INSTALL_DIR.tmp/deploy" "$INSTALL_DIR.tmp/scripts" "$BIN_DIR" "$HOME/.everything/skills"
cp "$ROOT/target/release/everythingd" "$INSTALL_DIR.tmp/bin/"
cp "$ROOT/target/release/everything-cli" "$INSTALL_DIR.tmp/bin/"
cp -R "$ROOT/apps/everything-app/out" "$INSTALL_DIR.tmp/app/out"
cp "$ROOT/apps/everything-app/package.json" "$INSTALL_DIR.tmp/app/package.json"
cp "$ROOT/apps/everything-app/package-lock.json" "$INSTALL_DIR.tmp/app/package-lock.json"
cp -R "$ROOT/apps/everything-app/node_modules" "$INSTALL_DIR.tmp/app/node_modules"
cp -R "$ROOT/deploy/searxng" "$INSTALL_DIR.tmp/deploy/searxng"
cp "$ROOT/scripts/research_sidecar.sh" "$INSTALL_DIR.tmp/scripts/research_sidecar.sh"
chmod +x "$INSTALL_DIR.tmp/scripts/research_sidecar.sh"
(
  cd "$INSTALL_DIR.tmp/app"
  npm prune --omit=dev
  npm audit --omit=dev --audit-level=high
)
sed "s|^model_name = .*|model_name = \"$MODEL\"|" "$ROOT/everything.toml" > "$INSTALL_DIR.tmp/everything.toml"
printf '%s\n' "$VERSION" > "$INSTALL_DIR.tmp/VERSION"
INSTALL_BACKUP="$INSTALL_DIR.previous"
rm -rf "$INSTALL_BACKUP"
if [[ -d "$INSTALL_DIR" ]]; then mv "$INSTALL_DIR" "$INSTALL_BACKUP"; fi
if ! mv "$INSTALL_DIR.tmp" "$INSTALL_DIR"; then
  rm -rf "$INSTALL_DIR"
  [[ -d "$INSTALL_BACKUP" ]] && mv "$INSTALL_BACKUP" "$INSTALL_DIR"
  fail "Atomic installation switch failed; previous installation was restored"
fi
INSTALL_SWITCHED=1
prepare_service_ports

cat > "$BIN_DIR/everything" <<EOF_LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
ENGINE_HOME="$INSTALL_DIR"
WORKSPACE="\${EVERYTHING_WORKSPACE:-\$PWD}"
if [[ \${1:-} == "--workspace" ]]; then
  WORKSPACE=\${2:?missing workspace}; shift 2
fi
mkdir -p "\$WORKSPACE"
WORKSPACE=\$(CDPATH= cd -- "\$WORKSPACE" && pwd)
if [[ ! -f "\$WORKSPACE/everything.toml" ]]; then
  cp "\$ENGINE_HOME/everything.toml" "\$WORKSPACE/everything.toml"
fi
export EVERYTHING_WORKSPACE="\$WORKSPACE"
export EVERYTHINGD_BIN="\$ENGINE_HOME/bin/everythingd"
export EVERYTHINGD_URL="\${EVERYTHINGD_URL:-http://127.0.0.1:$SERVICE_PORT}"
export EVERYTHING_HOME="\${EVERYTHING_HOME:-\$HOME/.everything}"
if [[ \$(uname -s) == "Darwin" ]]; then
  ELECTRON_BIN="\$ENGINE_HOME/app/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron"
else
  ELECTRON_BIN="\$ENGINE_HOME/app/node_modules/electron/dist/electron"
fi
if [[ ! -x "\$ELECTRON_BIN" ]]; then
  echo "Everything Electron runtime is missing: \$ELECTRON_BIN" >&2
  exit 1
fi
exec "\$ELECTRON_BIN" "\$ENGINE_HOME/app" "\$@"
EOF_LAUNCHER
chmod +x "$BIN_DIR/everything"

cat > "$BIN_DIR/everything-cli" <<EOF_CLI
#!/usr/bin/env bash
exec "$INSTALL_DIR/bin/everything-cli" "\$@"
EOF_CLI
chmod +x "$BIN_DIR/everything-cli"

if [[ ! -f "$WORKSPACE/everything.toml" ]]; then
  cp "$INSTALL_DIR/everything.toml" "$WORKSPACE/everything.toml"
fi

log "Running daemon smoke test"
SMOKE_PORT=$(node - <<'NODE'
const net = require('net');
const server = net.createServer();
server.listen(0, '127.0.0.1', () => {
  console.log(server.address().port);
  server.close();
});
NODE
)
"$INSTALL_DIR/bin/everythingd" --workspace "$WORKSPACE" --listen "127.0.0.1:$SMOKE_PORT" --oauth-listen "127.0.0.1:0" >"$INSTALL_DIR/smoke.log" 2>&1 &
SMOKE_PID=$!
cleanup_smoke() {
  if [[ -n "${SMOKE_PID:-}" ]]; then
    kill "$SMOKE_PID" >/dev/null 2>&1 || true
    wait "$SMOKE_PID" >/dev/null 2>&1 || true
    SMOKE_PID=""
  fi
}
node - "$SMOKE_PORT" <<'NODE'
const port = process.argv[2];
const deadline = Date.now() + 30000;
(async () => {
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/v1/info`);
      if (response.ok) process.exit(0);
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  process.exit(1);
})();
NODE
cleanup_smoke

if (( PULL_MODEL )); then
  command -v ollama >/dev/null 2>&1 || fail "Ollama is missing; use --install-deps or install it first."
  ensure_ollama_running || fail "Ollama could not be started"
  ensure_model_disk_capacity
  log "Pulling Ollama model $MODEL"
  retry 3 ollama pull "$MODEL" || fail "Model pull failed after retries"
  run_live_model_smoke
elif command -v ollama >/dev/null 2>&1; then
  if ! ollama list 2>/dev/null | awk 'NR>1 {print $1}' | grep -Fxq "$MODEL"; then
    log "Model $MODEL is not installed yet. Run: ollama pull '$MODEL'"
  fi
else
  log "Ollama is optional during installation. Install it before running local model tasks."
fi

if (( INSTALL_SERVICE )); then
  log "Installing the persistent scheduler and connector service"
  install_background_service
fi

start_research_sidecar
run_runtime_doctor

log "Writing the verified installation manifest"
write_install_manifest

INSTALL_COMPLETE=1
rm -rf "$INSTALL_BACKUP"
if (( LOCK_OWNED )); then rm -rf "$SETUP_LOCK_DIR"; LOCK_OWNED=0; fi
log "Installed successfully. Launcher: $BIN_DIR/everything"
printf '%s\n' "complete" > "$SETUP_STATE_DIR/current-stage"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) log "Add $BIN_DIR to PATH to run 'everything' from any terminal." ;;
esac

if (( LAUNCH )); then
  exec "$BIN_DIR/everything" --workspace "$WORKSPACE"
fi
