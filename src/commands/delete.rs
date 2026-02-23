use anyhow::{Context, Result};
use secrecy::SecretString;

use crate::config;
use crate::store::password::PasswordStore;
use crate::store::Store;

pub fn run(key: &str) -> Result<()> {
    let root = config::project_root()?;
    let cfg = config::read(&root)?;

    let password = rpassword::prompt_password("Enveil store password: ")
        .context("Failed to read Enveil store password")?;
    let password = SecretString::new(password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&password)
        .context("Failed to unlock store — wrong password?")?;

    if store.delete(key)? {
        store.save(&password).context("Failed to save store")?;
        println!("Secret '{}' deleted.", key);
    } else {
        println!("Secret '{}' not found.", key);
    }

    Ok(())
}
