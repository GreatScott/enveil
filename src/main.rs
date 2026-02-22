mod cli;
mod commands;
mod config;
mod env_template;
mod error;
mod runner;
mod store;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => commands::init::run()?,
        Command::Set { key } => commands::set::run(&key)?,
        Command::List => commands::list::run()?,
        Command::Delete { key } => commands::delete::run(&key)?,
        Command::Run { cmd } => commands::run::run(cmd)?,
        Command::Import { file } => commands::import::run(&file)?,
        Command::Rotate => commands::rotate::run()?,
    }

    Ok(())
}
