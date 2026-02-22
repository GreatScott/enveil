use anyhow::{Context, Result};
use secrecy::SecretString;

use crate::commands::init::prompt_new_password;
use crate::config;
use crate::store::password::PasswordStore;

pub fn run() -> Result<()> {
    let root = config::project_root()?;
    let cfg = config::read(&root)?;

    let old_password = rpassword::prompt_password("Current master password: ")
        .context("Failed to read current master password")?;
    let old_password = SecretString::new(old_password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&old_password)
        .context("Failed to unlock store — wrong password?")?;

    println!("Enter a new master password.");
    let new_password = prompt_new_password()?;

    store
        .save(&new_password)
        .context("Failed to re-encrypt store with new password")?;

    println!("Master password rotated successfully.");
    Ok(())
}
