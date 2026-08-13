use context_index::{exact_search, ExactQuery, ExactSearchOptions, ProjectIndex, ProjectRoot};
use std::path::PathBuf;
use std::process::Command;

/// Helper to run V2 exactSearch via node.
fn v2_exact_search(query: &str, literal: bool, limit: usize) -> Vec<String> {
    let js = format!(
        r#"
import {{ exactSearch }} from './v2/dist/retrieval/exactSearch.js';
import {{ setActiveProjectRoot }} from './v2/dist/retrieval/codeIndexClient.js';
import path from 'node:path';
setActiveProjectRoot(path.resolve('.'));
const res = await exactSearch('{}', {{ literal: {}, limit: {} }});
console.log(JSON.stringify(res.map(r=>r.file)));
"#,
        query.replace('\'', "\\'").replace('"', "\\\""),
        literal,
        limit
    );
    let out = Command::new("node")
        .args(["--input-type=module", "-e", &js])
        .current_dir("C:/Users/Dell/context/Context-Engine")
        .output()
        .expect("failed to spawn node");
    if !out.status.success() {
        eprintln!("v2 err: {}", String::from_utf8_lossy(&out.stderr));
        return vec![];
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Find JSON array line
    for line in stdout.lines().rev() {
        let t = line.trim();
        if t.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<Vec<String>>(t) {
                return v;
            }
        }
    }
    vec![]
}

#[tokio::test]
#[ignore]
async fn parity_count_tokens() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let rust = exact_search(
        &idx,
        ExactQuery::Literal("count_tokens".into()),
        ExactSearchOptions {
            max_results: 50,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let rust_files: Vec<String> = rust.iter().map(|e| e.file.clone()).collect();
    let v2_files = v2_exact_search("count_tokens", true, 50);
    println!("rust {:?}", rust_files);
    println!("v2 {:?}", v2_files);
    assert!(
        rust_files.iter().any(|f| f.ends_with("core/utils.py")),
        "rust should contain core/utils.py, got {:?}",
        rust_files
    );
    assert!(
        v2_files.iter().any(|f| f.ends_with("core/utils.py")),
        "v2 should contain core/utils.py, got {:?}",
        v2_files
    );
}

#[tokio::test]
#[ignore]
async fn parity_redact_secrets() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let rust = exact_search(
        &idx,
        ExactQuery::Literal("redact_secrets".into()),
        ExactSearchOptions {
            max_results: 100,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let rust_files: Vec<String> = rust.iter().map(|e| e.file.clone()).collect();
    let v2_files = v2_exact_search("redact_secrets", true, 100);
    println!("rust {:?}", rust_files);
    println!("v2 {:?}", v2_files);
    assert!(
        rust_files.iter().any(|f| f.ends_with("core/utils.py")),
        "rust {:?}",
        rust_files
    );
    assert!(
        v2_files.iter().any(|f| f.ends_with("core/utils.py")),
        "v2 {:?}",
        v2_files
    );
}

#[tokio::test]
#[ignore]
async fn parity_bundle() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let rust = exact_search(
        &idx,
        ExactQuery::Literal("bundle".into()),
        ExactSearchOptions {
            max_results: 5,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let v2_files = v2_exact_search("bundle", true, 5);
    let rust_files: Vec<String> = rust.iter().map(|e| e.file.clone()).collect();
    println!("rust bundle {:?}", rust_files);
    println!("v2 bundle {:?}", v2_files);
    assert!(!rust_files.is_empty());
    assert!(!v2_files.is_empty());
}

#[tokio::test]
#[ignore]
async fn parity_filename() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    // Test with a file that exists in Context-Engine: Cargo.toml at root
    let rust = exact_search(
        &idx,
        ExactQuery::FileName("Cargo.toml".into()),
        ExactSearchOptions::default(),
    )
    .await
    .unwrap();
    println!(
        "Cargo.toml rust {:?}",
        rust.iter().map(|e| &e.file).collect::<Vec<_>>()
    );
    assert!(!rust.is_empty(), "Cargo.toml should be found");
    assert!(rust.iter().any(|e| e.file.ends_with("Cargo.toml")));
    // Also test a Go file on Mulanous if available, but for Context-Engine just check Cargo.toml
}

#[tokio::test]
#[ignore]
async fn parity_regex() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let rust = exact_search(
        &idx,
        ExactQuery::Regex("Health.*Handler".into()),
        ExactSearchOptions {
            max_results: 5,
            ..Default::default()
        },
    )
    .await;
    println!("regex rust {:?}", rust.as_ref().map(|v| v.len()));
    // V2 regex may also be 0, but both should not error for valid regex
    assert!(rust.is_ok());
}

#[tokio::test]
#[ignore]
async fn parity_nonexistent() {
    let root =
        ProjectRoot::resolve(Some(&PathBuf::from("C:/Users/Dell/context/Context-Engine"))).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let rust = exact_search(
        &idx,
        ExactQuery::Literal("nonexistent_token_xyz_123".into()),
        ExactSearchOptions::default(),
    )
    .await
    .unwrap();
    let v2_files = v2_exact_search("nonexistent_token_xyz_123", true, 5);
    assert!(rust.is_empty());
    assert!(v2_files.is_empty());
}
