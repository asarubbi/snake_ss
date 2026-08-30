#!/usr/bin/env bash
# snake_ss installer — works piped from curl or run locally.
#
#   curl -fsSL https://example.com/install.sh | bash
#   bash install.sh
#
# Environment overrides:
#   SNAKE_SS_REPO     Git repo URL to clone from (when run via curl)
#   SNAKE_SS_TIMEOUT  Idle seconds before screensaver activates (default: 120)
#   SNAKE_SS_MODE     Screensaver mode: snake|starfield|lava|seismograph|balls (default: snake)
#   INSTALL_DIR       Binary destination (default: ~/.local/bin)
set -euo pipefail

INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="$HOME/.config/snake_ss"
SYSTEMD_DIR="$HOME/.config/systemd/user"
SNAKE_SS_TIMEOUT="${SNAKE_SS_TIMEOUT:-120}"
SNAKE_SS_MODE="${SNAKE_SS_MODE:-snake}"
SNAKE_SS_REPO="${SNAKE_SS_REPO:-https://github.com/asarubbi/snake_ss}"

# ── helpers ──────────────────────────────────────────────────────────────────

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
ok()    { printf '\033[1;32m  ✓\033[0m %s\n' "$*"; }
warn()  { printf '\033[1;33m  !\033[0m %s\n' "$*"; }
die()   { printf '\033[1;31mERROR:\033[0m %s\n' "$*" >&2; exit 1; }
need()  { command -v "$1" &>/dev/null || die "Required command not found: $1"; }

# ── detect invocation ─────────────────────────────────────────────────────────

PIPED=false
SOURCE_DIR=""
if [[ "$0" == "bash" || "$0" == "/bin/bash" || "$0" == "/usr/bin/bash" ]]; then
    PIPED=true
else
    SOURCE_DIR="$(cd "$(dirname "$0")" && pwd)"
fi

# ── check dependencies ────────────────────────────────────────────────────────

info "Checking dependencies..."
need cargo
[[ "$PIPED" == true ]] && need git

# ── obtain source ─────────────────────────────────────────────────────────────

if [[ "$PIPED" == true ]]; then
    info "Cloning source from $SNAKE_SS_REPO ..."
    BUILD_DIR="$(mktemp -d)"
    trap 'rm -rf "$BUILD_DIR"' EXIT
    git clone --depth 1 "$SNAKE_SS_REPO" "$BUILD_DIR"
else
    BUILD_DIR="$SOURCE_DIR"
fi

# ── build ─────────────────────────────────────────────────────────────────────

info "Building snake_ss (release)..."
cargo build --release --manifest-path "$BUILD_DIR/Cargo.toml" --quiet
ok "Build complete"

# ── install binary ────────────────────────────────────────────────────────────

info "Installing binary to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR"
cp "$BUILD_DIR/target/release/snake_ss" "$INSTALL_DIR/snake_ss"
chmod +x "$INSTALL_DIR/snake_ss"
ok "Binary installed: $INSTALL_DIR/snake_ss"

# ── write TTY daemon ──────────────────────────────────────────────────────────
# This is the real idle detection for headless TTY sessions.
# It polls the TTY's last-access time (updated on every keypress by the kernel)
# and launches the screensaver when idle >= SNAKE_SS_TIMEOUT seconds.
# Runs as a systemd user service so it starts automatically on login.

info "Writing TTY idle daemon to $CONFIG_DIR/snake_ss_daemon.sh..."
mkdir -p "$CONFIG_DIR"

cat > "$CONFIG_DIR/snake_ss_daemon.sh" << DAEMON_SH
#!/usr/bin/env bash
# snake_ss TTY idle daemon — launched by systemd user service.
# Watches the current TTY for inactivity and launches the screensaver.

TIMEOUT="${SNAKE_SS_TIMEOUT}"
BIN="${INSTALL_DIR}/snake_ss"
MODE_FLAG="${SNAKE_SS_MODE}"
[[ "\$MODE_FLAG" == "snake" ]] && MODE_FLAG=""  # snake is the default, no flag needed

_find_user_ttys() {
    # Find all TTYs owned by the current user (works from systemd service context
    # where tty(1) returns nothing because there is no controlling terminal).
    local uid
    uid=\$(id -u)
    for dev in /dev/tty[0-9]* /dev/pts/[0-9]*; do
        [[ -c "\$dev" ]] || continue
        local owner
        owner=\$(stat -c %u "\$dev" 2>/dev/null) || continue
        [[ "\$owner" == "\$uid" ]] && printf '%s\n' "\$dev"
    done
}

while true; do
    # Collect all TTYs owned by this user (handles systemd service context
    # where tty(1) returns nothing).
    mapfile -t TTYS < <(_find_user_ttys)
    if [[ \${#TTYS[@]} -eq 0 ]]; then
        sleep 10
        continue
    fi

    # Pick the TTY that has been idle the longest and track the most-idle one.
    TTY_DEV=""
    MAX_IDLE=0
    NOW=\$(date +%s)
    for dev in "\${TTYS[@]}"; do
        ATIME=\$(stat -c %X "\$dev" 2>/dev/null) || continue
        IDLE=\$(( NOW - ATIME ))
        if (( IDLE > MAX_IDLE )); then
            MAX_IDLE=\$IDLE
            TTY_DEV=\$dev
        fi
    done

    if [[ -z "\$TTY_DEV" ]]; then
        sleep 10
        continue
    fi

    IDLE=\$MAX_IDLE

    if (( IDLE >= TIMEOUT )); then
        # Launch screensaver directly on the TTY.
        # setsid detaches it from this daemon's process group so signals
        # from the screensaver don't kill the daemon.
        if [[ -n "\$MODE_FLAG" ]]; then
            setsid "\$BIN" --screensaver "--\${MODE_FLAG}" < "\$TTY_DEV" > "\$TTY_DEV" 2>&1
        else
            setsid "\$BIN" --screensaver < "\$TTY_DEV" > "\$TTY_DEV" 2>&1
        fi
        # After screensaver exits (keypress), wait a moment before re-arming
        # so the wake keypress doesn't immediately re-trigger idle logic.
        sleep 2
    else
        # Sleep until the idle threshold would be reached, minimum 5s
        SLEEP=\$(( TIMEOUT - IDLE ))
        sleep \$(( SLEEP > 5 ? SLEEP : 5 ))
    fi
done
DAEMON_SH

chmod +x "$CONFIG_DIR/snake_ss_daemon.sh"
ok "TTY daemon written"

# ── write systemd user service ────────────────────────────────────────────────

if command -v systemctl &>/dev/null && systemctl --user status &>/dev/null 2>&1; then
    info "Installing systemd user service..."
    mkdir -p "$SYSTEMD_DIR"

    cat > "$SYSTEMD_DIR/snake_ss.service" << SYSTEMD_UNIT
[Unit]
Description=snake_ss TTY idle screensaver daemon
After=default.target

[Service]
Type=simple
ExecStart=${CONFIG_DIR}/snake_ss_daemon.sh
Restart=always
RestartSec=5

[Install]
WantedBy=default.target
SYSTEMD_UNIT

    systemctl --user daemon-reload
    systemctl --user enable snake_ss.service
    systemctl --user start  snake_ss.service
    ok "systemd user service enabled and started"
    HAVE_SYSTEMD=true
else
    warn "systemd user session not available — skipping service install"
    warn "You can start the daemon manually: $CONFIG_DIR/snake_ss_daemon.sh &"
    HAVE_SYSTEMD=false
fi

# ── write bash/zsh shell integration (prompt-based fallback) ──────────────────
# This is a secondary fallback for interactive shells where the daemon
# might not cover (e.g. inside tmux with a new shell).

info "Writing shell integration to $CONFIG_DIR/snake_ss.sh..."

cat > "$CONFIG_DIR/snake_ss.sh" << 'SNAKE_SS_SH'
# snake_ss — bash/zsh idle screensaver integration (prompt-based fallback)
# The systemd daemon handles true mid-idle TTY activation.
# This adds coverage for zsh (native TRAPALRM) and bash (next-Enter fallback).

SNAKE_SS_TIMEOUT="${SNAKE_SS_TIMEOUT:-120}"
SNAKE_SS_BIN="${SNAKE_SS_BIN:-snake_ss}"

_snake_ss_resolve_bin() {
    if command -v "$SNAKE_SS_BIN" &>/dev/null; then
        printf '%s' "$SNAKE_SS_BIN"
        return
    fi
    local candidate="$HOME/.local/bin/snake_ss"
    [[ -x "$candidate" ]] && printf '%s' "$candidate"
}

_snake_ss_launch() {
    local bin
    bin="$(_snake_ss_resolve_bin)"
    [[ -z "$bin" ]] && return
    "$bin" --screensaver
}

# ZSH — native idle trap (fires mid-idle, no keypress needed)
if [[ -n "${ZSH_VERSION:-}" ]]; then
    TMOUT=$SNAKE_SS_TIMEOUT
    TRAPALRM() { _snake_ss_launch; }

# BASH — fires on next Enter press after idle period
elif [[ -n "${BASH_VERSION:-}" ]]; then
    _SNAKE_SS_LAST_ACTIVE=$(date +%s)

    _snake_ss_check() {
        local now elapsed
        now=$(date +%s)
        elapsed=$(( now - _SNAKE_SS_LAST_ACTIVE ))
        if (( elapsed >= SNAKE_SS_TIMEOUT )); then
            _snake_ss_launch
        fi
        _SNAKE_SS_LAST_ACTIVE=$(date +%s)
    }

    if [[ -z "${PROMPT_COMMAND:-}" ]]; then
        PROMPT_COMMAND='_snake_ss_check'
    elif [[ "$PROMPT_COMMAND" != *_snake_ss_check* ]]; then
        PROMPT_COMMAND="_snake_ss_check; $PROMPT_COMMAND"
    fi
fi
SNAKE_SS_SH

ok "Shell integration written"

# ── write fish integration ────────────────────────────────────────────────────

FISH_CONF_DIR="$HOME/.config/fish/conf.d"
if command -v fish &>/dev/null; then
    info "Writing fish integration to $FISH_CONF_DIR/snake_ss.fish..."
    mkdir -p "$FISH_CONF_DIR"

    cat > "$FISH_CONF_DIR/snake_ss.fish" << SNAKE_SS_FISH
# snake_ss — fish idle screensaver integration (prompt-based fallback)
set -g SNAKE_SS_TIMEOUT ${SNAKE_SS_TIMEOUT}
set -g _snake_ss_last_active (date +%s)

function _snake_ss_resolve_bin
    if command -q snake_ss
        echo snake_ss; return
    end
    set -l c \$HOME/.local/bin/snake_ss
    test -x \$c && echo \$c
end

function _snake_ss_check --on-event fish_prompt
    set -l now (date +%s)
    set -l elapsed (math \$now - \$_snake_ss_last_active)
    if test \$elapsed -ge \$SNAKE_SS_TIMEOUT
        set -l bin (_snake_ss_resolve_bin)
        test -n "\$bin" && \$bin --screensaver
    end
    set -g _snake_ss_last_active (date +%s)
end
SNAKE_SS_FISH

    ok "fish integration written"
else
    warn "fish not found — skipping fish integration"
fi

# ── wire up rc files ──────────────────────────────────────────────────────────

_ensure_path() {
    local rc="$1"
    [[ -f "$rc" ]] || return
    grep -q '\.local/bin' "$rc" 2>/dev/null && return
    printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$rc"
}

_ensure_posix_integration() {
    local rc="$1"
    [[ -f "$rc" ]] || return
    if grep -q 'snake_ss/snake_ss.sh' "$rc" 2>/dev/null; then
        warn "Already in $rc — skipped"
        return
    fi
    printf '\n# snake_ss screensaver\n' >> "$rc"
    printf 'export SNAKE_SS_TIMEOUT=%s\n' "$SNAKE_SS_TIMEOUT" >> "$rc"
    printf 'source "%s/snake_ss.sh"\n' "$CONFIG_DIR" >> "$rc"
    ok "Hooked into $rc"
}

info "Configuring shell rc files..."
for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
    _ensure_path "$rc"
    _ensure_posix_integration "$rc"
done

# ── done ─────────────────────────────────────────────────────────────────────

printf '\n'
info "Installation complete!"
printf '  Binary   : %s/snake_ss\n'        "$INSTALL_DIR"
printf '  Daemon   : %s/snake_ss_daemon.sh\n' "$CONFIG_DIR"
printf '  Timeout  : %s seconds\n'         "$SNAKE_SS_TIMEOUT"
printf '  Mode     : %s\n'                 "$SNAKE_SS_MODE"
printf '\n'
if [[ "${HAVE_SYSTEMD:-false}" == true ]]; then
    printf 'TTY idle detection active via systemd (true mid-idle, no keypress needed).\n'
    printf '  Status : systemctl --user status snake_ss\n'
    printf '  Logs   : journalctl --user -u snake_ss -f\n'
    printf '  Stop   : systemctl --user disable --now snake_ss\n'
else
    printf 'systemd not available. Start the daemon manually:\n'
    printf '  %s/snake_ss_daemon.sh &\n' "$CONFIG_DIR"
    printf 'Or add it to your ~/.profile to start on login.\n'
fi
printf '\n'
printf 'Shell integration also active (zsh: true idle  bash/fish: next-Enter).\n'
printf 'Reload your shell: source ~/.zshrc  or  source ~/.bashrc\n'
printf '\n'
printf 'To change mode or timeout, re-run with env vars:\n'
printf '  SNAKE_SS_TIMEOUT=60 SNAKE_SS_MODE=starfield bash install.sh\n'
