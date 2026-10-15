mod cli;
mod client;
mod commands;
mod ui;

use std::process;

use clap::Parser;

use cli::Cli;
use ui::output;

fn main() {
    if let Err(e) = run(Cli::parse()) {
        output::error(&e.to_string());
        process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        None => {
            let limit = steno_core::config::load_config().history_limit;
            commands::history::run(false, false, limit, None)
        }

        Some(cli::Command::History { run, print, limit }) => {
            let limit = limit.unwrap_or_else(|| steno_core::config::load_config().history_limit);
            commands::history::run(run, print, limit, None)
        }

        Some(cli::Command::Here {
            pattern,
            run,
            print,
            limit,
        }) => {
            let limit = limit.unwrap_or_else(|| steno_core::config::load_config().history_limit);
            commands::here::run(pattern, run, print, limit)
        }

        Some(cli::Command::Prime {
            shell,
            file,
            limit,
            dry_run,
        }) => commands::prime::run(shell, file, limit, dry_run),

        Some(cli::Command::Hook { action }) => commands::hook::run(action),

        Some(cli::Command::Daemon { action }) => commands::daemon::run(action),

        Some(cli::Command::Install { no_start }) => commands::install::run(no_start),

        Some(cli::Command::Version) => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }

        Some(cli::Command::Completions { shell }) => commands::completions::run(shell),
    }
}
