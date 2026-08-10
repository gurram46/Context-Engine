use std::path::{Path, PathBuf};

use context_core::ContextError;

/// Authoritative project root for R1.
/// Wraps a canonicalized absolute PathBuf and ensures it exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    /// Resolve from explicit path, env, or cwd.
    /// Prefers `CONTEXT_ENGINE_PROJECT_ROOT`, then `explicit`, then `cwd`.
    /// Canonicalizes and validates existence.
    pub fn resolve(explicit: Option<&Path>) -> Result<Self, ContextError> {
        let raw = if let Some(p) = explicit {
            p.to_path_buf()
        } else if let Ok(env) = std::env::var("CONTEXT_ENGINE_PROJECT_ROOT") {
            PathBuf::from(env)
        } else {
            std::env::current_dir()
                .map_err(|e| ContextError::InvalidRoot(format!("cannot get cwd: {}", e)))?
        };
        Self::new(raw)
    }

    /// Create from a raw path, canonicalize, validate.
    pub fn new(raw: PathBuf) -> Result<Self, ContextError> {
        if raw.as_os_str().is_empty() {
            return Err(ContextError::InvalidRoot("empty path".into()));
        }
        // Canonicalize if exists, otherwise return error.
        let canon = raw.canonicalize().map_err(|e| {
            ContextError::InvalidRoot(format!(
                "project root does not exist or not accessible: {} — {}",
                raw.display(),
                e
            ))
        })?;
        if !canon.is_dir() {
            return Err(ContextError::InvalidRoot(format!(
                "project root is not a directory: {}",
                canon.display()
            )));
        }
        Ok(Self(canon))
    }

    /// For tests: create without canonicalize (allows non-existent temp).
    #[cfg(test)]
    pub fn from_path_unchecked(p: PathBuf) -> Self {
        Self(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn as_path_buf(&self) -> PathBuf {
        self.0.clone()
    }
}

impl AsRef<Path> for ProjectRoot {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl std::fmt::Display for ProjectRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.display())
    }
}

/// Resolve project root for the current process (used by discovery/exact).
pub fn resolve_project_root(explicit: Option<&Path>) -> Result<ProjectRoot, ContextError> {
    ProjectRoot::resolve(explicit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolve_explicit() {
        let tmp = TempDir::new().unwrap();
        let pr = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        assert!(pr.path().exists());
    }

    #[test]
    fn resolve_nonexistent_fails() {
        let p = PathBuf::from("/tmp/does-not-exist-xyz-123456");
        let r = ProjectRoot::new(p);
        assert!(r.is_err());
    }

    #[test]
    fn display() {
        let tmp = TempDir::new().unwrap();
        let pr = ProjectRoot::resolve(Some(tmp.path())).unwrap();
        assert!(!pr.to_string().is_empty());
    }
}
