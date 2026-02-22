use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::EnveilError;
use crate::store::password::KdfParams;

const CONFIG_DIR: &str = ".enveil";
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

    pub fn salt_bytes(&self) -> Result<Vec<u8>, EnveilError> {
        hex::decode(&self.salt)
            .map_err(|_| EnveilError::Config("Invalid salt hex in config.toml".into()))
    }
}

/// Returns the `.enveil` directory for a given project root.
pub fn enveil_dir(project_root: &Path) -> PathBuf {
    project_root.join(CONFIG_DIR)
}

/// Returns the config file path for a given project root.
pub fn config_path(project_root: &Path) -> PathBuf {
    enveil_dir(project_root).join(CONFIG_FILE)
}

/// Returns the store file path for a given project root.
pub fn store_path(project_root: &Path) -> PathBuf {
    enveil_dir(project_root).join(STORE_FILE)
}

/// Read and parse config from the given project root. Returns an error if not initialized.
pub fn read(project_root: &Path) -> Result<Config, EnveilError> {
    let path = config_path(project_root);
    if !path.exists() {
        return Err(EnveilError::StoreNotInitialized);
    }
    let raw = std::fs::read_to_string(&path)?;
    toml::from_str(&raw).map_err(|e| EnveilError::Config(e.to_string()))
}

/// Write config to the given project root. Creates the `.enveil` directory if needed.
pub fn write(project_root: &Path, config: &Config) -> Result<(), EnveilError> {
    let dir = enveil_dir(project_root);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(CONFIG_FILE);
    let raw = toml::to_string(config).map_err(|e| EnveilError::Config(e.to_string()))?;
    std::fs::write(path, raw)?;
    Ok(())
}

/// Returns the current project root (cwd).
pub fn project_root() -> Result<PathBuf, EnveilError> {
    std::env::current_dir().map_err(EnveilError::Io)
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
        assert!(matches!(err, EnveilError::StoreNotInitialized));
    }

    #[test]
    fn test_salt_bytes_roundtrip() {
        let original: Vec<u8> = (0u8..32).collect();
        let hex = hex::encode(&original);
        let config = Config::default_new(hex);
        assert_eq!(config.salt_bytes().unwrap(), original);
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
