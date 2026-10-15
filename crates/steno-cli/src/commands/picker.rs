use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyModifiers};

use colored::Colorize;
use skim::prelude::{SkimOptionsBuilder, bounded};
use skim::tui::options::PreviewLayout;
use skim::{ItemPreview, PreviewContext, Skim, SkimItem, SkimItemReceiver, SkimItemSender};

use steno_core::config;
use steno_core::ipc::methods::Method;
use steno_core::model::StoredEvent;

use crate::client::RpcClient;
use crate::ui::output;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerAction {
    Print,
    Run,
}

/// Returns true if the command is a shell builtin that must run in the parent shell.
fn is_shell_builtin(cmd: &str) -> bool {
    let first_word = cmd.split_whitespace().next().unwrap_or("");
    matches!(
        first_word,
        "cd" | "export"
            | "source"
            | "."
            | "eval"
            | "alias"
            | "unalias"
            | "set"
            | "unset"
            | "pushd"
            | "popd"
            | "ulimit"
            | "umask"
    )
}

/// Write the selected command to STENO_RESULT_FILE (shell widget path) or stdout.
/// The shell wrapper reads this and uses `print -z` to place it at the prompt.
pub fn emit_command(command: &str, result_file: Option<&str>) {
    if let Some(path) = result_file {
        let _ = std::fs::write(path, command);
    } else {
        println!("{}", command);
    }
}

/// Write an EXEC-prefixed command to STENO_RESULT_FILE so the shell wrapper
/// runs it via `eval` in the interactive shell instead of placing it at the prompt.
/// Falls back to a subprocess only when there is no shell wrapper active.
pub fn emit_execute(command: &str, result_file: Option<&str>) {
    if let Some(path) = result_file {
        let _ = std::fs::write(path, format!("EXEC:{}", command));
    } else {
        execute_command(command);
    }
}

/// Execute a command: runs in subprocess, or prints for shell eval if it's a builtin.
pub fn execute_command(command: &str) {
    if is_shell_builtin(command) {
        // Shell builtins must run in the parent shell — print to stdout for eval
        eprintln!(
            "{} {}",
            "▸".blue().bold(),
            "(printing for shell eval)".dimmed()
        );
        println!("{}", command);
    } else {
        eprintln!("{} {}", "▸".blue().bold(), command.white().bold());
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(command)
            .status();
        match status {
            Ok(s) if s.success() => output::success("Done"),
            Ok(s) => output::warn(&format!("Exit code: {}", s.code().unwrap_or(-1))),
            Err(e) => output::error(&format!("Failed to execute: {}", e)),
        }
    }
}

pub fn resolve_action(run_flag: bool, print_flag: bool) -> PickerAction {
    if run_flag {
        return PickerAction::Run;
    }
    if print_flag {
        return PickerAction::Print;
    }
    let cfg = config::load_config();
    if cfg.history_action == "run" {
        PickerAction::Run
    } else {
        PickerAction::Print
    }
}

struct HistoryItem {
    event: StoredEvent,
    display_text: String,
    session_display_id: i64,
    client: Arc<Mutex<RpcClient>>,
}

impl SkimItem for HistoryItem {
    fn text(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.display_text)
    }

    fn output(&self) -> Cow<'_, str> {
        if let Some(ref canonical) = self.event.cd_target_canonical {
            return Cow::Owned(format!("cd {}", canonical));
        }

        let cmd = &self.event.command;
        let first_word = cmd.split_whitespace().next().unwrap_or("");

        // Absolutize relative-path executables (e.g. `zarf/extras/setup.sh`).
        // A first token qualifies when it contains '/' (path-like), is not already
        // absolute, and is not an env-var assignment (no '=').
        if !first_word.is_empty()
            && first_word.contains('/')
            && !first_word.starts_with('/')
            && !first_word.contains('=')
        {
            let cwd = self
                .event
                .canonical_cwd
                .as_deref()
                .unwrap_or(&self.event.cwd);
            let abs = std::path::Path::new(cwd).join(first_word);
            if let Some(abs_str) = abs.to_str() {
                let rest = &cmd[first_word.len()..];
                return Cow::Owned(format!("{}{}", abs_str, rest));
            }
        }

        Cow::Borrowed(cmd)
    }

    fn preview(&self, _context: PreviewContext) -> ItemPreview {
        let mut lines = Vec::new();

        // Command
        lines.push(format!("\x1b[1;37m{}\x1b[0m", self.event.command));
        lines.push(String::new());

        // Metadata
        lines.push(format!(
            "  Session: \x1b[1;36m#{}\x1b[0m",
            self.session_display_id
        ));
        lines.push(format!("  CWD:     {}", self.event.cwd));
        if let Some(ref canonical) = self.event.cd_target_canonical {
            let expanded = format!("cd {}", canonical);
            if expanded != self.event.command {
                lines.push(format!(
                    "  Expanded:\x1b[1;32m {}\x1b[0m  \x1b[2m(was: {})\x1b[0m",
                    expanded, self.event.command
                ));
            }
        }
        lines.push(format!(
            "  Time:    {}",
            format_timestamp(&self.event.started_at)
        ));
        if let Some(code) = self.event.exit_code {
            let color = if code == 0 { "32" } else { "31" };
            lines.push(format!("  Exit:    \x1b[{}m{}\x1b[0m", color, code));
        }
        if let Some(ms) = self.event.duration_ms {
            lines.push(format!("  Duration: {}", format_duration(ms)));
        }
        if let Some(ref branch) = self.event.git_branch {
            lines.push(format!("  Branch:  {}", branch));
        }

        // Context
        if let Ok(mut client) = self.client.lock()
            && let Ok(result) = client.call(
                Method::QUERY_CONTEXT,
                serde_json::json!({
                    "session_id": self.event.session_id,
                    "sequence": self.event.sequence,
                    "count": 3
                }),
            )
        {
            let before: Vec<StoredEvent> =
                serde_json::from_value(result["before"].clone()).unwrap_or_default();
            let after: Vec<StoredEvent> =
                serde_json::from_value(result["after"].clone()).unwrap_or_default();

            if !before.is_empty() || !after.is_empty() {
                lines.push(String::new());
                lines.push("\x1b[2m─── Context ───\x1b[0m".to_string());

                for evt in &before {
                    let code_str = evt
                        .exit_code
                        .map(|c| format!(" [{}]", c))
                        .unwrap_or_default();
                    lines.push(format!("\x1b[2m  {}{}\x1b[0m", evt.command, code_str));
                }

                lines.push(format!("\x1b[1;33m▸ {}\x1b[0m", self.event.command));

                for evt in &after {
                    let code_str = evt
                        .exit_code
                        .map(|c| format!(" [{}]", c))
                        .unwrap_or_default();
                    lines.push(format!("\x1b[2m  {}{}\x1b[0m", evt.command, code_str));
                }
            }
        }

        lines.push(String::new());
        lines.push("\x1b[2mEnter: place at prompt  |  Alt-Enter: execute now\x1b[0m".to_string());

        ItemPreview::AnsiText(lines.join("\n"))
    }
}

/// Parse a `:N` query into a 1-indexed line number.
fn parse_colon_number(query: &str) -> Option<usize> {
    let trimmed = query.trim();
    if trimmed.starts_with(':')
        && trimmed.len() > 1
        && trimmed[1..].chars().all(|c| c.is_ascii_digit())
    {
        trimmed[1..].parse().ok()
    } else {
        None
    }
}

pub fn run_picker(events: Vec<StoredEvent>, default_action: PickerAction) -> anyhow::Result<()> {
    use crate::ui::output;

    if events.is_empty() {
        output::info("No commands found");
        return Ok(());
    }

    let client = Arc::new(Mutex::new(RpcClient::connect()?));

    // Resolve session display IDs
    let session_ids = resolve_session_display_ids(&client, &events);

    // Keep a copy of commands for :N lookup after skim exits
    let commands: Vec<String> = events.iter().map(|e| e.command.clone()).collect();

    let (tx, rx): (SkimItemSender, SkimItemReceiver) = bounded(1);

    let client_clone = Arc::clone(&client);
    std::thread::spawn(move || {
        let items: Vec<Arc<dyn SkimItem>> = events
            .into_iter()
            .enumerate()
            .map(|(i, event)| {
                let sid = &event.session_id;
                let display_id = session_ids.get(sid).copied().unwrap_or(0);
                let line_number = i + 1;
                let display_text = format_display_text(&event, line_number);
                Arc::new(HistoryItem {
                    event,
                    display_text,
                    session_display_id: display_id,
                    client: Arc::clone(&client_clone),
                }) as Arc<dyn SkimItem>
            })
            .collect();
        let _ = tx.send(items);
    });

    let options = SkimOptionsBuilder::default()
        .height("100%".to_string())
        .reverse(true)
        .prompt("search> ".to_string())
        .preview_window(PreviewLayout::from("right:40%:wrap"))
        .no_sort(true)
        .exact(true)
        .no_hscroll(true)
        .bind(vec![
            // Accept on alt-enter; final_key.modifiers carries ALT so we can
            // distinguish it from a plain Enter and trigger execute-now mode.
            "alt-enter:accept".to_string(),
        ])
        .build()
        .map_err(|e| anyhow::anyhow!("Failed to build skim options: {}", e))?;

    let result =
        Skim::run_with(options, Some(rx)).map_err(|e| anyhow::anyhow!("Skim error: {}", e))?;

    if result.is_abort {
        return Ok(());
    }

    // Check for :N syntax in the query
    if let Some(n) = parse_colon_number(&result.query) {
        if let Some(cmd) = commands.get(n.wrapping_sub(1)) {
            execute_command(cmd);
        } else {
            output::error(&format!(
                "Line {} out of range (only {} results)",
                n,
                commands.len()
            ));
        }
        return Ok(());
    }

    // Detect Alt+Enter via the actual key modifiers (not the deprecated
    // result.cmd field, which is the interactive command query).
    let execute_now = result.final_key.code == KeyCode::Enter
        && result.final_key.modifiers.contains(KeyModifiers::ALT);

    let result_file = std::env::var("STENO_RESULT_FILE").ok();

    for item in &result.selected_items {
        let command = item.output();

        if execute_now {
            match default_action {
                PickerAction::Run => emit_command(&command, result_file.as_deref()),
                PickerAction::Print => emit_execute(&command, result_file.as_deref()),
            }
        } else {
            match default_action {
                PickerAction::Print => emit_command(&command, result_file.as_deref()),
                PickerAction::Run => emit_execute(&command, result_file.as_deref()),
            }
        }
    }

    Ok(())
}

fn resolve_session_display_ids(
    client: &Arc<Mutex<RpcClient>>,
    events: &[StoredEvent],
) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    let unique_sids: Vec<&str> = {
        let mut seen = std::collections::HashSet::new();
        events
            .iter()
            .filter_map(|e| {
                if seen.insert(e.session_id.as_str()) {
                    Some(e.session_id.as_str())
                } else {
                    None
                }
            })
            .collect()
    };

    if let Ok(mut client) = client.lock() {
        for sid in unique_sids {
            if let Ok(result) = client.call(Method::SESSION_SHOW, serde_json::json!({ "id": sid }))
                && let Some(display_id) = result["session"]["display_id"].as_i64()
            {
                map.insert(sid.to_string(), display_id);
            }
        }
    }

    map
}

fn terminal_cols() -> usize {
    #[cfg(unix)]
    unsafe {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdout().as_raw_fd();
        let mut ws: nix::libc::winsize = std::mem::zeroed();
        if nix::libc::ioctl(fd, nix::libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return ws.ws_col as usize;
        }
    }
    120
}

fn format_display_text(event: &StoredEvent, line_number: usize) -> String {
    let occ_badge = match event.occurrence_count {
        Some(n) if n > 1 => format!("  (x{})", n),
        _ => String::new(),
    };

    // Use the canonical absolute path for relative cd commands so the user
    // sees where they'd actually land, not an ambiguous "../something".
    let raw = if let Some(ref canonical) = event.cd_target_canonical {
        let expanded = format!("cd {}", canonical);
        if expanded != event.command {
            expanded
        } else {
            event.command.clone()
        }
    } else {
        event.command.clone()
    };

    // Collapse newlines so embedded \n doesn't break TUI line rendering.
    let cmd = raw.replace('\n', " ↵ ").replace('\r', "");

    // Skim has no per-item hscroll — truncate to terminal width so line
    // numbers never slide off screen. Full command is in the preview pane.
    let prefix = 6usize; // "  42  "
    let cols = terminal_cols();
    let max_cmd = cols.saturating_sub(prefix + occ_badge.len() + 1);

    let cmd_display = if cmd.len() > max_cmd {
        format!(
            "{}…",
            &cmd[..cmd.floor_char_boundary(max_cmd.saturating_sub(1))]
        )
    } else {
        cmd
    };

    format!("{:>4}  {}{}", line_number, cmd_display, occ_badge)
}

fn format_timestamp(ts: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| ts.to_string())
}

fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m{}s", mins, secs)
    }
}
