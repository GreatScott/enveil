use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use zeroize::Zeroize;

use crate::error::EnveilError;
use crate::store::{Result, Store};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// AES-256-GCM + Argon2id password-based secret store.
pub struct PasswordStore {
    store_path: PathBuf,
    kdf_params: KdfParams,
    /// 32-byte salt for Argon2id key derivation. Generated once at init, never changes.
    salt: Vec<u8>,
    /// Decrypted secrets, populated after `unlock()`.
    secrets: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: 65536, // 64 MB
            t_cost: 3,
            p_cost: 4,
        }
    }
}

impl PasswordStore {
    pub fn new(store_path: PathBuf, kdf_params: KdfParams, salt: Vec<u8>) -> Self {
        Self {
            store_path,
            kdf_params,
            salt,
            secrets: None,
        }
    }

    /// Decrypt the store file and load secrets into memory.
    /// If the store file does not exist yet, initializes an empty in-memory map.
    pub fn unlock(&mut self, password: &SecretString) -> Result<()> {
        if !self.store_path.exists() {
            self.secrets = Some(HashMap::new());
            return Ok(());
        }

        let ciphertext_with_nonce = std::fs::read(&self.store_path)?;
        if ciphertext_with_nonce.len() < NONCE_LEN {
            return Err(EnveilError::CorruptStore(
                "Store file too short to contain a nonce.".into(),
            ));
        }

        let (nonce_bytes, ciphertext) = ciphertext_with_nonce.split_at(NONCE_LEN);

        let mut key = derive_key(
            password.expose_secret().as_bytes(),
            &self.salt,
            &self.kdf_params,
        )?;

        let plaintext_result = {
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|_| EnveilError::CorruptStore("Invalid key length.".into()))?;
            let nonce = Nonce::from_slice(nonce_bytes);
            cipher
                .decrypt(nonce, ciphertext)
                .map_err(|_| EnveilError::DecryptionFailed)
        };

        key.zeroize();

        let plaintext = plaintext_result?;

        let secrets: HashMap<String, String> = serde_json::from_slice(&plaintext)
            .map_err(|e| EnveilError::CorruptStore(e.to_string()))?;

        self.secrets = Some(secrets);
        Ok(())
    }

    /// Encrypt the in-memory secrets and write them atomically to disk.
    pub fn save(&self, password: &SecretString) -> Result<()> {
        let secrets = self.secrets_ref()?;

        let mut json_bytes =
            serde_json::to_vec(secrets).map_err(|e| EnveilError::Serialization(e.to_string()))?;

        let mut key = derive_key(
            password.expose_secret().as_bytes(),
            &self.salt,
            &self.kdf_params,
        )?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext_result = {
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|_| EnveilError::CorruptStore("Invalid key length.".into()))?;
            cipher
                .encrypt(nonce, json_bytes.as_ref())
                .map_err(|_| EnveilError::CorruptStore("Encryption failed.".into()))
        };

        key.zeroize();
        json_bytes.zeroize();

        let ciphertext = ciphertext_result?;

        // Atomic write: write to temp file → fsync → rename
        let parent = self
            .store_path
            .parent()
            .ok_or_else(|| EnveilError::Config("Store has no parent directory.".into()))?;

        let tmp_path = parent.join(format!(".store.tmp.{}", rand::random::<u64>()));

        {
            let mut tmp = std::fs::File::create(&tmp_path)?;
            tmp.write_all(&nonce_bytes)?;
            tmp.write_all(&ciphertext)?;
            tmp.sync_all()?;
        }

        std::fs::rename(&tmp_path, &self.store_path)?;
        Ok(())
    }

    /// Create a new empty store file, encrypted with the given password.
    pub fn create_empty(
        store_path: &Path,
        kdf_params: KdfParams,
        salt: Vec<u8>,
        password: &SecretString,
    ) -> Result<Self> {
        let mut store = Self::new(store_path.to_path_buf(), kdf_params, salt);
        store.secrets = Some(HashMap::new());
        store.save(password)?;
        Ok(store)
    }

    fn secrets_mut(&mut self) -> Result<&mut HashMap<String, String>> {
        self.secrets
            .as_mut()
            .ok_or_else(|| EnveilError::CorruptStore("Store not unlocked.".into()))
    }

    fn secrets_ref(&self) -> Result<&HashMap<String, String>> {
        self.secrets
            .as_ref()
            .ok_or_else(|| EnveilError::CorruptStore("Store not unlocked.".into()))
    }
}

impl Store for PasswordStore {
    fn get(&self, key: &str) -> Result<Option<SecretString>> {
        let secrets = self.secrets_ref()?;
        Ok(secrets.get(key).map(|v| SecretString::new(v.clone())))
    }

    fn set(&mut self, key: &str, value: SecretString) -> Result<()> {
        let secrets = self.secrets_mut()?;
        secrets.insert(key.to_string(), value.expose_secret().to_string());
        Ok(())
    }

    fn delete(&mut self, key: &str) -> Result<bool> {
        let secrets = self.secrets_mut()?;
        Ok(secrets.remove(key).is_some())
    }

    fn list(&self) -> Result<Vec<String>> {
        let secrets = self.secrets_ref()?;
        let mut keys: Vec<String> = secrets.keys().cloned().collect();
        keys.sort();
        Ok(keys)
    }
}

/// Derive a 32-byte AES key from the given password and salt using Argon2id.
/// The caller is responsible for zeroizing the returned array after use.
fn derive_key(password: &[u8], salt: &[u8], params: &KdfParams) -> Result<[u8; KEY_LEN]> {
    let argon2_params = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|e| EnveilError::Config(e.to_string()))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| EnveilError::Config(e.to_string()))?;

    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;
    use tempfile::TempDir;

    fn test_params() -> KdfParams {
        // Very low cost for fast tests
        KdfParams {
            m_cost: 8192,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn test_salt() -> Vec<u8> {
        (0u8..32).collect()
    }

    fn test_password() -> SecretString {
        SecretString::new("test-password-do-not-use".to_string())
    }

    fn setup_unlocked_store(dir: &TempDir) -> PasswordStore {
        let store_path = dir.path().join("store");
        let mut store = PasswordStore::new(store_path, test_params(), test_salt());
        store.unlock(&test_password()).unwrap();
        store
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store_path = dir.path().join("store");
        let password = test_password();

        let mut store = PasswordStore::new(store_path.clone(), test_params(), test_salt());
        store.unlock(&password).unwrap();
        store
            .set(
                "my_key",
                SecretString::new("super-secret-value".to_string()),
            )
            .unwrap();
        store.save(&password).unwrap();

        // Reload from disk
        let mut store2 = PasswordStore::new(store_path, test_params(), test_salt());
        store2.unlock(&password).unwrap();
        let retrieved = store2.get("my_key").unwrap().expect("key should exist");
        assert_eq!(retrieved.expose_secret(), "super-secret-value");
    }

    #[test]
    fn test_wrong_password_returns_err() {
        let dir = TempDir::new().unwrap();
        let store_path = dir.path().join("store");
        let password = test_password();
        let wrong = SecretString::new("wrong-password".to_string());

        let mut store = PasswordStore::new(store_path.clone(), test_params(), test_salt());
        store.unlock(&password).unwrap();
        store
            .set("key", SecretString::new("val".to_string()))
            .unwrap();
        store.save(&password).unwrap();

        let mut store2 = PasswordStore::new(store_path, test_params(), test_salt());
        let result = store2.unlock(&wrong);
        assert!(result.is_err(), "Wrong password should return Err");
    }

    #[test]
    fn test_tampered_ciphertext_returns_err() {
        let dir = TempDir::new().unwrap();
        let store_path = dir.path().join("store");
        let password = test_password();

        let mut store = PasswordStore::new(store_path.clone(), test_params(), test_salt());
        store.unlock(&password).unwrap();
        store
            .set("key", SecretString::new("val".to_string()))
            .unwrap();
        store.save(&password).unwrap();

        // Flip a byte in the ciphertext region (past the nonce)
        let mut bytes = std::fs::read(&store_path).unwrap();
        bytes[NONCE_LEN + 5] ^= 0xFF;
        std::fs::write(&store_path, bytes).unwrap();

        let mut store2 = PasswordStore::new(store_path, test_params(), test_salt());
        let result = store2.unlock(&password);
        assert!(result.is_err(), "Tampered ciphertext should return Err");
    }

    #[test]
    fn test_list_returns_sorted_keys() {
        let dir = TempDir::new().unwrap();
        let mut store = setup_unlocked_store(&dir);

        store
            .set("zebra", SecretString::new("z".to_string()))
            .unwrap();
        store
            .set("apple", SecretString::new("a".to_string()))
            .unwrap();
        store
            .set("mango", SecretString::new("m".to_string()))
            .unwrap();

        let keys = store.list().unwrap();
        assert_eq!(keys, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn test_delete_existing_key() {
        let dir = TempDir::new().unwrap();
        let mut store = setup_unlocked_store(&dir);

        store
            .set("to_delete", SecretString::new("v".to_string()))
            .unwrap();
        assert!(store.delete("to_delete").unwrap());
        assert!(store.get("to_delete").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_key() {
        let dir = TempDir::new().unwrap();
        let mut store = setup_unlocked_store(&dir);
        assert!(!store.delete("nonexistent").unwrap());
    }

    #[test]
    fn test_get_missing_key_returns_none() {
        let dir = TempDir::new().unwrap();
        let store = setup_unlocked_store(&dir);
        assert!(store.get("missing").unwrap().is_none());
    }

    #[test]
    fn test_nonce_changes_on_each_save() {
        let dir = TempDir::new().unwrap();
        let store_path = dir.path().join("store");
        let password = test_password();

        let mut store = PasswordStore::new(store_path.clone(), test_params(), test_salt());
        store.unlock(&password).unwrap();
        store.set("k", SecretString::new("v".to_string())).unwrap();
        store.save(&password).unwrap();

        let nonce1 = std::fs::read(&store_path).unwrap()[..NONCE_LEN].to_vec();
        store.save(&password).unwrap();
        let nonce2 = std::fs::read(&store_path).unwrap()[..NONCE_LEN].to_vec();

        // Nonces should almost certainly differ (probability of collision is negligible)
        assert_ne!(
            nonce1, nonce2,
            "Nonce should be freshly generated on every write"
        );
    }
}
