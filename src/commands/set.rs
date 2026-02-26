use anyhow::{Context, Result};
use secrecy::SecretString;

use crate::config;
use crate::store::password::PasswordStore;
use crate::store::Store;

pub fn run(key: &str) -> Result<()> {
    let root = config::project_root()?;
    let cfg = config::read(&root)?;

    let password = rpassword::prompt_password("Enject store password: ")
        .context("Failed to read Enject store password")?;
    let password = SecretString::new(password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&password)
        .context("Failed to unlock store — wrong password?")?;

    let secret = rpassword::prompt_password(format!("Value for '{}': ", key))
        .context("Failed to read secret value")?;
    if secret.is_empty() {
        anyhow::bail!("Secret value must not be empty.");
    }
    let secret = SecretString::new(secret);

    store.set(key, secret)?;
    store.save(&password).context("Failed to save store")?;

    println!("Secret '{}' saved.", key);
    Ok(())
}
