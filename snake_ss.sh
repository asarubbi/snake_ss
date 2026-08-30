#!/usr/bin/env bash
# snake_ss shell integration — source from ~/.bashrc or ~/.zshrc
# Fish users: see snake_ss.fish (written by install.sh to ~/.config/fish/conf.d/)
#
# zsh:  true idle detection via TRAPALRM (fires mid-idle at the prompt)
# bash: fires on the next Enter press after the idle period has elapsed

SNAKE_SS_TIMEOUT="${SNAKE_SS_TIMEOUT:-120}"
SNAKE_SS_BIN="${SNAKE_SS_BIN:-snake_ss}"

_snake_ss_resolve_bin() {
    if command -v "$SNAKE_SS_BIN" &>/dev/null; then
        printf '%s' "$SNAKE_SS_BIN"
        return
    fi
    local candidate="$HOME/.local/bin/snake_ss"
    if [[ -x "$candidate" ]]; then
        printf '%s' "$candidate"
    fi
}

_snake_ss_launch() {
    local bin
    bin="$(_snake_ss_resolve_bin)"
    [[ -z "$bin" ]] && return
    "$bin" --screensaver
}

# ZSH — native idle trap
if [[ -n "${ZSH_VERSION:-}" ]]; then
    TMOUT=$SNAKE_SS_TIMEOUT
    TRAPALRM() { _snake_ss_launch; }

# BASH — check elapsed time at each prompt (fires on next Enter after idle)
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
