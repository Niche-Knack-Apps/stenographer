use std::io::IsTerminal;

use colored::Colorize;

pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

pub fn success(msg: &str) {
    if is_tty() {
        eprintln!("{} {}", "✓".green().bold(), msg);
    } else {
        eprintln!("OK: {}", msg);
    }
}

pub fn info(msg: &str) {
    if is_tty() {
        eprintln!("{} {}", "→".blue().bold(), msg);
    } else {
        eprintln!("{}", msg);
    }
}

pub fn warn(msg: &str) {
    if is_tty() {
        eprintln!("{} {}", "!".yellow().bold(), msg);
    } else {
        eprintln!("WARN: {}", msg);
    }
}

pub fn error(msg: &str) {
    if is_tty() {
        eprintln!("{} {}", "✗".red().bold(), msg);
    } else {
        eprintln!("ERROR: {}", msg);
    }
}
