use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "enveil",
    about = "Keep secrets out of .env files — and out of AI context.",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Initialize a new enveil store in the current directory.
    Init,

    /// Add or update a secret (value is prompted interactively).
    Set {
        /// The secret key name.
        key: String,
    },

    /// List all stored secret key names (never values).
    List,

    /// Delete a secret from the store.
    Delete {
        /// The secret key name to delete.
        key: String,
    },

    /// Resolve .env template and run a subprocess with injected secrets.
    Run {
        /// Command and arguments to run (everything after --).
        #[arg(last = true, required = true)]
        cmd: Vec<String>,
    },

    /// Import a plaintext .env file: encrypt all values, rewrite as ev:// template.
    Import {
        /// Path to the plaintext .env file to import.
        file: PathBuf,
    },

    /// Re-encrypt the store with a new master password.
    Rotate,
}
