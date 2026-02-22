use anyhow::{bail, Context, Result};
use rand::RngCore;
use secrecy::SecretString;

use crate::config;
use crate::store::password::PasswordStore;

pub fn run() -> Result<()> {
    let root = config::project_root()?;
    let cfg_path = config::config_path(&root);

    if cfg_path.exists() {
        bail!(
            "enveil is already initialized in this directory. \
             To reinitialize, delete .enveil/ first."
        );
    }

    println!("Initializing enveil store...");

    // Generate a fresh 32-byte salt
    let mut salt = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    let salt_hex = hex::encode(&salt);

    let cfg = config::Config::default_new(salt_hex);

    // Prompt for master password (twice, with confirmation)
    let password = prompt_new_password()?;

    let store_path = config::store_path(&root);
    PasswordStore::create_empty(&store_path, cfg.kdf_params(), salt, &password)
        .context("Failed to create encrypted store")?;

    config::write(&root, &cfg).context("Failed to write config")?;

    println!("Initialized. Add secrets with: enveil set <key>");
    println!("Reference them in .env as: KEY=ev://<key>");
    println!("Run your app with: enveil run -- <command>");

    Ok(())
}

pub fn prompt_new_password() -> Result<SecretString> {
    let password = rpassword::prompt_password("Enter new master password: ")
        .context("Failed to read password")?;
    let confirm = rpassword::prompt_password("Confirm master password: ")
        .context("Failed to read password confirmation")?;

    if password != confirm {
        bail!("Passwords do not match.");
    }
    if password.is_empty() {
        bail!("Master password must not be empty.");
    }

    Ok(SecretString::new(password))
}
