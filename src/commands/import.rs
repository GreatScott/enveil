use anyhow::{bail, Context, Result};
use secrecy::SecretString;
use std::io::Write;
use std::path::Path;

use crate::config;
use crate::env_template::{self, templatize, EnvLine};
use crate::store::password::PasswordStore;
use crate::store::Store;

pub fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        bail!("File not found: {}", file.display());
    }

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

    let lines = env_template::parse_file(file).context("Failed to parse import file")?;

    let mut imported = 0usize;

    for line in &lines {
        if let EnvLine::Plain { key, value } = line {
            let secret_name = key.to_lowercase();
            store.set(&secret_name, SecretString::new(value.clone()))?;
            imported += 1;
        }
    }

    store.save(&password).context("Failed to save store")?;

    // Rewrite the source file as an ev:// template
    let new_lines = templatize(&lines);

    let output = new_lines.join("\n");
    let tmp_path = file.with_extension("env.tmp");
    {
        let mut tmp = std::fs::File::create(&tmp_path)?;
        write!(tmp, "{}", output)?;
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, file)?;

    println!(
        "Imported {} secret(s). {} rewritten as ev:// template.",
        imported,
        file.display()
    );

    Ok(())
}
