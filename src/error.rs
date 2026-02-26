use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnjectError {
    #[error("Store not initialized. Run `enject init` first.")]
    StoreNotInitialized,

    #[error("Wrong Enject store password, or store is corrupted.")]
    DecryptionFailed,

    #[error("Store is corrupted: {0}")]
    CorruptStore(String),

    #[error("Secret '{0}' not found in store. Add it with: enject set {0}")]
    SecretNotFound(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
}
