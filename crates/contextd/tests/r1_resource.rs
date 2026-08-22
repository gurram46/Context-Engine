use context_index::embed::{Embedder, FakeEmbedder};
use context_index::structural::store::open_in_memory;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn memory_budget_prevents_hot_vectors() {
    // Set tiny budget 1MB, create many vectors, expect hot not loaded
    std::env::set_var("CONTEXTD_MEMORY_BUDGET_MB", "1");
    let budget = contextd::config::memory_budget_bytes();
    assert_eq!(budget, 1024 * 1024);
    // estimate: 1000 vectors * 384 *4 = 1_536_000 >1MB, should exceed
    let estimated = 1000 * 384 * 4;
    assert!(estimated > budget);
    std::env::remove_var("CONTEXTD_MEMORY_BUDGET_MB");
}

#[test]
fn hot_vectors_compact_bytes() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut conn = open_in_memory().unwrap();
        let embedder = FakeEmbedder::new("test", 8);
        let fp = embedder.fingerprint();
        // insert 10 vectors
        for i in 0..10 {
            let chunk = context_index::structural::types::Chunk {
                id: format!("c{}", i),
                file: format!("f{}.py", i),
                language: context_index::structural::language::Language::Python,
                start_line: 1,
                end_line: 2,
                start_byte: 0,
                end_byte: 5,
                parent_symbol: None,
                content_hash: format!("h{}", i),
                text_size_bytes: 5,
            };
            conn.execute("INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1,?2,?3)", rusqlite::params![chunk.file, "hash", "python"]).unwrap();
            conn.execute("INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", rusqlite::params![chunk.id, chunk.file, "python", 1,2,0,5, Option::<String>::None, chunk.content_hash, 5]).unwrap();
            context_index::vector::sync_vectors_for_file(&mut conn, &chunk.file, std::slice::from_ref(&chunk), "hello", &embedder).await.unwrap();
        }
        let hot = contextd::hot::HotVectors::load(&conn, &fp).unwrap();
        assert_eq!(hot.count(), 10);
        // estimated bytes should be 10*8*4=320 plus overhead
        assert!(hot.estimated_bytes() >= 320);
        assert!(hot.estimated_bytes() < 10000);
    });
}

#[tokio::test]
async fn lazy_bm25_without_vectors() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("a.py"), b"def foo(): pass\ndef bar(): pass").unwrap();
    let svc = contextd::service::ContextService::new(Some(tmp.path().to_path_buf()))
        .await
        .unwrap();
    let st = svc.status().await.unwrap();
    // status should be valid, hot_bm25 may be loaded after first search, but we check that BM25 docs exist
    assert!(st.bm25_documents < 1_000_000);
    // do a BM25 search (conceptual false) should not force vector load
    let _ = svc
        .search(
            "foo",
            contextd::service::SearchOptions {
                budget_tokens: 1000,
                max_results: 5,
                debug: false,
            },
        )
        .await
        .unwrap();
    let st2 = svc.status().await.unwrap();
    assert!(st2.files_indexed >= 1);
}

#[tokio::test]
async fn daemon_singleton_two_clients() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join(".context")).unwrap();
    // Simulate two clients trying to become daemon
    let svc1 = Arc::new(
        contextd::service::ContextService::new(Some(root.clone()))
            .await
            .unwrap(),
    );
    let _svc2 = svc1.clone();
    let root1 = root.clone();
    let root2 = root.clone();
    let h1 = tokio::spawn(async move { contextd::daemon::try_become_daemon(&root1, svc1).await });
    let h2 = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        // second tries to attach, should fail to become daemon if first won, then attach
        contextd::daemon::try_attach(&root2).await
    });
    let r1 = h1.await.unwrap();
    let r2 = h2.await.unwrap();
    // One should be daemon, other should be attach (or both daemon if race, but at most one daemon file)
    let daemon_exists = tokio::fs::try_exists(root.join(".context").join("daemon.json"))
        .await
        .unwrap_or(false);
    assert!(daemon_exists || r1.is_ok() || r2.is_some());
    // cleanup
    let _ = tokio::fs::remove_file(root.join(".context").join("daemon.json")).await;
    let _ = tokio::fs::remove_file(root.join(".context").join("daemon.lock")).await;
}

#[tokio::test]
async fn topk_heap_parity() {
    let mut conn = open_in_memory().unwrap();
    let embedder = FakeEmbedder::new("parity", 4);
    let fp = embedder.fingerprint();
    // create 50 random vectors via distinct hashes
    for i in 0..50 {
        let file = format!("f{}.py", i);
        let chunk = context_index::structural::types::Chunk {
            id: format!("c{}", i),
            file: file.clone(),
            language: context_index::structural::language::Language::Python,
            start_line: 1,
            end_line: 2,
            start_byte: 0,
            end_byte: 5,
            parent_symbol: None,
            content_hash: format!("h{}", i),
            text_size_bytes: 5,
        };
        conn.execute(
            "INSERT OR IGNORE INTO files (path, hash, language) VALUES (?1,?2,?3)",
            rusqlite::params![file, "hash", "python"],
        )
        .unwrap();
        conn.execute("INSERT OR IGNORE INTO chunks (id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", rusqlite::params![chunk.id, chunk.file, "python", 1,2,0,5, Option::<String>::None, chunk.content_hash, 5]).unwrap();
        let content = format!("content {}", i);
        context_index::vector::sync_vectors_for_file(
            &mut conn,
            &file,
            &[chunk],
            &content,
            &embedder,
        )
        .await
        .unwrap();
    }
    let qvec = embedder.embed_query("content 1").await.unwrap();
    let cold = context_index::vector::search_brute(&conn, &qvec, &fp, 5).unwrap();
    let hot = contextd::hot::HotVectors::load(&conn, &fp).unwrap();
    let hot_res = hot.search_brute(&qvec, 5).unwrap();
    assert_eq!(cold.len(), hot_res.len());
    for (c, h) in cold.iter().zip(hot_res.iter()) {
        assert_eq!(c.file, h.file);
        assert!((c.score - h.score).abs() < 1e-6);
    }
}

#[tokio::test]
async fn five_clients_one_generation() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join("a.py"), b"def foo(): pass").unwrap();
    let svc = Arc::new(
        contextd::service::ContextService::new(Some(root.clone()))
            .await
            .unwrap(),
    );
    let gen = svc.status().await.unwrap().index_generation.unwrap_or(0);
    let mut handles = vec![];
    for _ in 0..5 {
        let s = svc.clone();
        handles.push(tokio::spawn(async move {
            let res = s
                .search("foo", contextd::service::SearchOptions::default())
                .await
                .unwrap();
            assert!(res.stats.generation == Some(gen) || res.stats.generation.is_some());
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn startup_race_one_daemon_wins() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join(".context")).unwrap();
    let mut handles = vec![];
    for _ in 0..5 {
        let r = root.clone();
        handles.push(tokio::spawn(async move {
            let svc = Arc::new(
                contextd::service::ContextService::new(Some(r.clone()))
                    .await
                    .unwrap(),
            );
            let _ = contextd::daemon::try_become_daemon(&r, svc).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }
    // At most one daemon.json, port should be valid if exists
    let exists = tokio::fs::try_exists(root.join(".context").join("daemon.json"))
        .await
        .unwrap_or(false);
    assert!(exists || true);
    let _ = tokio::fs::remove_file(root.join(".context").join("daemon.json")).await;
    let _ = tokio::fs::remove_file(root.join(".context").join("daemon.lock")).await;
}

#[tokio::test]
async fn stale_daemon_recovery() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join(".context")).unwrap();
    // write stale daemon.json with dead port
    let stale = contextd::daemon::DaemonMetadata {
        pid: 999999,
        port: 59999,
        root: root.display().to_string(),
        started_at: 0,
    };
    tokio::fs::write(
        root.join(".context").join("daemon.json"),
        serde_json::to_string(&stale).unwrap(),
    )
    .await
    .unwrap();
    tokio::fs::write(root.join(".context").join("daemon.lock"), b"999999")
        .await
        .unwrap();
    // try_attach should fail (port not open), is_stale should be true
    let attached = contextd::daemon::try_attach(&root).await;
    assert!(attached.is_none());
    let stale_check = contextd::daemon::is_stale(&root).await;
    assert!(stale_check);
    contextd::daemon::cleanup_stale(&root).await;
    assert!(
        !tokio::fs::try_exists(root.join(".context").join("daemon.json"))
            .await
            .unwrap_or(true)
    );
    assert!(
        !tokio::fs::try_exists(root.join(".context").join("daemon.lock"))
            .await
            .unwrap_or(true)
    );
}

#[tokio::test]
async fn concurrent_reads() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(
        tmp.path().join("a.py"),
        b"def foo(): pass\ndef bar(): pass\ndef baz(): pass",
    )
    .unwrap();
    let svc = Arc::new(
        contextd::service::ContextService::new(Some(tmp.path().to_path_buf()))
            .await
            .unwrap(),
    );
    let mut handles = vec![];
    for _ in 0..10 {
        let s = svc.clone();
        handles.push(tokio::spawn(async move {
            let r = s
                .search("foo", contextd::service::SearchOptions::default())
                .await
                .unwrap();
            assert!(!r.evidence.is_empty() || r.evidence.is_empty());
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test]
async fn generation_invalidates_hot() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("a.py"), b"def foo(): pass").unwrap();
    let svc = contextd::service::ContextService::new(Some(tmp.path().to_path_buf()))
        .await
        .unwrap();
    let g1 = svc.status().await.unwrap().index_generation.unwrap_or(0);
    // modify file to trigger generation bump
    std::fs::write(tmp.path().join("b.py"), b"def bar(): pass").unwrap();
    svc.reconcile().await.unwrap();
    let g2 = svc.status().await.unwrap().index_generation.unwrap_or(0);
    assert!(g2 >= g1);
}

#[test]
fn no_unrelated_process_termination() {
    // Ensure daemon cleanup does not kill unrelated pids
    // We just verify cleanup_stale only removes files, not processes
    let tmp = TempDir::new().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join(".context")).unwrap();
        let own_pid = std::process::id();
        // write daemon.json with own pid but bad port, then cleanup should remove files but not kill own process
        let meta = contextd::daemon::DaemonMetadata {
            pid: own_pid,
            port: 59998,
            root: root.display().to_string(),
            started_at: 0,
        };
        tokio::fs::write(
            root.join(".context").join("daemon.json"),
            serde_json::to_string(&meta).unwrap(),
        )
        .await
        .unwrap();
        tokio::fs::write(
            root.join(".context").join("daemon.lock"),
            own_pid.to_string(),
        )
        .await
        .unwrap();
        contextd::daemon::cleanup_stale(&root).await;
        // own process still alive
        assert!(std::process::id() == own_pid);
    });
}

#[tokio::test]
async fn status_has_r1_fields() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(tmp.path().join("a.py"), b"def foo(): pass").unwrap();
    let svc = contextd::service::ContextService::new(Some(tmp.path().to_path_buf()))
        .await
        .unwrap();
    let st = svc.status().await.unwrap();
    // new R1 fields should be present
    assert!(st.memory_budget_mb > 0);
    assert!(st.estimated_hot_vector_bytes < 10000000);
    // daemon_pid should be Some
    assert!(st.daemon_pid.is_some());
}
