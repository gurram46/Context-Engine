use std::path::{Path, PathBuf};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub embedding_model: String,
    pub semantic_enabled: bool,
    pub context_budget: usize,
    pub watcher_enabled: bool,
    pub index_location: Option<PathBuf>,
    #[serde(default = "default_memory_budget_mb")]
    pub memory_budget_mb: usize,
}

fn default_memory_budget_mb() -> usize {
    512
}

impl Default for Config {
    fn default() -> Self {
        Self {
            embedding_model: "all-minilm".into(),
            semantic_enabled: true,
            context_budget: 10000,
            watcher_enabled: true,
            index_location: None,
            memory_budget_mb: 512,
        }
    }
}

pub fn memory_budget_bytes() -> usize {
    let mb = std::env::var("CONTEXTD_MEMORY_BUDGET_MB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(512);
    mb * 1024 * 1024
}

impl Config {
    /// Load config with priority: CLI flag > env > project config > defaults.
    /// For now CLI flags passed as Option overrides; env vars CONTEXTD_EMBED_MODEL, etc.
    #[allow(dead_code)]
    pub fn load(root: &Path, cli_model: Option<String>, cli_budget: Option<usize>) -> Self {
        let mut cfg = Self::default();

        // project config .context/contextd.toml
        let cfg_path = root.join(".context").join("contextd.toml");
        if cfg_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&cfg_path) {
                if let Ok(parsed) = toml::from_str::<Config>(&content) {
                    cfg = parsed;
                }
            }
        }

        // env overrides
        if let Ok(m) = std::env::var("CONTEXTD_EMBED_MODEL") {
            cfg.embedding_model = m;
        }
        if let Ok(b) = std::env::var("CONTEXTD_BUDGET") {
            if let Ok(n) = b.parse::<usize>() {
                cfg.context_budget = n;
            }
        }
        if let Ok(v) = std::env::var("CONTEXTD_SEMANTIC_ENABLED") {
            cfg.semantic_enabled = v != "0" && v.to_lowercase() != "false";
        }
        if let Ok(mb) = std::env::var("CONTEXTD_MEMORY_BUDGET_MB") {
            if let Ok(n) = mb.parse::<usize>() {
                cfg.memory_budget_mb = n;
            }
        }

        // CLI overrides
        if let Some(m) = cli_model {
            cfg.embedding_model = m;
        }
        if let Some(b) = cli_budget {
            cfg.context_budget = b;
        }

        cfg
    }
}
