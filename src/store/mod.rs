pub mod password;

use crate::error::EnveilError;
use secrecy::SecretString;

pub type Result<T> = std::result::Result<T, EnveilError>;

/// Core abstraction for secret storage. Commands interact only with this trait.
pub trait Store {
    fn get(&self, key: &str) -> Result<Option<SecretString>>;
    fn set(&mut self, key: &str, value: SecretString) -> Result<()>;
    fn delete(&mut self, key: &str) -> Result<bool>;
    fn list(&self) -> Result<Vec<String>>;
}
