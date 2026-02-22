use anyhow::{Context, Result};
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;

use crate::config;
use crate::env_template;
use crate::runner;
use crate::store::password::PasswordStore;
use crate::store::Store;

pub fn run(cmd: Vec<String>) -> Result<()> {
    let root = config::project_root()?;
    let cfg = config::read(&root)?;

    // Parse the .env template
    let env_path = root.join(".env");
    if !env_path.exists() {
        anyhow::bail!(
            ".env file not found in current directory. \
             Create one with ev:// references and try again."
        );
    }
    let lines = env_template::parse_file(&env_path).context("Failed to parse .env")?;

    // Unlock the local store
    let password = rpassword::prompt_password("Master password: ")
        .context("Failed to read master password")?;
    let password = SecretString::new(password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&password)
        .context("Failed to unlock store — wrong password?")?;

    // Build the local secrets map (expose only to resolve, not to disk/stdout)
    let local_secrets = build_secrets_map(&store)?;

    // TODO: global store support — for now, global refs will produce a clear error
    let global_secrets: HashMap<String, String> = HashMap::new();

    // Resolve all ev:// references — hard-errors on any unresolved ref
    let resolved = env_template::resolve(&lines, &local_secrets, &global_secrets)
        .context("Failed to resolve .env references")?;

    // Hand off to runner — secrets exist only in process memory from here
    runner::exec(&cmd, &resolved)?;

    Ok(())
}

fn build_secrets_map(store: &PasswordStore) -> Result<HashMap<String, String>> {
    let keys = store.list()?;
    let mut map = HashMap::new();
    for key in keys {
        if let Some(val) = store.get(&key)? {
            map.insert(key, val.expose_secret().to_string());
        }
    }
    Ok(map)
}
