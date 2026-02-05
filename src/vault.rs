use std::path::{Component, Path, PathBuf};

use crate::{Error, Result, VaultConfig};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultPath(PathBuf);

impl VaultPath {
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn as_str_lossy(&self) -> String {
        self.0.to_string_lossy().to_string()
    }
}

impl TryFrom<&Path> for VaultPath {
    type Error = Error;

    fn try_from(value: &Path) -> Result<Self> {
        if value.as_os_str().is_empty() {
            return Err(Error::InvalidVaultPath("empty path".into()));
        }
        if value.is_absolute() {
            return Err(Error::InvalidVaultPath(
                "absolute paths are not allowed".into(),
            ));
        }

        let mut cleaned = PathBuf::new();
        for c in value.components() {
            match c {
                Component::Prefix(_) | Component::RootDir => {
                    return Err(Error::InvalidVaultPath(
                        "absolute paths are not allowed".into(),
                    ));
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(Error::InvalidVaultPath(
                        "path traversal is not allowed".into(),
                    ));
                }
                Component::Normal(part) => cleaned.push(part),
            }
        }

        if cleaned.as_os_str().is_empty() {
            return Err(Error::InvalidVaultPath("empty path".into()));
        }

        Ok(Self(cleaned))
    }
}

#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
    cfg: VaultConfig,
}

impl Vault {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        Self::with_config(root, VaultConfig::default())
    }

    pub fn with_config(root: impl Into<PathBuf>, cfg: VaultConfig) -> Result<Self> {
        let root = root.into();
        if !root.exists() {
            return Err(Error::VaultNotFound(root));
        }
        let root = std::fs::canonicalize(&root).map_err(|e| Error::io(&root, e))?;
        Ok(Self { root, cfg })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config(&self) -> &VaultConfig {
        &self.cfg
    }

    pub fn to_abs(&self, rel: &VaultPath) -> PathBuf {
        self.root.join(rel.as_path())
    }

    pub fn to_rel(&self, abs: &Path) -> Result<VaultPath> {
        let abs = if abs.is_absolute() {
            abs.to_path_buf()
        } else {
            self.root.join(abs)
        };

        let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
        if !abs.starts_with(&self.root) {
            return Err(Error::PathOutsideVault(abs));
        }
        let rel = abs
            .strip_prefix(&self.root)
            .map_err(|_| Error::PathOutsideVault(abs.clone()))?;
        VaultPath::try_from(rel)
    }

    pub fn is_ignored_rel(&self, rel: &Path) -> bool {
        rel.components().any(|c| {
            let Component::Normal(part) = c else {
                return false;
            };
            let s = part.to_string_lossy();
            self.cfg.ignore_dirs.iter().any(|d| d == &s)
        })
    }

    pub fn is_indexable_rel(&self, rel: &Path) -> bool {
        if self.is_ignored_rel(rel) {
            return false;
        }

        // Skip directories and empty paths.
        if rel.as_os_str().is_empty() {
            return false;
        }
        let file_name = rel.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if file_name.starts_with('.') {
            // Obsidian notes can be dotfiles, but default to ignoring.
            return false;
        }

        true
    }

    pub fn is_indexable_path(&self, abs_or_rel: &Path) -> bool {
        match self.to_rel(abs_or_rel) {
            Ok(rel) => self.is_indexable_rel(rel.as_path()),
            Err(_) => false,
        }
    }
}
