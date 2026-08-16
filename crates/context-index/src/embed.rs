//! R4C — Embedding provider contract.
//! Minimal typed abstraction, deterministic identity, dimension, model/version fingerprint.

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use tokio::sync::Mutex;

/// Fingerprint for model/version tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelFingerprint {
    pub model_id: String,
    pub version: String,
    pub dimension: usize,
}

impl ModelFingerprint {
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.model_id, self.version, self.dimension)
    }
}

#[async_trait]
pub trait Embedder: Send + Sync {
    fn model_id(&self) -> &str;
    fn dimension(&self) -> usize;
    fn version(&self) -> &str {
        "1"
    }
    fn fingerprint(&self) -> ModelFingerprint {
        ModelFingerprint {
            model_id: self.model_id().to_string(),
            version: self.version().to_string(),
            dimension: self.dimension(),
        }
    }
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>>;
    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::new();
        for t in texts {
            out.push(self.embed_query(t).await?);
        }
        Ok(out)
    }
}

/// Deterministic fake embedder for tests and fallback.
/// Produces fixed vectors via blake3 hash, normalized.
/// Not for quality benchmarking, but for incremental reuse and unit tests.
// ponytail: simple test helper, kept unconditional for cross-crate tests
pub struct FakeEmbedder {
    model: String,
    dim: usize,
    version: String,
}

impl FakeEmbedder {
    pub fn new(model: &str, dim: usize) -> Self {
        Self {
            model: model.to_string(),
            dim,
            version: "fake-v1".to_string(),
        }
    }
    fn hash_to_vec(&self, text: &str) -> Vec<f32> {
        let hash = blake3::hash(text.as_bytes());
        let bytes = hash.as_bytes();
        let mut v = Vec::with_capacity(self.dim);
        for i in 0..self.dim {
            let b = bytes[i % 32];
            // map 0-255 -> -1.0..1.0
            let f = (b as f32 / 127.5) - 1.0;
            v.push(f);
        }
        // normalize
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
        for x in &mut v {
            *x /= norm;
        }
        v
    }
}

#[async_trait]
impl Embedder for FakeEmbedder {
    fn model_id(&self) -> &str {
        &self.model
    }
    fn dimension(&self) -> usize {
        self.dim
    }
    fn version(&self) -> &str {
        &self.version
    }
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        Ok(self.hash_to_vec(query))
    }
}

/// SlowTestEmbedder — deterministic slow embedder for async freshness test.
/// Wraps FakeEmbedder and sleeps 2s per embedding to prove structural/exact not blocked.
#[cfg(test)]
pub struct SlowTestEmbedder {
    inner: FakeEmbedder,
    delay_ms: u64,
}

#[cfg(test)]
impl SlowTestEmbedder {
    pub fn new(delay_ms: u64) -> Self {
        Self {
            inner: FakeEmbedder::new("slow-test", 8),
            delay_ms,
        }
    }
}

#[cfg(test)]
#[async_trait]
impl Embedder for SlowTestEmbedder {
    fn model_id(&self) -> &str {
        "slow-test"
    }
    fn dimension(&self) -> usize {
        8
    }
    fn version(&self) -> &str {
        "slow-v1"
    }
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        self.inner.embed_query(query).await
    }
    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
        self.inner.embed_documents(texts).await
    }
}

/// Canonical configured model helpers — single source for indexing/query/status.
/// CONTEXTD_EMBED_MODEL drives all; DO NOT hardcode elsewhere.
pub fn configured_model_name() -> String {
    std::env::var("CONTEXTD_EMBED_MODEL").unwrap_or_else(|_| "all-minilm".to_string())
}

pub fn configured_fingerprint() -> ModelFingerprint {
    let model = configured_model_name();
    match model.as_str() {
        "all-minilm" => ModelFingerprint {
            model_id: "all-minilm".to_string(),
            version: "ollama-all-minilm-v1".to_string(),
            dimension: 384,
        },
        "nomic-embed-text" => ModelFingerprint {
            model_id: "nomic-embed-text".to_string(),
            version: "ollama-nomic-embed-text-v1".to_string(),
            dimension: 768,
        },
        "qwen3-embedding:0.6b" => ModelFingerprint {
            model_id: "qwen3-embedding:0.6b".to_string(),
            version: "ollama-qwen3-embedding:0.6b-v1".to_string(),
            dimension: 1024,
        },
        "qwen3-embedding" => ModelFingerprint {
            model_id: "qwen3-embedding".to_string(),
            version: "ollama-qwen3-embedding-v1".to_string(),
            dimension: 1024,
        },
        other => ModelFingerprint {
            model_id: other.to_string(),
            version: format!("ollama-{}-v1", other),
            dimension: 768,
        },
    }
}

pub fn configured_embedder() -> OllamaEmbedder {
    let fp = configured_fingerprint();
    OllamaEmbedder::with_model(&fp.model_id, fp.dimension)
}

/// Returns true if Ollama daemon reachable AND configured model is listed in /api/tags.
/// If daemon exists but model absent, returns false (truthful unavailable).
pub async fn is_configured_model_available() -> bool {
    // honor semantic disable
    if let Ok(v) = std::env::var("CONTEXTD_SEMANTIC_ENABLED") {
        if v == "0" || v.to_lowercase() == "false" {
            return false;
        }
    }
    let fp = configured_fingerprint();
    is_model_available(&fp.model_id).await
}

pub async fn is_model_available(model_id: &str) -> bool {
    let ollama_host =
        std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let url = format!("{}/api/tags", ollama_host.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let resp = match tokio::time::timeout(
        std::time::Duration::from_millis(800),
        client.get(&url).send(),
    )
    .await
    {
        Ok(Ok(r)) if r.status().is_success() => r,
        _ => return false,
    };
    // Parse tags JSON: {"models": [{"name":"all-minilm:latest", ...}]}
    if let Ok(json) = resp.json::<serde_json::Value>().await {
        if let Some(models) = json.get("models").and_then(|v| v.as_array()) {
            for m in models {
                if let Some(name) = m.get("name").and_then(|v| v.as_str()) {
                    // name may contain tag suffix like ":latest" or digest
                    let base = name.split(':').next().unwrap_or(name);
                    if base == model_id
                        || name == model_id
                        || name.starts_with(&format!("{}:", model_id))
                    {
                        return true;
                    }
                }
                if let Some(model) = m.get("model").and_then(|v| v.as_str()) {
                    let base = model.split(':').next().unwrap_or(model);
                    if base == model_id {
                        return true;
                    }
                }
            }
            // If models array empty, but daemon reachable, we still consider model unavailable
            return false;
        }
        // Some Ollama versions return empty or different shape but daemon reachable — if we expected a specific model, treat as unavailable
        // However for backward compat: if tags endpoint succeeds but parsing fails, assume daemon reachable but can't verify model -> require model
        return false;
    }
    false
}

/// Nomic via Ollama (historical baseline).
/// Calls http://localhost:11434/api/embed by default.
pub struct OllamaEmbedder {
    model: String,
    dim: usize,
    version: String,
    endpoint: String,
    client: reqwest::Client,
}

impl OllamaEmbedder {
    pub fn nomic() -> Self {
        Self {
            model: "nomic-embed-text".to_string(),
            dim: 768,
            version: "ollama-nomic-v1".to_string(),
            endpoint: std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".to_string())
                + "/api/embed",
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
    pub fn with_model(model: &str, dim: usize) -> Self {
        Self {
            model: model.to_string(),
            dim,
            version: format!("ollama-{}-v1", model),
            endpoint: std::env::var("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://localhost:11434".to_string())
                + "/api/embed",
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    fn model_id(&self) -> &str {
        &self.model
    }
    fn dimension(&self) -> usize {
        self.dim
    }
    fn version(&self) -> &str {
        &self.version
    }
    async fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": query
        });
        let resp = self.client.post(&self.endpoint).json(&body).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("ollama embed failed: {}", resp.status());
        }
        let json: serde_json::Value = resp.json().await?;
        // Ollama returns { embeddings: [[...]] } or { embedding: [...] }
        if let Some(arr) = json.get("embeddings").and_then(|v| v.as_array()) {
            if let Some(first) = arr.first().and_then(|v| v.as_array()) {
                let vec: Vec<f32> = first
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();
                if vec.len() != self.dim {
                    anyhow::bail!(
                        "dimension mismatch: expected {}, got {}",
                        self.dim,
                        vec.len()
                    );
                }
                // normalize for cosine
                let mut v = vec;
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                for x in &mut v {
                    *x /= norm;
                }
                return Ok(v);
            }
        }
        if let Some(arr) = json.get("embedding").and_then(|v| v.as_array()) {
            let vec: Vec<f32> = arr
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            if vec.len() != self.dim {
                anyhow::bail!(
                    "dimension mismatch: expected {}, got {}",
                    self.dim,
                    vec.len()
                );
            }
            let mut v = vec;
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
            for x in &mut v {
                *x /= norm;
            }
            return Ok(v);
        }
        anyhow::bail!("unexpected ollama embed response: {}", json);
    }
    async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Batch via single call if possible, but fallback to sequential
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Ollama /api/embed supports batch input as array
        let body = serde_json::json!({
            "model": self.model,
            "input": texts
        });
        let resp = self.client.post(&self.endpoint).json(&body).send().await;
        if let Ok(r) = resp {
            if r.status().is_success() {
                if let Ok(json) = r.json::<serde_json::Value>().await {
                    if let Some(arr) = json.get("embeddings").and_then(|v| v.as_array()) {
                        let mut out = Vec::new();
                        for emb in arr {
                            if let Some(a) = emb.as_array() {
                                let mut v: Vec<f32> = a
                                    .iter()
                                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                                    .collect();
                                if v.len() != self.dim {
                                    anyhow::bail!(
                                        "dimension mismatch in batch: expected {}, got {}",
                                        self.dim,
                                        v.len()
                                    );
                                }
                                let norm: f32 =
                                    v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                                for x in &mut v {
                                    *x /= norm;
                                }
                                out.push(v);
                            }
                        }
                        if out.len() == texts.len() {
                            return Ok(out);
                        }
                    }
                }
            }
        }
        // Fallback sequential
        let mut out = Vec::new();
        for t in texts {
            out.push(self.embed_query(t).await?);
        }
        Ok(out)
    }
}

/// Bounded query embedding cache.
/// Key = model_id + query, bounded to 128 entries, LRU eviction simple.
pub struct QueryCache {
    map: Mutex<HashMap<String, Vec<f32>>>,
    order: Mutex<Vec<String>>,
    cap: usize,
}

impl QueryCache {
    pub fn new(cap: usize) -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            cap,
        }
    }
    fn key(fp: &ModelFingerprint, query: &str) -> String {
        format!("{}::{}", fp.key(), query)
    }
    fn key_model(model: &str, query: &str) -> String {
        format!("{}::{}", model, query)
    }
    pub async fn get(&self, fp: &ModelFingerprint, query: &str) -> Option<Vec<f32>> {
        let k = Self::key(fp, query);
        self.map.lock().await.get(&k).cloned()
    }
    // For tests that use raw model string (legacy)
    pub async fn get_model(&self, model: &str, query: &str) -> Option<Vec<f32>> {
        let k = Self::key_model(model, query);
        self.map.lock().await.get(&k).cloned()
    }
    pub async fn insert(&self, fp: &ModelFingerprint, query: &str, vec: Vec<f32>) {
        let k = Self::key(fp, query);
        let mut map = self.map.lock().await;
        let mut order = self.order.lock().await;
        if map.contains_key(&k) {
            map.insert(k.clone(), vec);
            // move to end
            order.retain(|x| x != &k);
            order.push(k);
            return;
        }
        if map.len() >= self.cap {
            if let Some(old) = order.first().cloned() {
                order.remove(0);
                map.remove(&old);
            }
        }
        map.insert(k.clone(), vec);
        order.push(k);
    }
    pub async fn insert_model(&self, model: &str, query: &str, vec: Vec<f32>) {
        let k = Self::key_model(model, query);
        let mut map = self.map.lock().await;
        let mut order = self.order.lock().await;
        if map.contains_key(&k) {
            map.insert(k.clone(), vec);
            order.retain(|x| x != &k);
            order.push(k);
            return;
        }
        if map.len() >= self.cap {
            if let Some(old) = order.first().cloned() {
                order.remove(0);
                map.remove(&old);
            }
        }
        map.insert(k.clone(), vec);
        order.push(k);
    }
}

/// Global small cache
pub static QUERY_CACHE: LazyLock<Arc<QueryCache>> =
    LazyLock::new(|| Arc::new(QueryCache::new(128)));

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_deterministic() {
        let e = FakeEmbedder::new("fake-test", 8);
        let v1 = e.embed_query("hello world").await.unwrap();
        let v2 = e.embed_query("hello world").await.unwrap();
        assert_eq!(v1, v2);
        let v3 = e.embed_query("different").await.unwrap();
        assert_ne!(v1, v3);
        assert_eq!(v1.len(), 8);
        // normalized
        let norm: f32 = v1.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn cache_bounded() {
        let c = QueryCache::new(2);
        let fp = ModelFingerprint {
            model_id: "m".into(),
            version: "v1".into(),
            dimension: 8,
        };
        c.insert(&fp, "q1", vec![1.0]).await;
        c.insert(&fp, "q2", vec![2.0]).await;
        assert!(c.get(&fp, "q1").await.is_some());
        c.insert(&fp, "q3", vec![3.0]).await;
        // q1 should be evicted (LRU)
        assert!(c.get(&fp, "q1").await.is_none());
        assert!(c.get(&fp, "q3").await.is_some());
    }

    #[tokio::test]
    async fn cache_key_includes_fingerprint() {
        let c = QueryCache::new(10);
        let fp1 = ModelFingerprint {
            model_id: "m".into(),
            version: "v1".into(),
            dimension: 8,
        };
        let fp2 = ModelFingerprint {
            model_id: "m".into(),
            version: "v2".into(),
            dimension: 8,
        };
        let fp3 = ModelFingerprint {
            model_id: "m".into(),
            version: "v1".into(),
            dimension: 16,
        };
        c.insert(&fp1, "q", vec![1.0]).await;
        assert!(c.get(&fp1, "q").await.is_some());
        assert!(
            c.get(&fp2, "q").await.is_none(),
            "different version should not hit cache"
        );
        assert!(
            c.get(&fp3, "q").await.is_none(),
            "different dimension should not hit cache"
        );
    }
}
