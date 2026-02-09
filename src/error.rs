use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("vault root does not exist: {0}")]
    VaultNotFound(PathBuf),

    #[error("invalid vault path: {0}")]
    InvalidVaultPath(String),

    #[error("path is outside vault: {0}")]
    PathOutsideVault(PathBuf),

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("frontmatter yaml parse error: {0}")]
    FrontmatterYaml(#[from] serde_yaml::Error),

    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),

    #[error("schema toml parse error: {0}")]
    SchemaToml(String),

    #[error("embedding error: {0}")]
    Embedding(String),
}

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
