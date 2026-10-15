use std::path::PathBuf;

use serde_json::json;

use steno_core::ipc::methods::Method;

use crate::cli::ShellType;
use crate::client::RpcClient;
use crate::ui::output;

pub fn run(
    shell: Option<ShellType>,
    file: Option<PathBuf>,
    limit: usize,
    dry_run: bool,
) -> anyhow::Result<()> {
    let shell = shell.unwrap_or_else(detect_shell);

    let paths: Vec<(ShellKind, PathBuf)> = if let Some(path) = file {
        let kind = match &shell {
            ShellType::Fish => ShellKind::Fish,
            ShellType::Bash => ShellKind::Bash,
            _ => ShellKind::Zsh,
        };
        vec![(kind, path)]
    } else {
        collect_history_paths(&shell)
    };

    if paths.is_empty() {
        output::warn("No history files found");
        return Ok(());
    }

    let mut all_commands: Vec<String> = Vec::new();

    for (kind, path) in &paths {
        if !path.exists() {
            output::info(&format!("History file not found: {}", path.display()));
            continue;
        }

        output::info(&format!("Reading {}", path.display()));

        let bytes = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", path.display(), e))?;
        let raw = String::from_utf8_lossy(&bytes).into_owned();

        let commands = match kind {
            ShellKind::Zsh => parse_zsh(&raw),
            ShellKind::Bash => parse_bash(&raw),
            ShellKind::Fish => parse_fish(&raw),
        };

        all_commands.extend(commands);
    }

    // Filter noise, then deduplicate preserving last occurrence
    let filtered: Vec<String> = all_commands.into_iter().filter(|c| !is_noise(c)).collect();

    let deduped = dedup_keep_last(filtered);

    // Take at most `limit` (most recent = last in list)
    let to_import: Vec<String> = if deduped.len() > limit {
        deduped[deduped.len() - limit..].to_vec()
    } else {
        deduped
    };

    let total = to_import.len();

    if dry_run {
        output::info(&format!("{} commands would be imported", total));
        for cmd in &to_import {
            println!("  {}", cmd);
        }
        return Ok(());
    }

    if total == 0 {
        output::warn("No commands to import after filtering");
        return Ok(());
    }

    let shell_name = match &shell {
        ShellType::Bash => "bash",
        ShellType::Zsh => "zsh",
        ShellType::Fish => "fish",
        ShellType::All => "shell",
    };

    let commands_json: Vec<serde_json::Value> =
        to_import.iter().map(|c| json!({ "command": c })).collect();

    let mut client = RpcClient::connect()?;
    let result = client.call(
        Method::IMPORT_EVENTS,
        json!({ "shell": shell_name, "commands": commands_json }),
    )?;

    let imported = result["imported"].as_u64().unwrap_or(0);
    output::success(&format!(
        "Imported {} commands from shell history",
        imported
    ));

    Ok(())
}

#[derive(Debug)]
enum ShellKind {
    Bash,
    Zsh,
    Fish,
}

fn detect_shell() -> ShellType {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("zsh") {
        ShellType::Zsh
    } else if shell.contains("fish") {
        ShellType::Fish
    } else {
        ShellType::Bash
    }
}

fn collect_history_paths(shell: &ShellType) -> Vec<(ShellKind, PathBuf)> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return vec![],
    };

    match shell {
        ShellType::Zsh => {
            let path = std::env::var("HISTFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home.join(".zsh_history"));
            vec![(ShellKind::Zsh, path)]
        }
        ShellType::Bash => {
            let path = std::env::var("HISTFILE")
                .map(PathBuf::from)
                .unwrap_or_else(|_| home.join(".bash_history"));
            vec![(ShellKind::Bash, path)]
        }
        ShellType::Fish => {
            let path = home.join(".local/share/fish/fish_history");
            vec![(ShellKind::Fish, path)]
        }
        ShellType::All => {
            let mut paths = Vec::new();
            let zsh = home.join(".zsh_history");
            if zsh.exists() {
                paths.push((ShellKind::Zsh, zsh));
            }
            let bash = home.join(".bash_history");
            if bash.exists() {
                paths.push((ShellKind::Bash, bash));
            }
            let fish = home.join(".local/share/fish/fish_history");
            if fish.exists() {
                paths.push((ShellKind::Fish, fish));
            }
            paths
        }
    }
}

/// Parse zsh extended history format: `: timestamp:duration;command\n`
/// Also handles simple format (no timestamp prefix) and multi-line continuation.
fn parse_zsh(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut current: Option<String> = None;

    for line in content.lines() {
        if let Some(cmd_part) = line.strip_prefix(": ") {
            // Flush previous
            if let Some(cmd) = current.take() {
                commands.push(cmd);
            }
            // `: timestamp:duration;command`
            if let Some(semi) = cmd_part.find(';') {
                current = Some(cmd_part[semi + 1..].to_string());
            }
        } else if line.starts_with('\t') {
            // Multi-line continuation
            if let Some(ref mut cmd) = current {
                cmd.push('\n');
                cmd.push_str(line.trim_start_matches('\t'));
            }
        } else if !line.is_empty() {
            // Simple format (no timestamp)
            if let Some(cmd) = current.take() {
                commands.push(cmd);
            }
            current = Some(line.to_string());
        }
    }

    if let Some(cmd) = current {
        commands.push(cmd);
    }

    commands
}

/// Parse bash history — plain lines, with optional `# timestamp` lines.
fn parse_bash(content: &str) -> Vec<String> {
    let mut commands = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip timestamp lines generated by HISTTIMEFORMAT
        if line.starts_with('#') && line[1..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        commands.push(line.to_string());
    }

    commands
}

/// Parse fish history (YAML-like format with `- cmd:` entries).
fn parse_fish(content: &str) -> Vec<String> {
    let mut commands = Vec::new();

    for line in content.lines() {
        // Lines starting with `- cmd: ` or `  cmd: ` (indented)
        if let Some(rest) = line
            .strip_prefix("- cmd: ")
            .or_else(|| line.strip_prefix("  cmd: "))
        {
            let cmd = rest.trim().to_string();
            if !cmd.is_empty() {
                commands.push(cmd);
            }
        }
    }

    commands
}

/// Returns true if the command is meaningless noise not worth storing.
fn is_noise(cmd: &str) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() || cmd.len() < 2 {
        return true;
    }

    let mut parts = cmd.split_whitespace();
    let first = parts.next().unwrap_or("");
    let args: Vec<&str> = parts.collect();

    // Always-noise commands
    const ALWAYS_NOISE: &[&str] = &[
        "clear", "reset", "exit", "logout", "history", "fc", "bg", "fg", "jobs", "true", "false",
        "pwd",
    ];
    if ALWAYS_NOISE.contains(&first) {
        return true;
    }

    // Noise when used without meaningful arguments (flags only or nothing)
    const NOISE_WITHOUT_ARGS: &[&str] = &["ls", "ll", "la", "lr", "lt", "lsa", "dir", "echo"];
    if NOISE_WITHOUT_ARGS.contains(&first)
        && args.iter().all(|a| a.starts_with('-') || a.is_empty())
    {
        return true;
    }

    // cd with trivial or relative targets
    if first == "cd" {
        match args.as_slice() {
            [] | ["."] | [".."] | ["/"] | ["-"] => return true,
            // Relative paths (cd ../foo, cd subdir/bar) are useless without CWD context
            [target]
                if !target.starts_with('/')
                    && !target.starts_with('~')
                    && !target.starts_with('$') =>
            {
                return true;
            }
            _ => {}
        }
    }

    false
}

/// Deduplicate a list of commands, keeping the last occurrence of each unique command.
/// The returned list preserves original relative order of the last occurrences.
fn dedup_keep_last(commands: Vec<String>) -> Vec<String> {
    // Walk backwards and keep track of seen commands
    let mut seen = std::collections::HashSet::new();
    let mut result: Vec<String> = commands
        .into_iter()
        .rev()
        .filter(|c| seen.insert(c.clone()))
        .collect();
    result.reverse();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_noise() {
        assert!(is_noise(""));
        assert!(is_noise("ls"));
        assert!(is_noise("ls -la"));
        assert!(is_noise("clear"));
        assert!(is_noise("cd .."));
        assert!(is_noise("cd"));
        assert!(is_noise("cd ../foo"));
        assert!(is_noise("cd subdir/bar"));
        assert!(is_noise("cd ./local"));
        assert!(!is_noise("ls /etc"));
        assert!(!is_noise("cd ~/projects"));
        assert!(!is_noise("cd /absolute/path"));
        assert!(!is_noise("cd $HOME/work"));
        assert!(!is_noise("git status"));
        assert!(!is_noise("make build"));
    }

    #[test]
    fn test_parse_zsh() {
        let input = ": 1620000000:0;git status\n: 1620000001:0;ls -la\n";
        let result = parse_zsh(input);
        assert_eq!(result, vec!["git status", "ls -la"]);
    }

    #[test]
    fn test_dedup_keep_last() {
        let cmds = vec![
            "git status".to_string(),
            "ls".to_string(),
            "git status".to_string(),
            "make build".to_string(),
        ];
        let result = dedup_keep_last(cmds);
        assert_eq!(result, vec!["ls", "git status", "make build"]);
    }
}
