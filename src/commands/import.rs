use anyhow::{bail, Context, Result};
use secrecy::SecretString;
use std::io::{self, BufRead, Write};
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

    // Count importable secrets so the warning is specific
    let lines = env_template::parse_file(file).context("Failed to parse import file")?;
    let secret_count = lines
        .iter()
        .filter(|l| matches!(l, EnvLine::Plain { .. }))
        .count();

    if secret_count == 0 {
        bail!("No plain KEY=value pairs found in {}. Nothing to import.", file.display());
    }

    // Warning
    println!();
    println!("WARNING: enveil import will:");
    println!("  1. Encrypt {} secret(s) from {} into your enveil store", secret_count, file.display());
    println!("  2. Overwrite {} in place, replacing secret values with ev:// references", file.display());
    println!();
    println!("This is destructive. If anything goes wrong (wrong password, etc.),");
    println!("your original secret values may be unrecoverable from the file.");
    println!();

    // Backup prompt
    let backup_path = file.with_extension("env.bak");
    print!("Create a backup at {} before importing? [y/N]: ", backup_path.display());
    io::stdout().flush()?;

    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    let wants_backup = answer.trim().eq_ignore_ascii_case("y");

    if wants_backup {
        std::fs::copy(file, &backup_path).context("Failed to create backup file")?;
        println!();
        println!("Backup written to {}", backup_path.display());
        println!();
        println!("IMPORTANT: {} still contains your plaintext secrets.", backup_path.display());
        println!("Move it somewhere safe or delete it before giving any AI tool");
        println!("access to this directory.");
        println!();
    }

    // Final confirmation before proceeding
    print!("Proceed with import? [y/N]: ");
    io::stdout().flush()?;

    let mut confirm = String::new();
    io::stdin().lock().read_line(&mut confirm)?;
    if !confirm.trim().eq_ignore_ascii_case("y") {
        println!("Import cancelled.");
        return Ok(());
    }

    println!();

    let password = rpassword::prompt_password("Enveil store password: ")
        .context("Failed to read Enveil store password")?;
    let password = SecretString::new(password);

    let store_path = config::store_path(&root);
    let mut store = PasswordStore::new(store_path, cfg.kdf_params(), cfg.salt_bytes()?);
    store
        .unlock(&password)
        .context("Failed to unlock store — wrong password?")?;

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

    println!("Imported {} secret(s). {} rewritten as ev:// template.", imported, file.display());
    if wants_backup {
        println!();
        println!("Remember: delete or move {} — it still contains plaintext secrets.", backup_path.display());
    }

    Ok(())
}
