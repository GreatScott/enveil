use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnveilError {
    #[error("Store not initialized. Run `enveil init` first.")]
    StoreNotInitialized,

    #[error("Wrong Enveil store password, or store is corrupted.")]
    DecryptionFailed,

    #[error("Store is corrupted: {0}")]
    CorruptStore(String),

    #[error("Secret '{0}' not found in store. Add it with: enveil set {0}")]
    SecretNotFound(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
