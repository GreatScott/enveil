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
            "enject is already initialized in this directory. \
             To reinitialize, delete .enject/ first."
        );
    }

    println!("Initializing enject store...");

    // Generate a fresh 32-byte salt
    let mut salt = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    let salt_hex = hex::encode(&salt);

    let cfg = config::Config::default_new(salt_hex);

    // Prompt for Enject store password (twice, with confirmation)
    let password = prompt_new_password()?;

    // Write config first — this creates the .enject/ directory
    config::write(&root, &cfg).context("Failed to write config")?;

    let store_path = config::store_path(&root);
    PasswordStore::create_empty(&store_path, cfg.kdf_params(), salt, &password)
        .context("Failed to create encrypted store")?;

    println!("Initialized.");
    println!();
    println!("  1. Add a secret:       enject set some_api_key");
    println!("  2. Reference in .env:  API_KEY=en://some_api_key");
    println!("  3. Run your app:       enject run -- npm start");
    println!();
    println!("The en:// name must match the key you used in 'enject set'.");
    println!("The left side (DATABASE_URL) is what your app sees.");

    Ok(())
}

pub fn prompt_new_password() -> Result<SecretString> {
    let password = rpassword::prompt_password("New Enject store password: ")
        .context("Failed to read password")?;
    let confirm = rpassword::prompt_password("Confirm Enject store password: ")
        .context("Failed to read password confirmation")?;

    if password != confirm {
        bail!("Passwords do not match.");
    }
    if password.is_empty() {
        bail!("Enject store password must not be empty.");
    }

    Ok(SecretString::new(password))
}
