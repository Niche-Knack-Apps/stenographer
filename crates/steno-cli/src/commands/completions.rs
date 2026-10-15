use clap::CommandFactory;
use clap_complete::generate;

use crate::cli::Cli;

pub fn run(shell: clap_complete::Shell) -> anyhow::Result<()> {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "steno", &mut std::io::stdout());
    Ok(())
}
