use anyhow::{Context, Result};
use secrecy::SecretString;

use crate::config;
use crate::store::password::PasswordStore;
use crate::store::Store;

pub fn run() -> Result<()> {
    let root = config::project_root()?;
    let cfg = config::read(&root)?;

    let password = rpassword::prompt_password("Master password: ")
        .context("Failed to read master password")?;
    let password = SecretString::new(password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&password)
        .context("Failed to unlock store — wrong password?")?;

    let keys = store.list()?;
    if keys.is_empty() {
        println!("No secrets stored. Add one with: enveil set <key>");
    } else {
        for key in &keys {
            println!("{}", key);
        }
    }

    Ok(())
}
