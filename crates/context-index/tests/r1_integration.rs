use context_index::{exact_search, ExactQuery, ExactSearchOptions, ProjectIndex, ProjectRoot};
use std::path::PathBuf;
use std::time::Instant;

#[tokio::test]
#[ignore]
async fn cross_repo_context_engine() {
    let root = ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine")))
        .or_else(|_| ProjectRoot::resolve(None))
        .unwrap();
    let t0 = Instant::now();
    let idx = ProjectIndex::discover(&root).unwrap();
    let elapsed = t0.elapsed();
    println!("Context-Engine: {} files, source {}, test {}, doc {}, config {}, generated {}, total_bytes {}, elapsed {:?}", idx.stats.discovered, idx.stats.source, idx.stats.test, idx.stats.doc, idx.stats.config, idx.stats.generated, idx.stats.total_bytes, elapsed);
    assert!(idx.stats.discovered > 100, "should discover >100 files");
    assert!(
        elapsed.as_millis() < 1500,
        "discovery <1500ms for 149 files, got {:?}",
        elapsed
    );
}

#[tokio::test]
#[ignore]
async fn cross_repo_mulanous() {
    let m_path = PathBuf::from("C:/Users/Dell/Mulanous-Lens");
    if !m_path.exists() {
        eprintln!("Mulanous-Lens not found, skipping");
        return;
    }
    let root = ProjectRoot::resolve(Some(&m_path)).unwrap();
    let t0 = Instant::now();
    let idx = ProjectIndex::discover(&root).unwrap();
    let elapsed = t0.elapsed();
    println!(
        "Mulanous: {} files, source {}, test {}, elapsed {:?}",
        idx.stats.discovered, idx.stats.source, idx.stats.test, elapsed
    );
    assert!(idx.stats.discovered > 50);
}

#[tokio::test]
#[ignore]
async fn hash_reuse_experiment() {
    use std::fs;
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    fs::write(tmp.path().join("b.txt"), b"world").unwrap();
    let idx1 = ProjectIndex::discover(&root).unwrap();
    let a_hash1 = idx1
        .files
        .iter()
        .find(|f| f.relative_path == "a.txt")
        .unwrap()
        .content_hash
        .clone();
    let b_hash1 = idx1
        .files
        .iter()
        .find(|f| f.relative_path == "b.txt")
        .unwrap()
        .content_hash
        .clone();
    // modify one file
    fs::write(tmp.path().join("a.txt"), b"hello world").unwrap();
    let idx2 = ProjectIndex::discover(&root).unwrap();
    let a_hash2 = idx2
        .files
        .iter()
        .find(|f| f.relative_path == "a.txt")
        .unwrap()
        .content_hash
        .clone();
    let b_hash2 = idx2
        .files
        .iter()
        .find(|f| f.relative_path == "b.txt")
        .unwrap()
        .content_hash
        .clone();
    assert_ne!(a_hash1, a_hash2, "a hash should change");
    assert_eq!(b_hash1, b_hash2, "b hash should stay same");
    println!(
        "hash reuse: a {:?} -> {:?}, b unchanged {:?}",
        a_hash1, a_hash2, b_hash1
    );
}

#[tokio::test]
#[ignore]
async fn memory_small() {
    let root = ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine")))
        .or_else(|_| ProjectRoot::resolve(None))
        .unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    // Rough memory: files * avg 200 bytes + overhead
    let est_bytes = idx.files.len() * 256;
    let mb = est_bytes as f64 / 1024.0 / 1024.0;
    println!("est metadata {} files ~{:.2} MB", idx.files.len(), mb);
    assert!(
        mb < 5.0,
        "metadata should be <5 MB for ~1k files, got {:.2}",
        mb
    );
}

#[tokio::test]
#[ignore]
async fn exact_latency() {
    let root = ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine")))
        .or_else(|_| ProjectRoot::resolve(None))
        .unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let queries = vec![
        (ExactQuery::Literal("count_tokens".into()), "count_tokens"),
        (
            ExactQuery::Literal("redact_secrets".into()),
            "redact_secrets",
        ),
        (ExactQuery::Identifier("bundle".into()), "bundle"),
        (ExactQuery::FileName("go.mod".into()), "go.mod"),
        (ExactQuery::Regex("Health.*Handler".into()), "regex"),
    ];
    for (q, label) in queries {
        let t0 = Instant::now();
        let res = exact_search(
            &idx,
            q,
            ExactSearchOptions {
                max_results: 10,
                ..Default::default()
            },
        )
        .await;
        let elapsed = t0.elapsed();
        match res {
            Ok(ev) => println!("{}: {} results in {:?}", label, ev.len(), elapsed),
            Err(e) => println!("{} error: {} in {:?}", label, e, elapsed),
        }
        assert!(
            elapsed.as_millis() < 1500,
            "{} took too long: {:?}",
            label,
            elapsed
        );
    }
}
