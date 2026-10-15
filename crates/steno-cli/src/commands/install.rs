use std::path::{Path, PathBuf};

use crate::ui::output;

const SERVICE_TEMPLATE: &str = include_str!("../../../../service/stenographer.service");

pub fn run(no_start: bool) -> anyhow::Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let bin_dir = home.join(".local/bin");
    std::fs::create_dir_all(&bin_dir)?;

    // ── 1. Find stenographerd ────────────────────────────────────────────────
    let daemon_src = find_daemon_binary()?;
    let daemon_dst = bin_dir.join("stenographerd");
    install_binary(&daemon_src, &daemon_dst)?;
    output::success(&format!(
        "Installed stenographerd → {}",
        daemon_dst.display()
    ));

    // ── 2. Install steno itself ──────────────────────────────────────────────
    let steno_src = std::env::current_exe()?;
    let steno_dst = bin_dir.join("steno");
    if steno_src != steno_dst {
        install_binary(&steno_src, &steno_dst)?;
        output::success(&format!("Installed steno → {}", steno_dst.display()));
    }

    // ── 3. Systemd service ───────────────────────────────────────────────────
    let svc_dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&svc_dir)?;
    let svc_path = svc_dir.join("stenographer.service");
    std::fs::write(&svc_path, SERVICE_TEMPLATE)?;
    output::success(&format!("Installed service → {}", svc_path.display()));

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "enable", "stenographer.service"])
        .status();
    output::success("Enabled stenographer.service");

    // ── 4. Shell hooks ───────────────────────────────────────────────────────
    super::hook::run(crate::cli::HookAction::Install { shell: None })?;

    // ── 5. Start daemon ──────────────────────────────────────────────────────
    if !no_start {
        let started = std::process::Command::new("systemctl")
            .args(["--user", "start", "stenographer.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !started {
            // Fallback: launch directly
            let log_dir = steno_core::config::log_dir(&steno_core::config::load_config());
            std::fs::create_dir_all(&log_dir)?;
            let log_file = log_dir.join("daemon.log");
            std::process::Command::new(&daemon_dst)
                .stdout(std::fs::File::create(&log_file)?)
                .stderr(std::fs::File::create(&log_file)?)
                .spawn()?;
        }
        output::success("Daemon started");
    }

    // ── 6. Done ──────────────────────────────────────────────────────────────
    let source_cmd = detect_source_cmd(&home);
    eprintln!();
    eprintln!("  Run this to activate in the current shell:");
    eprintln!();
    eprintln!("    {}", source_cmd);
    eprintln!();
    if !bin_dir_in_path(&bin_dir) {
        output::warn(&format!(
            "{} is not in your PATH — add it:",
            bin_dir.display()
        ));
        eprintln!("    export PATH=\"{}:$PATH\"", bin_dir.display());
        eprintln!();
    }

    Ok(())
}

/// Copy src → dst safely on Linux: write to a sibling temp file then rename.
///
/// A plain `fs::copy` over a running executable fails with ETXTBSY because the
/// kernel holds the inode mapped. `rename` is allowed — it just swaps the
/// directory entry, leaving the old inode alive until all processes close it.
fn install_binary(src: &Path, dst: &Path) -> anyhow::Result<()> {
    let dir = dst
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid destination path"))?;
    let tmp = dir.join(format!(
        ".steno-install-tmp-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos()
    ));
    std::fs::copy(src, &tmp)?;

    // Match the executable bit of the source
    #[cfg(unix)]
    {
        let perms = std::fs::metadata(src)?.permissions();
        std::fs::set_permissions(&tmp, perms)?;
    }

    std::fs::rename(&tmp, dst)?;
    Ok(())
}

/// Find the daemon binary: first look next to the current executable, then PATH.
fn find_daemon_binary() -> anyhow::Result<PathBuf> {
    // Look beside the running steno binary
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("stenographerd");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Fall back to PATH
    which::which("stenographerd").map_err(|_| {
        anyhow::anyhow!(
            "Cannot find stenographerd binary.\n\
             It should be in the same directory as steno, or on your PATH.\n\
             Download the full release archive (steno + stenographerd)."
        )
    })
}

fn detect_source_cmd(home: &Path) -> String {
    let shell = std::env::var("SHELL").unwrap_or_default();
    if shell.contains("fish") {
        "source ~/.config/fish/config.fish".to_string()
    } else if shell.contains("zsh") {
        let rc = std::env::var("ZDOTDIR")
            .map(|d| format!("{}/{}", d, ".zshrc"))
            .unwrap_or_else(|_| format!("{}", home.join(".zshrc").display()));
        format!("source {}", rc)
    } else {
        format!("source {}", home.join(".bashrc").display())
    }
}

fn bin_dir_in_path(bin_dir: &PathBuf) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|p| &p == bin_dir))
        .unwrap_or(false)
}
