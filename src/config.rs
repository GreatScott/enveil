use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::EnjectError;
use crate::store::password::KdfParams;

const CONFIG_DIR: &str = ".enject";
const LEGACY_CONFIG_DIR: &str = ".enveil";
const CONFIG_FILE: &str = "config.toml";
const STORE_FILE: &str = "store";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub backend: String,
    pub version: u32,
    pub kdf: String,
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    /// Hex-encoded 32-byte salt for Argon2id.
    pub salt: String,
}

impl Config {
    pub fn default_new(salt_hex: String) -> Self {
        let kdf = KdfParams::default();
        Self {
            backend: "password".into(),
            version: 1,
            kdf: "argon2id".into(),
            m_cost: kdf.m_cost,
            t_cost: kdf.t_cost,
            p_cost: kdf.p_cost,
            salt: salt_hex,
        }
    }

    pub fn kdf_params(&self) -> KdfParams {
        KdfParams {
            m_cost: self.m_cost,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }

    pub fn salt_bytes(&self) -> Result<Vec<u8>, EnjectError> {
        hex::decode(&self.salt)
            .map_err(|_| EnjectError::Config("Invalid salt hex in config.toml".into()))
    }
}

/// Returns the `.enject` directory for a given project root,
/// falling back to the legacy `.enveil/` directory if `.enject/` does not exist.
pub fn enject_dir(project_root: &Path) -> PathBuf {
    let new_dir = project_root.join(CONFIG_DIR);
    if new_dir.exists() {
        return new_dir;
    }
    let legacy_dir = project_root.join(LEGACY_CONFIG_DIR);
    if legacy_dir.exists() {
        return legacy_dir;
    }
    new_dir
}

/// Returns the config file path for a given project root.
pub fn config_path(project_root: &Path) -> PathBuf {
    enject_dir(project_root).join(CONFIG_FILE)
}

/// Returns the store file path for a given project root.
pub fn store_path(project_root: &Path) -> PathBuf {
    enject_dir(project_root).join(STORE_FILE)
}

/// Read and parse config from the given project root. Returns an error if not initialized.
pub fn read(project_root: &Path) -> Result<Config, EnjectError> {
    maybe_migrate_dir(project_root);
    let path = config_path(project_root);
    if !path.exists() {
        return Err(EnjectError::StoreNotInitialized);
    }
    let raw = std::fs::read_to_string(&path)?;
    toml::from_str(&raw).map_err(|e| EnjectError::Config(e.to_string()))
}

/// Write config to the given project root. Creates the `.enject` directory if needed.
pub fn write(project_root: &Path, config: &Config) -> Result<(), EnjectError> {
    let dir = enject_dir(project_root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(CONFIG_FILE);
    let raw = toml::to_string(config).map_err(|e| EnjectError::Config(e.to_string()))?;
    std::fs::write(path, raw)?;
    Ok(())
}

/// Returns the current project root (cwd).
pub fn project_root() -> Result<PathBuf, EnjectError> {
    std::env::current_dir().map_err(EnjectError::Io)
}

/// If `.enveil/` exists but `.enject/` does not, offer to migrate.
/// Copies `.enveil/` to `.enveil.bak/` as a backup, then renames to `.enject/`.
/// Errors are non-fatal — a failure falls through to using the legacy path.
fn maybe_migrate_dir(project_root: &Path) {
    let new_dir = project_root.join(CONFIG_DIR);
    let old_dir = project_root.join(LEGACY_CONFIG_DIR);

    if new_dir.exists() || !old_dir.exists() {
        return;
    }

    if !std::io::stdin().is_terminal() {
        println!(
            "Warning: found legacy .enveil/ store. Rename it to .enject/ to silence this warning."
        );
        return;
    }

    println!("Warning: found legacy .enveil/ store.");
    print!("Rename .enveil/ to .enject/? A backup will be kept at .enveil.bak/ [y/N]: ");
    if std::io::stdout().flush().is_err() {
        return;
    }

    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return;
    }

    if !answer.trim().eq_ignore_ascii_case("y") {
        println!("Skipping. Rename .enveil/ to .enject/ to silence this warning.");
        return;
    }

    let backup = project_root.join(".enveil.bak");
    if let Err(e) = copy_dir_all(&old_dir, &backup) {
        println!(
            "Warning: could not create backup: {}. Migration skipped.",
            e
        );
        return;
    }
    if let Err(e) = std::fs::rename(&old_dir, &new_dir) {
        println!(
            "Warning: could not rename .enveil/ to .enject/: {}. Using legacy path.",
            e
        );
        let _ = std::fs::remove_dir_all(&backup);
        return;
    }

    println!("Migrated .enveil/ to .enject/ (backup at .enveil.bak/).");
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fake_salt_hex() -> String {
        hex::encode((0u8..32).collect::<Vec<u8>>())
    }

    #[test]
    fn test_config_roundtrip() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let salt = fake_salt_hex();

        let config = Config::default_new(salt.clone());
        write(root, &config).unwrap();

        let loaded = read(root).unwrap();
        assert_eq!(loaded.backend, "password");
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.kdf, "argon2id");
        assert_eq!(loaded.salt, salt);
        assert_eq!(loaded.m_cost, KdfParams::default().m_cost);
        assert_eq!(loaded.t_cost, KdfParams::default().t_cost);
        assert_eq!(loaded.p_cost, KdfParams::default().p_cost);
    }

    #[test]
    fn test_read_missing_config_returns_not_initialized() {
        let dir = TempDir::new().unwrap();
        let err = read(dir.path()).unwrap_err();
        assert!(matches!(err, EnjectError::StoreNotInitialized));
    }

    #[test]
    fn test_salt_bytes_roundtrip() {
        let original: Vec<u8> = (0u8..32).collect();
        let hex = hex::encode(&original);
        let config = Config::default_new(hex);
        assert_eq!(config.salt_bytes().unwrap(), original);
    }

    #[test]
    fn test_legacy_enveil_dir_fallback() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create the legacy .enveil/ directory (but not .enject/)
        let legacy_dir = root.join(".enveil");
        std::fs::create_dir_all(&legacy_dir).unwrap();

        // enject_dir, config_path, store_path should all resolve to .enveil/
        assert_eq!(enject_dir(root), root.join(".enveil"));
        assert_eq!(config_path(root), root.join(".enveil").join("config.toml"));
        assert_eq!(store_path(root), root.join(".enveil").join("store"));
    }

    #[test]
    fn test_new_dir_takes_precedence_over_legacy() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();

        // Create both directories with valid configs
        let config = Config::default_new(fake_salt_hex());
        write(root, &config).unwrap(); // creates .enject/

        let legacy_dir = root.join(".enveil");
        std::fs::create_dir_all(&legacy_dir).unwrap();

        // .enject/ should win
        assert_eq!(enject_dir(root), root.join(".enject"));
    }

    #[test]
    fn test_kdf_params_roundtrip() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let config = Config::default_new(fake_salt_hex());
        write(root, &config).unwrap();
        let loaded = read(root).unwrap();
        let params = loaded.kdf_params();
        assert_eq!(params.m_cost, 65536);
        assert_eq!(params.t_cost, 3);
        assert_eq!(params.p_cost, 4);
    }
}
