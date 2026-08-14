//! Synthetic cross-language test retrieval — verifies generic test lookup without benchmark-specific logic.
//! Covers Python, TypeScript, Go, and Rust inline tests via isolated TempDir fixtures.

use context_index::project_root::ProjectRoot;
use context_index::structural::StructuralIndex;
use context_index::ProjectIndex;

use contextd::pipeline::{retrieve_context, Providers};

fn setup_git(tmp: &std::path::Path) {
    std::fs::create_dir_all(tmp.join(".git")).expect("git");
}

#[tokio::test]
async fn test_retrieval_python() {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root = tmp.path();
    setup_git(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    // Source
    std::fs::write(root.join("src/foo.py"), b"def Foo():\n    pass\n").unwrap();
    // Dedicated test file test_foo.py (Python convention)
    std::fs::write(
        root.join("tests/test_foo.py"),
        b"def test_foo():\n    assert Foo() is not None\n",
    )
    .unwrap();
    // Distractor
    std::fs::write(root.join("src/other.py"), b"def other(): pass\n").unwrap();

    let pr = ProjectRoot::resolve(Some(root)).expect("root");
    let idx = ProjectIndex::discover(&pr).expect("idx");
    let si = StructuralIndex::new(&pr);
    si.build(&idx).expect("build");

    let project = ProjectIndex::discover(&pr).expect("idx2");
    let res = retrieve_context("What tests cover Foo?", &project, &Providers {}, 10000, 5)
        .await
        .expect("retrieve");

    let files: Vec<&str> = res.evidence.iter().map(|e| e.file.as_str()).collect();
    assert!(
        files.iter().any(|f| f.ends_with("test_foo.py")),
        "Should find tests/test_foo.py for Foo, got {:?}",
        files
    );
}

#[tokio::test]
async fn test_retrieval_typescript() {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root = tmp.path();
    setup_git(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/foo.ts"), b"export function Foo() {}\n").unwrap();
    std::fs::write(
        root.join("src/foo.spec.ts"),
        b"import { Foo } from './foo';\ndescribe('Foo', () => { it('works', () => { Foo(); }); });\n",
    )
    .unwrap();
    std::fs::write(root.join("src/other.ts"), b"export function other() {}\n").unwrap();

    let pr = ProjectRoot::resolve(Some(root)).expect("root");
    let idx = ProjectIndex::discover(&pr).expect("idx");
    let si = StructuralIndex::new(&pr);
    si.build(&idx).expect("build");

    let project = ProjectIndex::discover(&pr).expect("idx2");
    let res = retrieve_context("What tests cover Foo?", &project, &Providers {}, 10000, 5)
        .await
        .expect("retrieve");

    let files: Vec<&str> = res.evidence.iter().map(|e| e.file.as_str()).collect();
    assert!(
        files.iter().any(|f| f.ends_with("foo.spec.ts")),
        "Should find foo.spec.ts for Foo, got {:?}",
        files
    );
}

#[tokio::test]
async fn test_retrieval_go() {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root = tmp.path();
    setup_git(root);
    std::fs::write(root.join("go.mod"), b"module example.com/foo\n").unwrap();
    std::fs::write(root.join("foo.go"), b"package foo\nfunc Foo() {}\n").unwrap();
    std::fs::write(
        root.join("foo_test.go"),
        b"package foo\nimport \"testing\"\nfunc TestFoo(t *testing.T) { Foo() }\n",
    )
    .unwrap();
    std::fs::write(root.join("other.go"), b"package foo\nfunc Other() {}\n").unwrap();

    let pr = ProjectRoot::resolve(Some(root)).expect("root");
    let idx = ProjectIndex::discover(&pr).expect("idx");
    let si = StructuralIndex::new(&pr);
    si.build(&idx).expect("build");

    let project = ProjectIndex::discover(&pr).expect("idx2");
    let res = retrieve_context("What tests cover Foo?", &project, &Providers {}, 10000, 5)
        .await
        .expect("retrieve");

    let files: Vec<&str> = res.evidence.iter().map(|e| e.file.as_str()).collect();
    assert!(
        files.iter().any(|f| f.ends_with("foo_test.go")),
        "Should find foo_test.go for Foo, got {:?}",
        files
    );
}

#[tokio::test]
async fn test_retrieval_rust_inline() {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root = tmp.path();
    setup_git(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        b"[package]\nname=\"foo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    // Source with inline #[cfg(test)]
    std::fs::write(
        root.join("src/lib.rs"),
        b"pub struct Standard;\nimpl Standard { pub fn new() -> Self { Self } }\n#[cfg(test)]\nmod tests { use super::*; #[test] fn test_standard() { let _ = Standard::new(); } }\n",
    )
    .unwrap();

    let pr = ProjectRoot::resolve(Some(root)).expect("root");
    let idx = ProjectIndex::discover(&pr).expect("idx");
    let si = StructuralIndex::new(&pr);
    si.build(&idx).expect("build");

    let project = ProjectIndex::discover(&pr).expect("idx2");
    let res = retrieve_context(
        "What tests cover the Standard printer?",
        &project,
        &Providers {},
        10000,
        5,
    )
    .await
    .expect("retrieve");

    let files: Vec<&str> = res.evidence.iter().map(|e| e.file.as_str()).collect();
    // For inline tests, the implementation file itself is valid
    assert!(
        files.iter().any(|f| f.ends_with("src/lib.rs")),
        "Should find src/lib.rs inline tests for Standard, got {:?}",
        files
    );
}

#[tokio::test]
async fn test_retrieval_q_objects() {
    // Regression for django Q objects — single-letter identifier
    let tmp = tempfile::TempDir::new().expect("tmp");
    let root = tmp.path();
    setup_git(root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests/queries")).unwrap();
    std::fs::write(root.join("src/q.py"), b"class Q:\n    pass\n").unwrap();
    std::fs::write(
        root.join("tests/queries/test_q.py"),
        b"def test_q():\n    from src.q import Q\n    assert Q() is not None\n",
    )
    .unwrap();

    let pr = ProjectRoot::resolve(Some(root)).expect("root");
    let idx = ProjectIndex::discover(&pr).expect("idx");
    let si = StructuralIndex::new(&pr);
    si.build(&idx).expect("build");

    let project = ProjectIndex::discover(&pr).expect("idx2");
    let res = retrieve_context(
        "What tests cover Q objects?",
        &project,
        &Providers {},
        10000,
        5,
    )
    .await
    .expect("retrieve");

    let files: Vec<&str> = res.evidence.iter().map(|e| e.file.as_str()).collect();
    assert!(
        files.iter().any(|f| f.ends_with("test_q.py")),
        "Should find test_q.py for Q objects, got {:?}",
        files
    );
}
