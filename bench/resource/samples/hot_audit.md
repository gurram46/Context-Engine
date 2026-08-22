# R0 Hot-State Resource Audit — Evidence (code-first)

> Ponytail lite: facts + file:line. No fixes. Thoroughness: medium.

## 1. HotBm25 — `crates/contextd/src/hot.rs`

```rust
// hot.rs:12-19
pub struct HotBm25 {
    n: usize,
    avgdl: f64,
    docs: HashMap<String, Bm25Doc>,              // key = doc_id (owned)
    postings: HashMap<String, Vec<(String, usize)>>, // term -> [(doc_id, tf)]
}
#[derive(Clone, Debug)] // hot.rs:21-31
struct Bm25Doc { doc_id: String, chunk_id: String, file: String, content_hash: String, length: usize, symbol: Option<String>, start_line: u32, end_line: u32 }
```

**Load path `HotBm25::load` hot.rs:34-88**
- `hot.rs:36` `let mut docs: HashMap<String,Bm25Doc> = HashMap::new();`
- `hot.rs:59` `docs.insert(d.doc_id.clone(), d);` — **String duplication #1**: `doc_id` owned both as `HashMap` key *and* inside `Bm25Doc.doc_id` (2 heap allocs per doc, `clone()` = memcopy).
- `hot.rs:68` `let mut postings: HashMap<String, Vec<(String, usize)>> = HashMap::new();`
- `hot.rs:80` `postings.entry(term).or_default().push((doc_id, tf));` — each posting clones `doc_id` again; same doc_id lives in *3* places (docs-key, Bm25Doc.doc_id, postings value). `term` moved as key; no dedup with search-time query `String` keys.

**Retained sizes (let N = doc_count, U = unique_terms, P = total postings)**
- `docs`: 1 `HashMap` allocation + N buckets + N * (key String heap ~ doc_id len 64 + Bm25Doc 4 Strings: doc_id, chunk_id, file, content_hash (+ symbol Option)) → ~4-5 heap `String` allocs per doc.
- `postings`: 1 `HashMap` + U buckets + U `Vec` heap allocs (one `Vec<(String,usize)>` per term) + P `String` heap allocs for doc_id clones inside vectors. `df` not stored separately (derived via `list.len()` at `hot.rs:116`) but still O(U) Vec overhead.
- `Vec` allocation count: `1 (docs HashMap table) + 1 (postings table) + U (per-term Vecs)` plus per-String heaps. No interning, no `Arc<str>`.

**Search `HotBm25::search` hot.rs:91-177 — per-query temps**
- `hot.rs:96` `let mut query_terms_set: HashSet<String> = HashSet::new();` → up to Q inserts (Q = deduped query tokens)
- `hot.rs:103` `let mut query_terms: Vec<String> = query_terms_set.into_iter().collect();` — clone/move + sort
- `hot.rs:112` `let mut df_map: HashMap<String, usize> = HashMap::new();` + `hot.rs:116` `df_map.insert(term.clone(), list.len());` — clones query term again
- `hot.rs:120` `let mut doc_term_tfs: HashMap<String, HashMap<String, usize>> = HashMap::new();` — **nested HashMap**: D outer entries (D = docs matching query terms), each with inner `HashMap<String,usize>` per term → D * avg_terms_per_doc heap HashMaps
- `hot.rs:121` `let mut doc_ids_set: HashSet<String> = HashSet::new();` + `hot.rs:129` `doc_ids_set.insert(doc_id.clone());`
- `hot.rs:137` `let mut scored: Vec<(String, f64)> = Vec::new();` → D entries, each String cloned doc_id
- `hot.rs:156` `scored.sort_by(...)` — full sort O(D log D), then `hot.rs:161` `scored.truncate(limit);`
- `hot.rs:162` `let mut out = Vec::new();` + `hot.rs:164-172` per result clones 4 Strings (`file.clone(), chunk_id.clone(), symbol.clone(), content_hash.clone()`)

**Cold parity** `crates/context-index/src/bm25.rs:289-455` `search_bm25` uses same nested `postings: HashMap<String, HashMap<String,usize>>:357` and `doc_meta: HashMap<String,...>:387` — DB variant does not bound memory either but not Hot.

## 2. HotVectors — `crates/contextd/src/hot.rs:184-355`

```rust
// hot.rs:185-191
pub struct HotVectors {
    fingerprint: ModelFingerprint,
    vectors: Vec<(String, Vec<f32>)>,                 // (representation_hash hex, embedding)
    chunk_map: HashMap<String, Vec<VectorChunkInfo>>, // hash -> chunks
}
// hot.rs:193-201
struct VectorChunkInfo { chunk_id: String, file: String, start_line: u32, end_line: u32, parent_symbol: Option<String>, content_hash: String }
```

**Load `HotVectors::load` hot.rs:203-289**
- `hot.rs:209` `let mut vectors: Vec<(String, Vec<f32>)> = Vec::new();` — reserve grows geometrically (no `with_capacity`)
- `hot.rs:234` `let mut vecf = Vec::with_capacity(dim);` + `hot.rs:235-238` `for chunk in blob.chunks_exact(4) { vecf.push(f32::from_le_bytes(...)) }` → **1 heap alloc per vector** of `dim * 4` bytes. For `all-minilm` dim 384 ⇒ 1.5 KiB/vector; nomic 768 ⇒ 3.0 KiB/vector. `hash` String (64 hex) cloned at `hot.rs:242` `vectors.push((hash, vecf));`
- `hot.rs:246` `let mut chunk_map: HashMap<String, Vec<VectorChunkInfo>> = HashMap::new();`
- `hot.rs:272` `chunk_map.entry(hash).or_default().push(info);` — **hash duplication**: same 64-char `representation_hash` stored once in `vectors[i].0` *and* again as `chunk_map` key (2 heap allocs per distinct hash, independent `String` copies).
- `VectorChunkInfo` per chunk: 3 mandatory heap Strings (`chunk_id`, `file`, `content_hash`) + optional `parent_symbol` → 3-4 allocs per chunk ref. `chunk_id` file `content_hash` duplicate values already present in `HotBm25::docs` with **no sharing/interning** across hot structures.

**Retained sizes (V = distinct representation_hash count, C = total chunk_refs, Ddim = fingerprint.dimension)**
- `vectors`: 1 `Vec` table + V * (String 64B heap + Vec<f32> heap `Ddim*4`).
  Example: V=20k, 384-dim → vectors heap ≈ 20k*1.5KiB = 30 MiB + 20k*64B strings ≈ 1.3 MiB + Vec overhead 20k*24B ≈ 0.5 MiB.
  V=20k, 768-dim → ≈ 60 MiB + same string overhead.
- `chunk_map`: 1 `HashMap` table + V buckets + V `Vec<VectorChunkInfo>` heaps + C * (3-4 Strings). Each `Vec<VectorChunkInfo>` sorted `hot.rs:276-283` for determinism (in-place, no extra alloc).
- Total `Vec` heap allocations: `V (embeddings) + V (chunk_map per-hash Vecs)` + HashMap tables.

## 3. `search_brute` — `hot.rs:300-354` vs cold `context-index/src/vector.rs:841-937`

```rust
// hot.rs:308-321
let mut scored: Vec<(String, f32)> = Vec::with_capacity(self.vectors.len()); // full N
for (hash, vec) in &self.vectors {
    let dot: f32 = vec.iter().zip(query_vec.iter()).map(|(a,b)| a*b).sum();
    scored.push((hash.clone(), dot)); // per-vector String clone
}
scored.sort_by(|a,b| b.1.partial_cmp(&a.1)...then_with(|| a.0.cmp(&b.0))); // full sort
scored.truncate(limit*2);
```

- **Scored Vec**: `hot.rs:308` `Vec::with_capacity(V)` — always allocates V capacity (V = vectors.len()), even when `limit` = 10. Each `push` at `hot.rs:314` clones hash String (64B heap + memcpy). Peak: V * (String heap + 32B `String` struct + f32) resident until truncate (truncate keeps capacity).
- **Full sort vs top-K**: `hot.rs:316-320` does `O(V log V)` full sort with `partial_cmp` + string tie-breaker, not `select_nth_unstable` / binary heap `O(V log K)`. Cold `vector.rs:883-889` identical full sort. Oversample `limit*2` at `hot.rs:321` does not reduce sort cost. For V=50k, `limit=10`, waste ≈ 5k× sort comparisons.
- **Out expansion**: `hot.rs:323-343` iterates `scored` (≤ `limit*2` hashes), does `chunk_map.get(&hash)` then `hot.rs:327-332` clones per chunk: `file.clone(), chunk_id.clone(), parent_symbol.clone(), content_hash.clone()` up to `limit` times. Then final deterministic sort `hot.rs:344-351` `out.sort_by(...)` O(limit log limit) + `truncate(limit)`.
- **Cold query**: `vector.rs:867` `let mut scored: Vec<(String,f32)> = Vec::new();` (no reserve, grows), `vector.rs:870-876` per row decodes `blob_to_vec` allocating a temporary `Vec<f32>` per vector then drops; then same full sort.

## 4. Semantic Indexing — `crates/context-index/src/vector.rs`

**Batch size**
- Production `vector.rs:481` `sync_missing_vectors_for_root` delegates to `sync_missing_vectors_for_root_with_batch_size(..., 256)` with `256` hardcoded. `vector.rs:720` `for chunk in missing.chunks(256)` in `sync_changed_files_vectors`. Tests use `8` (`vector.rs:481 comment`, `vector.rs:1446` `fail_on_call`).
- `sync_vectors_for_file` `vector.rs:820-821` batches whole-file `missing` at once (`let texts: Vec<String> = missing.iter().map(|(_,t)| t.clone()).collect();`) — size = distinct hashes changed in file (typically < 10), but no cap needed.
- `vector.rs:612` mid-loop flush `if pending.len() >= batch_size { drain(..batch_size) }` keeps steady 256 cap.

**Pending rendered texts & clones**
- `vector.rs:516` `let mut pending: Vec<(String, String)> = Vec::new();` holds `(rep_hash, rendered_text)` for all missing distinct reps not yet embedded. `rendered_text` = `render_semantic_representation` `vector.rs:40-50` → `format!("{}\n{}\n{}\n{}", language, normalized_path, qualified, slice)` new String per chunk (heap size ≈ slice len + path + language + symbol + 3).
- `vector.rs:567` per-file `let mut file_hashes: Vec<(String,String)> = Vec::new();` + `vector.rs:595` `file_hashes.push((rep_hash.clone(), rendered))` dedup per file via `seen: HashSet<String>`. Then `vector.rs:606-608` for each `(h,text)` in `file_hashes`: `if get_vector(...).is_none() { pending.push((h,text)) }` — moves String, no clone if not missing.
- **Triple clone at embed boundary**:
  ```rust
  // vector.rs:612-614
  let batch: Vec<(String,String)> = pending.drain(..batch_size).collect(); // drain moves, batch owns
  let texts: Vec<String> = batch.iter().map(|(_,t)| t.clone()).collect(); // clone #1: text again
  let hashes: Vec<String> = batch.iter().map(|(h,_)| h.clone()).collect(); // clone #1: hash again
  ```
  Same triple clone in trailing loop `vector.rs:632-634`. And `sync_changed_files_vectors` `vector.rs:721-722` `texts: chunk.iter().map(|(_,t)| t.clone())` + `hashes: chunk.iter().map(|(h,_)| h.clone())`. `sync_vectors_for_file` `vector.rs:820` `texts: missing.iter().map(|(_,t)| t.clone())`.
- **Vector batch lifetime**: `vector.rs:616` `let vectors = embedder.embed_documents(&texts).await?;` — `texts`/`hashes`/`batch` live across `.await` (HTTP to Ollama, 15s timeout `embed.rs:257`). `vectors: Vec<Vec<f32>>` returned allocates `batch_size * dim * 4` bytes (e.g., 256*384*4 = 384 KiB batch, plus `Vec` table 256*24B). Then `vector.rs:618-620` per `(hash, vec)` does `upsert_vector` which at `vector.rs:133` `let blob = vec_to_blob(vector);` `v.iter().flat_map(|f| f.to_le_bytes()).collect()` allocates another `Vec<u8>` `dim*4` per vector before SQLite insert → batch peak ≈ 2× vector bytes + `texts` heap.
- `materialize_semantic_refs` `vector.rs:312-465` returns `HashMap<String,String>` `map: HashMap<hash, rendered>` `vector.rs:414` — for `sync_changed_files_vectors` path `vector.rs:697` this map holds all rendered texts for `changed_files` batch simultaneously (bounded by changed-file count, not global).

**Instrumentation retained** `vector.rs:517-521`, `643-651` `eprintln!` counters but no extra heap.

## 5. HotState Overall — `crates/contextd/src/hot.rs:357-407` + `crates/contextd/src/runtime.rs:62-77,69-70,234-336`

```rust
// hot.rs:357-364
pub struct HotState { pub generation: u64, pub fingerprint: ModelFingerprint, pub bm25: Arc<HotBm25>, pub vectors: Option<Arc<HotVectors>>, pub created_at: Instant }
// runtime.rs:69-70
hot: RwLock<Option<Arc<HotState>>>, hot_build_lock: Mutex<()>
```

**What is retained (read-only after creation, per-generation)**
- `HotState` Arc: 1 `HotBm25` Arc + optional 1 `HotVectors` Arc + `ModelFingerprint { model_id String, version String, dimension usize }` cloned at `hot.rs:285,401` (separate alloc from `HotVectors.fingerprint` clone).
- Memory not freed until `RuntimeData::publish` `runtime.rs:215-218` clears `hot` (`*hot_guard = None`) or `get_or_load_hot` replaces stale generation; old `Arc<HotState>` lives until all search futures drop it.
- DB connection not retained; `HotState::load_blocking` `hot.rs:372` opens fresh `structural_store::open_db`, does `BEGIN/COMMIT` `hot.rs:374,398` for snapshot, loads `HotBm25::load` then `HotVectors::load`.

**What is duplicated**
- `file` path: `HotBm25.docs[*].file` String per doc + `HotVectors.chunk_map[*].file` String per chunk_ref → same path string owned twice (no `Arc<str>`/intern). `content_hash`: same — `Bm25Doc.content_hash` and `VectorChunkInfo.content_hash` duplicate per chunk.
- `representation_hash` hex: `HotVectors.vectors[i].0` and `HotVectors.chunk_map` key duplicate (2× per V).
- `chunk_id`: if a chunk is indexed both lexically and semantically, its `chunk_id` appears in `Bm25Doc.chunk_id` *and* `VectorChunkInfo.chunk_id` (different structs, separate heap).
- `ModelFingerprint`: owned in `HotState.fingerprint` and `HotVectors.fingerprint` (clone at `hot.rs:285`).
- `content_hash` also in `semantic_chunk_refs` table but Hot copy duplicates DB string.

**Counts summary**
- Total heap String allocs ≈ N*5 (BM25) + V*2 (hash dup) + C*3.5 (vectors chunk_map) with N≈C (postings/exact 1:1 doc↔chunk), so ≈ 8-10 Strings per chunk.
- Total Vec heap allocs ≈ (1 + U) (BM25 postings) + V (embeddings) + V (chunk_map per-hash) + 1 (pending/sync transients).
- No sharing via `Arc<str>` or intern table; no `Cow`; no `Box<[f32]>` compaction.

---
*Generated 2026-08-22 from `hot.rs:12-407`, `bm25.rs:1-603`, `vector.rs:40-826`, `embed.rs:10-470`, `runtime.rs:62-336`. No code changed.*
