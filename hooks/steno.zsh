# Stenographer shell hook for zsh
# Appends NDJSON events to spool file for daemon ingestion

_steno_init() {
    # Generate session UUID
    if [[ -z "$STENO_SESSION_ID" ]]; then
        if [[ -f /proc/sys/kernel/random/uuid ]]; then
            export STENO_SESSION_ID="$(cat /proc/sys/kernel/random/uuid)"
        else
            export STENO_SESSION_ID="$(uuidgen 2>/dev/null || python3 -c 'import uuid; print(uuid.uuid4())' 2>/dev/null || echo "unknown-$$-$(date +%s)")"
        fi
    fi

    export STENO_SEQ=0

    # Determine spool path
    local runtime_dir="${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-/tmp}}"
    export STENO_SPOOL="${runtime_dir}/stenographer/spool.ndjson"
    mkdir -p "$(dirname "$STENO_SPOOL")" 2>/dev/null

    # Emit start event
    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '{"t":"start","sid":"%s","pid":%d,"shell":"zsh","tty":"%s","cwd":"%s","host":"%s","user":"%s","ts":"%s"' \
        "$STENO_SESSION_ID" "$$" "$(tty 2>/dev/null || echo unknown)" "$PWD" "$(hostname -s)" "$USER" "$ts" >> "$STENO_SPOOL"

    [[ -n "$PPID" ]] && printf ',"ppid":%d' "$PPID" >> "$STENO_SPOOL"
    [[ -n "$TMUX_PANE" ]] && printf ',"tmux_pane":"%s"' "$TMUX_PANE" >> "$STENO_SPOOL"
    [[ -n "$SSH_CONNECTION" ]] && printf ',"ssh_conn":"%s"' "$SSH_CONNECTION" >> "$STENO_SPOOL"
    printf '}\n' >> "$STENO_SPOOL"
}

_steno_preexec() {
    local cmd="$1"
    [[ -z "$cmd" ]] && return
    [[ -z "$STENO_SESSION_ID" ]] && return

    STENO_SEQ=$((STENO_SEQ + 1))
    _STENO_CURRENT_SEQ=$STENO_SEQ

    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"

    # Canonical cwd
    local ccwd
    ccwd="$(realpath "$PWD" 2>/dev/null || echo "$PWD")"

    # Escape JSON special characters
    local escaped_cmd="${cmd//\\/\\\\}"
    escaped_cmd="${escaped_cmd//\"/\\\"}"
    escaped_cmd="${escaped_cmd//$'\t'/\\t}"
    escaped_cmd="${escaped_cmd//$'\n'/\\n}"

    printf '{"t":"pre","sid":"%s","seq":%d,"cmd":"%s","cwd":"%s","ccwd":"%s","ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$STENO_SEQ" "$escaped_cmd" "$PWD" "$ccwd" "$ts" >> "$STENO_SPOOL"
}

_steno_precmd() {
    local exit_code=$?
    [[ -z "$STENO_SESSION_ID" ]] && return
    [[ -z "$_STENO_CURRENT_SEQ" ]] && return

    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"

    printf '{"t":"post","sid":"%s","seq":%d,"exit":%d,"ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$_STENO_CURRENT_SEQ" "$exit_code" "$ts" >> "$STENO_SPOOL"

    unset _STENO_CURRENT_SEQ
}

_steno_exit() {
    local exit_code=$?
    [[ -z "$STENO_SESSION_ID" ]] && return

    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%S.%3NZ 2>/dev/null || date -u +%Y-%m-%dT%H:%M:%SZ)"

    printf '{"t":"end","sid":"%s","exit":%d,"ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$exit_code" "$ts" >> "$STENO_SPOOL"
}

# Install hooks
_steno_init

# zsh has native preexec/precmd hooks
autoload -Uz add-zsh-hook
add-zsh-hook preexec _steno_preexec
add-zsh-hook precmd _steno_precmd
trap '_steno_exit' EXIT
