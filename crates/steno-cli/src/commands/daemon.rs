use crate::cli::DaemonAction;
use crate::ui::output;

pub fn run(action: DaemonAction) -> anyhow::Result<()> {
    match action {
        DaemonAction::Start => start(),
        DaemonAction::Stop => stop(),
        DaemonAction::Restart => {
            stop()?;
            std::thread::sleep(std::time::Duration::from_secs(1));
            start()
        }
        DaemonAction::Logs { lines, follow } => logs(lines, follow),
    }
}

fn start() -> anyhow::Result<()> {
    // Try systemd first
    let systemd = std::process::Command::new("systemctl")
        .args(["--user", "start", "stenographer.service"])
        .status();

    match systemd {
        Ok(s) if s.success() => {
            output::success("Daemon started via systemd");
            Ok(())
        }
        _ => {
            // Fallback: direct launch
            let log_dir = steno_core::config::log_dir(&steno_core::config::load_config());
            std::fs::create_dir_all(&log_dir)?;
            let log_file = log_dir.join("daemon.log");

            let child = std::process::Command::new("stenographerd")
                .stdout(std::fs::File::create(&log_file)?)
                .stderr(std::fs::File::create(&log_file)?)
                .spawn();

            match child {
                Ok(_) => {
                    output::success("Daemon started in background");
                    Ok(())
                }
                Err(e) => {
                    anyhow::bail!("Failed to start daemon: {}", e);
                }
            }
        }
    }
}

fn stop() -> anyhow::Result<()> {
    // Try systemd first
    let systemd = std::process::Command::new("systemctl")
        .args(["--user", "stop", "stenographer.service"])
        .status();

    match systemd {
        Ok(s) if s.success() => {
            output::success("Daemon stopped via systemd");
            return Ok(());
        }
        _ => {}
    }

    // Fallback: PID file
    let pid_path = steno_core::config::pid_path();
    if let Ok(pid_str) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        let result = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
        match result {
            Ok(()) => {
                output::success("Daemon stopped");
                return Ok(());
            }
            Err(e) => {
                output::warn(&format!("Failed to stop daemon (pid {}): {}", pid, e));
            }
        }
    }

    output::warn("Daemon does not appear to be running");
    Ok(())
}

fn logs(lines: u32, follow: bool) -> anyhow::Result<()> {
    // Try journalctl first
    let mut args = vec!["--user", "-u", "stenographer.service", "--no-pager"];
    let lines_str = format!("-n{}", lines);
    args.push(&lines_str);
    if follow {
        args.push("-f");
    }

    let result = std::process::Command::new("journalctl")
        .args(&args)
        .status();

    match result {
        Ok(s) if s.success() => Ok(()),
        _ => {
            // Fallback: read log file
            let log_dir = steno_core::config::log_dir(&steno_core::config::load_config());
            let log_pattern = log_dir.join("stenographerd.log");

            if follow {
                let status = std::process::Command::new("tail")
                    .args(["-f", "-n", &lines.to_string()])
                    .arg(&log_pattern)
                    .status()?;
                if !status.success() {
                    output::warn("No log files found");
                }
            } else {
                let status = std::process::Command::new("tail")
                    .args(["-n", &lines.to_string()])
                    .arg(&log_pattern)
                    .status()?;
                if !status.success() {
                    output::warn("No log files found");
                }
            }
            Ok(())
        }
    }
}
