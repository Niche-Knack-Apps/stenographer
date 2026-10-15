# Stenographer shell hook for fish
# Appends NDJSON events to spool file for daemon ingestion

function _steno_init
    # Generate session UUID
    if not set -q STENO_SESSION_ID
        if test -f /proc/sys/kernel/random/uuid
            set -gx STENO_SESSION_ID (cat /proc/sys/kernel/random/uuid)
        else if command -q uuidgen
            set -gx STENO_SESSION_ID (uuidgen)
        else
            set -gx STENO_SESSION_ID "unknown-"(echo %self)"-"(date +%s)
        end
    end

    set -gx STENO_SEQ 0

    # Determine spool path
    if set -q XDG_RUNTIME_DIR
        set -gx STENO_SPOOL "$XDG_RUNTIME_DIR/stenographer/spool.ndjson"
    else if set -q XDG_STATE_HOME
        set -gx STENO_SPOOL "$XDG_STATE_HOME/stenographer/spool.ndjson"
    else
        set -gx STENO_SPOOL "/tmp/stenographer/spool.ndjson"
    end

    mkdir -p (dirname "$STENO_SPOOL") 2>/dev/null

    # Emit start event
    set -l ts (date -u +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null; or date -u +%Y-%m-%dT%H:%M:%SZ)
    set -l tty_name (tty 2>/dev/null; or echo unknown)

    printf '{"t":"start","sid":"%s","pid":%d,"shell":"fish","tty":"%s","cwd":"%s","host":"%s","user":"%s","ts":"%s"}\n' \
        "$STENO_SESSION_ID" %self "$tty_name" "$PWD" (hostname -s) "$USER" "$ts" >> "$STENO_SPOOL"
end

function _steno_preexec --on-event fish_preexec
    set -l cmd $argv[1]
    test -z "$cmd"; and return
    test -z "$STENO_SESSION_ID"; and return

    set -gx STENO_SEQ (math $STENO_SEQ + 1)
    set -g _STENO_CURRENT_SEQ $STENO_SEQ

    set -l ts (date -u +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null; or date -u +%Y-%m-%dT%H:%M:%SZ)

    # Escape for JSON
    set -l escaped_cmd (string replace -a '\\' '\\\\' -- "$cmd" | string replace -a '"' '\\"' | string replace -a \t '\\t' | string replace -a \n '\\n')

    printf '{"t":"pre","sid":"%s","seq":%d,"cmd":"%s","cwd":"%s","ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$STENO_SEQ" "$escaped_cmd" "$PWD" "$ts" >> "$STENO_SPOOL"
end

function _steno_postexec --on-event fish_postexec
    set -l exit_code $status
    test -z "$STENO_SESSION_ID"; and return
    test -z "$_STENO_CURRENT_SEQ"; and return

    set -l ts (date -u +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null; or date -u +%Y-%m-%dT%H:%M:%SZ)

    printf '{"t":"post","sid":"%s","seq":%d,"exit":%d,"ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$_STENO_CURRENT_SEQ" "$exit_code" "$ts" >> "$STENO_SPOOL"

    set -e _STENO_CURRENT_SEQ
end

function _steno_exit --on-event fish_exit
    test -z "$STENO_SESSION_ID"; and return

    set -l ts (date -u +%Y-%m-%dT%H:%M:%S.000Z 2>/dev/null; or date -u +%Y-%m-%dT%H:%M:%SZ)

    printf '{"t":"end","sid":"%s","exit":0,"ts":"%s"}\n' \
        "$STENO_SESSION_ID" "$ts" >> "$STENO_SPOOL"
end

# Initialize
_steno_init
