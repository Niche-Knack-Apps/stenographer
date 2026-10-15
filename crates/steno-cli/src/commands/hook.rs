use std::path::Path;

use crate::cli::{HookAction, ShellType};
use crate::ui::output;

const HOOK_GUARD: &str = "# [stenographer hook]";
const HOOKS_DIR: &str = ".local/share/com.niche-knack.stenographer/hooks";

pub fn run(action: HookAction) -> anyhow::Result<()> {
    match action {
        HookAction::Install { shell } => install(shell),
        HookAction::Uninstall => uninstall(),
        HookAction::Status => status(),
    }
}

fn install(shell: Option<ShellType>) -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let hooks_dir = home.join(HOOKS_DIR);
    std::fs::create_dir_all(&hooks_dir)?;

    let shell_type = shell.unwrap_or(ShellType::All);

    match shell_type {
        ShellType::Bash => install_bash(&home, &hooks_dir)?,
        ShellType::Zsh => install_zsh(&home, &hooks_dir)?,
        ShellType::Fish => install_fish(&home, &hooks_dir)?,
        ShellType::All => {
            install_bash(&home, &hooks_dir)?;
            install_zsh(&home, &hooks_dir)?;
            install_fish(&home, &hooks_dir)?;
        }
    }

    Ok(())
}

fn spool_path_str() -> String {
    steno_core::config::spool_path()
        .to_string_lossy()
        .to_string()
}

fn runtime_dir_str() -> String {
    steno_core::config::runtime_dir()
        .to_string_lossy()
        .to_string()
}

fn generate_bash_hook() -> String {
    let spool = spool_path_str();
    let runtime = runtime_dir_str();
    format!(
        r##"# Stenographer shell hook for bash
# Auto-generated — do not edit manually

__steno_spool="{spool}"
__steno_runtime="{runtime}"
__steno_sid=""
__steno_seq=0

__steno_ts() {{
    date -u +"%Y-%m-%dT%H:%M:%S.000Z"
}}

__steno_json_escape() {{
    local s="$1"
    s="${{s//\\/\\\\}}"
    s="${{s//\"/\\\"}}"
    s="${{s//$'\n'/\\n}}"
    s="${{s//$'\t'/\\t}}"
    printf '%s' "$s"
}}

__steno_emit() {{
    mkdir -p "$__steno_runtime"
    printf '%s\n' "$1" >> "$__steno_spool"
}}

__steno_session_start() {{
    if [[ -n "$__steno_sid" ]]; then
        return
    fi
    __steno_sid="bash-$(cat /proc/sys/kernel/random/uuid 2>/dev/null || uuidgen 2>/dev/null || echo $$-$(date +%s))"
    __steno_seq=0
    local ts
    ts=$(__steno_ts)
    local tty_name
    tty_name=$(tty 2>/dev/null || echo "")
    local tmux_pane="${{TMUX_PANE:-}}"
    local ssh_conn="${{SSH_CONNECTION:-}}"
    local ppid_val=$$

    local json="{{"
    json+="\"t\":\"start\""
    json+=",\"sid\":\"$__steno_sid\""
    json+=",\"pid\":$$"
    json+=",\"shell\":\"bash\""
    if [[ -n "$tty_name" ]]; then json+=",\"tty\":\"$tty_name\""; fi
    json+=",\"cwd\":\"$(__steno_json_escape "$PWD")\""
    json+=",\"host\":\"$(hostname -s 2>/dev/null || echo unknown)\""
    json+=",\"user\":\"$USER\""
    json+=",\"ts\":\"$ts\""
    if [[ -n "$tmux_pane" ]]; then json+=",\"tmux_pane\":\"$tmux_pane\""; fi
    if [[ -n "$ssh_conn" ]]; then json+=",\"ssh_conn\":\"$(__steno_json_escape "$ssh_conn")\""; fi
    json+="}}"

    __steno_emit "$json"
}}

__steno_preexec() {{
    __steno_session_start
    __steno_seq=$(( __steno_seq + 1 ))
    local ts
    ts=$(__steno_ts)
    local cmd
    cmd=$(__steno_json_escape "$1")
    local cwd
    cwd=$(__steno_json_escape "$PWD")

    __steno_emit "{{\"t\":\"pre\",\"sid\":\"$__steno_sid\",\"seq\":$__steno_seq,\"cmd\":\"$cmd\",\"cwd\":\"$cwd\",\"ts\":\"$ts\"}}"
    __steno_cmd_start=$SECONDS
}}

__steno_postexec() {{
    local exit_code=$?
    if [[ -z "$__steno_sid" ]] || [[ "$__steno_seq" -eq 0 ]]; then
        return
    fi
    local ts
    ts=$(__steno_ts)

    __steno_emit "{{\"t\":\"post\",\"sid\":\"$__steno_sid\",\"seq\":$__steno_seq,\"exit\":$exit_code,\"ts\":\"$ts\"}}"
}}

# Use DEBUG trap for preexec and PROMPT_COMMAND for postexec
__steno_last_cmd=""
__steno_debug_trap() {{
    # Avoid firing for PROMPT_COMMAND itself and subshells
    [[ "$BASH_COMMAND" == "$PROMPT_COMMAND" ]] && return
    [[ "$BASH_COMMAND" == __steno_* ]] && return

    # Only fire once per command (the first DEBUG after prompt)
    if [[ "$BASH_COMMAND" != "$__steno_last_cmd" ]]; then
        __steno_last_cmd="$BASH_COMMAND"
        # For multi-line commands, get the full command from history
        local full_cmd
        full_cmd=$(HISTTIMEFORMAT='' history 1 | sed 's/^[ ]*[0-9]*[ ]*//')
        __steno_preexec "$full_cmd"
    fi
}}

__steno_prompt_command() {{
    __steno_postexec
    __steno_last_cmd=""
}}

# Install hooks
if [[ -z "$__steno_hooked" ]]; then
    __steno_hooked=1
    trap '__steno_debug_trap' DEBUG
    if [[ -z "$PROMPT_COMMAND" ]]; then
        PROMPT_COMMAND="__steno_prompt_command"
    else
        PROMPT_COMMAND="__steno_prompt_command;$PROMPT_COMMAND"
    fi
fi

# `steno history` / `steno here` run the TUI normally (no subshell capture),
# write the selection to a temp file, then inject into readline via bind -x.
# This is the atuin pattern — it's the only reliable way in bash.
__steno_pick() {{
    local tmpfile result
    tmpfile=$(mktemp /tmp/steno-result.XXXXXX)
    STENO_RESULT_FILE="$tmpfile" command steno "$@"
    if [[ -s "$tmpfile" ]]; then
        result=$(cat "$tmpfile")
        if [[ "$result" == EXEC:* ]]; then
            eval "${{result#EXEC:}}"
        else
            READLINE_LINE="$result"
            READLINE_POINT="${{#READLINE_LINE}}"
        fi
    fi
    rm -f "$tmpfile"
}}
steno() {{
    case "$1" in
        ""|history|here) __steno_pick "$@" ;;
        *)               command steno "$@" ;;
    esac
}}
bind -x '"\C-r": __steno_pick history' 2>/dev/null || true
"##
    )
}

fn generate_zsh_hook() -> String {
    let spool = spool_path_str();
    let runtime = runtime_dir_str();
    format!(
        r##"# Stenographer shell hook for zsh
# Auto-generated — do not edit manually

__steno_spool="{spool}"
__steno_runtime="{runtime}"
__steno_sid=""
__steno_seq=0

__steno_ts() {{
    date -u +"%Y-%m-%dT%H:%M:%S.000Z"
}}

__steno_json_escape() {{
    local s="$1"
    s="${{s//\\/\\\\}}"
    s="${{s//\"/\\\"}}"
    s="${{s//$'\n'/\\n}}"
    s="${{s//$'\t'/\\t}}"
    printf '%s' "$s"
}}

__steno_emit() {{
    mkdir -p "$__steno_runtime"
    printf '%s\n' "$1" >> "$__steno_spool"
}}

__steno_session_start() {{
    if [[ -n "$__steno_sid" ]]; then
        return
    fi
    __steno_sid="zsh-${{(%):-%m}}-$$-$RANDOM$RANDOM"
    __steno_seq=0
    local ts
    ts=$(__steno_ts)
    local tty_name
    tty_name=$(tty 2>/dev/null || echo "")
    local tmux_pane="${{TMUX_PANE:-}}"
    local ssh_conn="${{SSH_CONNECTION:-}}"

    local json="{{"
    json+="\"t\":\"start\""
    json+=",\"sid\":\"$__steno_sid\""
    json+=",\"pid\":$$"
    json+=",\"shell\":\"zsh\""
    if [[ -n "$tty_name" ]]; then json+=",\"tty\":\"$tty_name\""; fi
    json+=",\"cwd\":\"$(__steno_json_escape "$PWD")\""
    json+=",\"host\":\"$(hostname -s 2>/dev/null || echo unknown)\""
    json+=",\"user\":\"$USER\""
    json+=",\"ts\":\"$ts\""
    if [[ -n "$tmux_pane" ]]; then json+=",\"tmux_pane\":\"$tmux_pane\""; fi
    if [[ -n "$ssh_conn" ]]; then json+=",\"ssh_conn\":\"$(__steno_json_escape "$ssh_conn")\""; fi
    json+="}}"

    __steno_emit "$json"
}}

__steno_preexec() {{
    __steno_session_start
    __steno_seq=$(( __steno_seq + 1 ))
    local ts
    ts=$(__steno_ts)
    local cmd
    cmd=$(__steno_json_escape "$1")
    local cwd
    cwd=$(__steno_json_escape "$PWD")

    __steno_emit "{{\"t\":\"pre\",\"sid\":\"$__steno_sid\",\"seq\":$__steno_seq,\"cmd\":\"$cmd\",\"cwd\":\"$cwd\",\"ts\":\"$ts\"}}"
}}

__steno_precmd() {{
    local exit_code=$?
    if [[ -z "$__steno_sid" ]] || [[ "$__steno_seq" -eq 0 ]]; then
        return
    fi
    local ts
    ts=$(__steno_ts)

    __steno_emit "{{\"t\":\"post\",\"sid\":\"$__steno_sid\",\"seq\":$__steno_seq,\"exit\":$exit_code,\"ts\":\"$ts\"}}"
}}

# Install hooks via zsh hook system
if [[ -z "$__steno_hooked" ]]; then
    __steno_hooked=1
    autoload -Uz add-zsh-hook
    add-zsh-hook preexec __steno_preexec
    add-zsh-hook precmd __steno_precmd
fi

# `steno history` / `steno here` run the TUI on the real terminal,
# write the selection to a temp file, then print -z pushes it onto
# zsh's input stack so it appears at your prompt ready to edit or run.
steno() {{
    case "$1" in
        ""|history|here)
            local tmpfile result
            tmpfile=$(mktemp /tmp/steno-result.XXXXXX)
            STENO_RESULT_FILE="$tmpfile" command steno "$@"
            if [[ -s "$tmpfile" ]]; then
                result="$(<"$tmpfile")"
                if [[ "$result" == EXEC:* ]]; then
                    eval "${{result#EXEC:}}"
                else
                    print -z -- "$result"
                fi
            fi
            rm -f "$tmpfile"
            ;;
        *) command steno "$@" ;;
    esac
}}
# Ctrl-R for keyboard-first users (uses the same temp file mechanism)
__steno_widget() {{
    steno history
}}
zle -N __steno_widget
bindkey '^R' __steno_widget
"##
    )
}

fn generate_fish_hook() -> String {
    let spool = spool_path_str();
    let runtime = runtime_dir_str();
    format!(
        r##"# Stenographer shell hook for fish
# Auto-generated — do not edit manually

set -g __steno_spool "{spool}"
set -g __steno_runtime "{runtime}"
set -g __steno_sid ""
set -g __steno_seq 0

function __steno_ts
    date -u +"%Y-%m-%dT%H:%M:%S.000Z"
end

function __steno_json_escape
    string replace -a '\\' '\\\\' -- $argv[1] | \
    string replace -a '"' '\\"' | \
    string replace -a \n '\\n' | \
    string replace -a \t '\\t'
end

function __steno_emit
    mkdir -p "$__steno_runtime"
    echo $argv[1] >> "$__steno_spool"
end

function __steno_session_start
    if test -n "$__steno_sid"
        return
    end
    set -g __steno_sid "fish-"(random)"-"(random)"-"%self
    set -g __steno_seq 0
    set -l ts (__steno_ts)
    set -l tty_name (tty 2>/dev/null; or echo "")
    set -l tmux_pane "$TMUX_PANE"
    set -l ssh_conn "$SSH_CONNECTION"

    set -l json '{{'
    set json $json'"t":"start"'
    set json $json',"sid":"'$__steno_sid'"'
    set json $json',"pid":'%self
    set json $json',"shell":"fish"'
    if test -n "$tty_name"; set json $json',"tty":"'$tty_name'"'; end
    set json $json',"cwd":"'(__steno_json_escape "$PWD")'"'
    set json $json',"host":"'(hostname -s 2>/dev/null; or echo unknown)'"'
    set json $json',"user":"'$USER'"'
    set json $json',"ts":"'$ts'"'
    if test -n "$tmux_pane"; set json $json',"tmux_pane":"'$tmux_pane'"'; end
    if test -n "$ssh_conn"; set json $json',"ssh_conn":"'(__steno_json_escape "$ssh_conn")'"'; end
    set json $json'}}'

    __steno_emit $json
end

function __steno_preexec --on-event fish_preexec
    __steno_session_start
    set -g __steno_seq (math $__steno_seq + 1)
    set -l ts (__steno_ts)
    set -l cmd (__steno_json_escape "$argv[1]")
    set -l cwd (__steno_json_escape "$PWD")

    __steno_emit '{{"t":"pre","sid":"'$__steno_sid'","seq":'$__steno_seq',"cmd":"'$cmd'","cwd":"'$cwd'","ts":"'$ts'"}}'
end

function __steno_postexec --on-event fish_postexec
    set -l exit_code $status
    if test -z "$__steno_sid"; or test "$__steno_seq" -eq 0
        return
    end
    set -l ts (__steno_ts)

    __steno_emit '{{"t":"post","sid":"'$__steno_sid'","seq":'$__steno_seq',"exit":'$exit_code',"ts":"'$ts'"}}'
end

# `steno history` / `steno here` run the TUI on the real terminal,
# write the selection to a temp file, then inject it into the fish commandline.
function steno
    switch $argv[1]
        case '' history here
            set tmpfile (mktemp /tmp/steno-result.XXXXXX)
            STENO_RESULT_FILE="$tmpfile" command steno $argv
            if test -s "$tmpfile"
                set result (cat "$tmpfile")
                if string match -q 'EXEC:*' -- "$result"
                    eval (string replace --regex '^EXEC:' '' -- "$result")
                else
                    commandline --replace "$result"
                    commandline --function repaint
                end
            end
            rm -f "$tmpfile"
        case '*'
            command steno $argv
    end
end
bind \cr 'steno history'
"##
    )
}

fn install_bash(home: &Path, hooks_dir: &Path) -> anyhow::Result<()> {
    let hook_path = hooks_dir.join("steno.bash");
    std::fs::write(&hook_path, generate_bash_hook())?;
    output::success(&format!("Generated {}", hook_path.display()));

    let rc = home.join(".bashrc");
    let line = format!(
        "{}\n[ -f \"{}\" ] && source \"{}\"\n",
        HOOK_GUARD,
        hook_path.display(),
        hook_path.display()
    );
    append_to_rc(&rc, &line, "bash")
}

fn install_zsh(home: &Path, hooks_dir: &Path) -> anyhow::Result<()> {
    let hook_path = hooks_dir.join("steno.zsh");
    std::fs::write(&hook_path, generate_zsh_hook())?;
    output::success(&format!("Generated {}", hook_path.display()));

    let rc = home.join(".zshrc");
    let line = format!(
        "{}\n[ -f \"{}\" ] && source \"{}\"\n",
        HOOK_GUARD,
        hook_path.display(),
        hook_path.display()
    );
    append_to_rc(&rc, &line, "zsh")
}

fn install_fish(home: &Path, hooks_dir: &Path) -> anyhow::Result<()> {
    let hook_path = hooks_dir.join("steno.fish");
    std::fs::write(&hook_path, generate_fish_hook())?;
    output::success(&format!("Generated {}", hook_path.display()));

    let conf_d = home.join(".config/fish/conf.d");
    let symlink_path = conf_d.join("steno.fish");

    std::fs::create_dir_all(&conf_d)?;

    if symlink_path.exists() || symlink_path.is_symlink() {
        output::info("fish hook already installed");
        return Ok(());
    }

    std::os::unix::fs::symlink(&hook_path, &symlink_path)?;
    output::success("Installed fish hook");
    Ok(())
}

fn append_to_rc(rc_path: &Path, line: &str, shell_name: &str) -> anyhow::Result<()> {
    if let Ok(contents) = std::fs::read_to_string(rc_path)
        && contents.contains(HOOK_GUARD)
    {
        output::info(&format!("{} hook already installed", shell_name));
        return Ok(());
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(rc_path)?;

    use std::io::Write;
    writeln!(file)?;
    write!(file, "{}", line)?;

    output::success(&format!(
        "Installed {} hook in {}",
        shell_name,
        rc_path.display()
    ));
    Ok(())
}

fn uninstall() -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;

    remove_from_rc(&home.join(".bashrc"), "bash")?;
    remove_from_rc(&home.join(".zshrc"), "zsh")?;

    let fish_link = home.join(".config/fish/conf.d/steno.fish");
    if fish_link.is_symlink() {
        std::fs::remove_file(&fish_link)?;
        output::success("Removed fish hook");
    }

    // Clean up generated hook scripts
    let hooks_dir = home.join(HOOKS_DIR);
    for name in &["steno.bash", "steno.zsh", "steno.fish"] {
        let path = hooks_dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
    }

    Ok(())
}

fn remove_from_rc(rc_path: &Path, shell_name: &str) -> anyhow::Result<()> {
    if let Ok(contents) = std::fs::read_to_string(rc_path)
        && contents.contains(HOOK_GUARD)
    {
        let new_contents: Vec<&str> = contents
            .lines()
            .filter(|line| !line.contains(HOOK_GUARD) && !line.contains("steno."))
            .collect();
        std::fs::write(rc_path, new_contents.join("\n"))?;
        output::success(&format!(
            "Removed {} hook from {}",
            shell_name,
            rc_path.display()
        ));
    }
    Ok(())
}

fn status() -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let hooks_dir = home.join(HOOKS_DIR);

    for name in &["steno.bash", "steno.zsh", "steno.fish"] {
        let path = hooks_dir.join(name);
        if path.exists() {
            output::success(&format!("{} installed", name));
        } else {
            output::warn(&format!("{} not found", name));
        }
    }

    let bashrc = home.join(".bashrc");
    if let Ok(c) = std::fs::read_to_string(&bashrc)
        && c.contains(HOOK_GUARD)
    {
        output::success("bash hook active in .bashrc");
    }

    let zshrc = home.join(".zshrc");
    if let Ok(c) = std::fs::read_to_string(&zshrc)
        && c.contains(HOOK_GUARD)
    {
        output::success("zsh hook active in .zshrc");
    }

    let fish_link = home.join(".config/fish/conf.d/steno.fish");
    if fish_link.is_symlink() {
        output::success("fish hook active");
    }

    Ok(())
}
