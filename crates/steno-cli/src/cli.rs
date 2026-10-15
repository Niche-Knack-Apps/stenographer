use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "steno",
    about = "Shell history — fuzzy-search commands across all sessions or the current directory",
    long_about = None,
    version = env!("CARGO_PKG_VERSION"),
    disable_version_flag = true,
)]
pub struct Cli {
    /// Print version
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Interactive fuzzy search across all history (default when no command given)
    History {
        /// Execute selected command instead of printing it
        #[arg(long)]
        run: bool,
        /// Print selected command to stdout (default)
        #[arg(long, conflicts_with = "run")]
        print: bool,
        /// Max commands to load (0 or -1 = unlimited). Defaults to history_limit in config.
        #[arg(long, short, allow_hyphen_values = true)]
        limit: Option<i64>,
    },

    /// Interactive fuzzy search scoped to the current directory
    Here {
        /// Optional command filter (e.g., "git")
        pattern: Option<String>,
        /// Execute selected command instead of printing it
        #[arg(long)]
        run: bool,
        /// Print selected command to stdout (default)
        #[arg(long, conflicts_with = "run")]
        print: bool,
        /// Max commands to load (0 or -1 = unlimited). Defaults to history_limit in config.
        #[arg(long, short, allow_hyphen_values = true)]
        limit: Option<i64>,
    },

    /// Import commands from your existing shell history file
    Prime {
        /// Shell to import from (default: auto-detect from $SHELL)
        #[arg(long, short = 's')]
        shell: Option<ShellType>,
        /// Path to history file (default: auto-detected)
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// Max commands to import
        #[arg(long, short, default_value = "10000")]
        limit: usize,
        /// Show what would be imported without actually importing
        #[arg(long)]
        dry_run: bool,
    },

    /// Manage shell hooks (install/uninstall/status)
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },

    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Install stenographer (binaries, daemon, shell hooks) — run this first
    Install {
        /// Skip starting the daemon after install
        #[arg(long)]
        no_start: bool,
    },

    /// Print version information
    Version,

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
pub enum HookAction {
    /// Add shell hooks to your RC file(s)
    Install {
        /// Shell to install for (default: all detected)
        #[arg(long)]
        shell: Option<ShellType>,
    },
    /// Remove shell hooks from RC file(s)
    Uninstall,
    /// Check hook installation status
    Status,
}

#[derive(Clone, clap::ValueEnum)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
    All,
}

#[derive(Subcommand)]
pub enum DaemonAction {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// Restart the daemon
    Restart,
    /// Tail daemon logs
    Logs {
        #[arg(long, short, default_value = "50")]
        lines: u32,
        #[arg(long, short)]
        follow: bool,
    },
}
