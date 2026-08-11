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
                    tracing::warn!(expected=%self.dim, got=%vec.len(), "dimension mismatch");
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
    fn key(model: &str, query: &str) -> String {
        format!("{}::{}", model, query)
    }
    pub async fn get(&self, model: &str, query: &str) -> Option<Vec<f32>> {
        let k = Self::key(model, query);
        self.map.lock().await.get(&k).cloned()
    }
    pub async fn insert(&self, model: &str, query: &str, vec: Vec<f32>) {
        let k = Self::key(model, query);
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
        c.insert("m", "q1", vec![1.0]).await;
        c.insert("m", "q2", vec![2.0]).await;
        assert!(c.get("m", "q1").await.is_some());
        c.insert("m", "q3", vec![3.0]).await;
        // q1 should be evicted (LRU)
        assert!(c.get("m", "q1").await.is_none());
        assert!(c.get("m", "q3").await.is_some());
    }
}
