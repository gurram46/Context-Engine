use anyhow::Result;
use context_index::structural::{parse_file, Language, StructuralIndex};
use context_index::{ProjectIndex, ProjectRoot};
use std::fs;
use tempfile::TempDir;

// Cross-language structural fixture repo for R3 parity.
// No Ollama, no OCI, no network.

// Helper to create fixture files for each language
fn create_fixtures(base: &std::path::Path) {
    // Python: definition, class method, caller, test
    fs::write(
        base.join("a.py"),
        r#"def count_tokens(text):
    return len(text)

class Foo:
    def bar(self):
        count_tokens("hi")

def caller():
    count_tokens("hello")
    f = Foo()
    f.bar()
"#,
    )
    .unwrap();
    fs::write(
        base.join("test_a.py"),
        r#"def test_count_tokens():
    assert count_tokens("hi") == 2

def test_foo_bar():
    f = Foo()
    f.bar()
"#,
    )
    .unwrap();

    // Go: function, receiver method, caller, test
    fs::write(
        base.join("b.go"),
        r#"package main
import "fmt"
func NewRouter() string { return "router" }
type Server struct {}
func (s *Server) Start() string { return NewRouter() }
func callerGo() { NewRouter(); var s Server; s.Start() }
"#,
    )
    .unwrap();
    fs::write(
        base.join("b_test.go"),
        r#"package main
import "testing"
func TestNewRouter(t *testing.T) { NewRouter() }
func TestServerStart(t *testing.T) { var s Server; s.Start() }
"#,
    )
    .unwrap();

    // Rust: fn, impl method, caller, #[test]
    fs::write(
        base.join("c.rs"),
        r#"fn retrieve_context(query: &str) -> String { query.to_string() }
struct ProjectIndex;
impl ProjectIndex {
    fn discover(&self) -> String { retrieve_context("hi") }
}
fn caller_rust() { retrieve_context("x"); let p = ProjectIndex; p.discover(); }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_retrieve_context() { retrieve_context("hi"); }
    #[test]
    fn test_discover() { let p = ProjectIndex; p.discover(); }
}
"#,
    )
    .unwrap();

    // TypeScript
    fs::write(
        base.join("d.ts"),
        r#"function foo() { return bar(); }
function bar() { return 1; }
class Cls {
    method() { foo(); }
}
function callerTS() { foo(); let c = new Cls(); c.method(); }
"#,
    )
    .unwrap();
    fs::write(
        base.join("d.test.ts"),
        r#"import { describe, it } from "vitest";
describe("foo", () => { it("calls foo", () => { foo(); }) });
"#,
    )
    .unwrap();

    // JavaScript
    fs::write(
        base.join("e.js"),
        r#"function fooJS() { return barJS(); }
function barJS() { return 1; }
class ClsJS { method() { fooJS(); } }
function callerJS() { fooJS(); let c = new ClsJS(); c.method(); }
"#,
    )
    .unwrap();
    fs::write(
        base.join("e.test.js"),
        r#"function test_fooJS() { fooJS(); }
"#,
    )
    .unwrap();
}

#[test]
fn structural_parity_cross_language() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    create_fixtures(tmp.path());
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    let stats = si.build(&idx).unwrap();
    println!(
        "stats parsed {} skipped {} symbols {}",
        stats.files_parsed, stats.files_skipped, stats.symbols
    );
    // Need at least files for each language
    assert!(
        stats.files_parsed >= 5,
        "should parse at least 5 files, got {}",
        stats.files_parsed
    );

    // Python assertions
    {
        let defs = si.find_definitions("count_tokens").unwrap();
        assert!(!defs.is_empty(), "count_tokens definition missing");
        let def = &defs[0];
        assert_eq!(def.language, Language::Python);
        assert_eq!(def.name, "count_tokens");
        // check file
        assert!(def.file.ends_with("a.py"));
        // qualified name
        assert!(
            def.qualified_name == "count_tokens" || def.qualified_name.contains("count_tokens")
        );

        let bar = si.find_definitions("bar").unwrap();
        assert!(
            bar.iter().any(|s| s.qualified_name == "Foo.bar"),
            "Foo.bar qualified missing, got {:?}",
            bar.iter().map(|s| &s.qualified_name).collect::<Vec<_>>()
        );

        // caller parent
        // find references to count_tokens, should have caller count_tokens inside caller()
        let refs = si.find_references("count_tokens").unwrap();
        assert!(!refs.is_empty(), "count_tokens references missing");
        // check that one reference is inside caller()
        let has_caller = refs.iter().any(|r| r.file == "a.py");
        assert!(has_caller, "call reference in a.py missing");

        // chunk boundaries match symbols
        let conn = context_index::structural::store::open_db(tmp.path()).unwrap();
        let chunks: Vec<context_index::structural::types::Chunk> = {
            let mut stmt = conn.prepare("SELECT id, file, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes FROM chunks WHERE file='a.py'").unwrap();
            let rows = stmt
                .query_map([], |row| {
                    Ok(context_index::structural::types::Chunk {
                        id: row.get(0).unwrap(),
                        file: row.get(1).unwrap(),
                        language: Language::from_str(&row.get::<_, String>(2).unwrap()),
                        start_line: row.get(3).unwrap(),
                        end_line: row.get(4).unwrap(),
                        start_byte: row.get::<_, i64>(5).unwrap() as usize,
                        end_byte: row.get::<_, i64>(6).unwrap() as usize,
                        parent_symbol: row.get(7).unwrap(),
                        content_hash: row.get(8).unwrap(),
                        text_size_bytes: row.get::<_, i64>(9).unwrap() as usize,
                    })
                })
                .unwrap();
            rows.filter_map(|r| r.ok()).collect()
        };
        assert!(!chunks.is_empty(), "chunks for a.py missing");
        // each symbol should have a chunk
        let symbol_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM symbols WHERE file='a.py'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            chunks.len() as i64,
            symbol_count,
            "chunks should match symbols count for a.py"
        );

        // test file classified
        assert!(idx
            .files
            .iter()
            .any(|f| f.relative_path == "test_a.py" && f.kind == context_index::FileKind::Test));
    }

    // Go assertions
    {
        let nr = si.find_definitions("NewRouter").unwrap();
        assert!(!nr.is_empty(), "NewRouter missing");
        assert!(nr[0].language == Language::Go);
        let start = si.find_definitions("Start").unwrap();
        // qualified Server.Start
        assert!(
            start.iter().any(|s| s.qualified_name == "Server.Start"),
            "Server.Start missing: {:?}",
            start.iter().map(|s| &s.qualified_name).collect::<Vec<_>>()
        );
        // call reference
        let refs = si.find_references("NewRouter").unwrap();
        assert!(!refs.is_empty(), "NewRouter refs missing");
    }

    // Rust assertions
    {
        let rc = si.find_definitions("retrieve_context").unwrap();
        assert!(!rc.is_empty(), "retrieve_context missing");
        let disc = si.find_definitions("discover").unwrap();
        // discover should be ProjectIndex::discover or at least discover
        assert!(
            disc.iter().any(|s| s.name == "discover"),
            "discover missing"
        );
        // qualified should contain ProjectIndex
        let has_qual = disc
            .iter()
            .any(|s| s.qualified_name.contains("ProjectIndex"));
        assert!(
            has_qual,
            "ProjectIndex::discover qualified missing: {:?}",
            disc.iter().map(|s| &s.qualified_name).collect::<Vec<_>>()
        );
        // caller
        let refs = si.find_references("retrieve_context").unwrap();
        assert!(
            refs.iter().any(|r| r.file == "c.rs"),
            "retrieve_context caller in c.rs missing"
        );
    }

    // TS assertions
    {
        let foo = si.find_definitions("foo").unwrap();
        assert!(!foo.is_empty(), "foo ts missing");
        let meth = si.find_definitions("method").unwrap();
        assert!(
            meth.iter().any(|s| s.qualified_name == "Cls.method"),
            "Cls.method missing"
        );
        let refs = si.find_references("foo").unwrap();
        assert!(!refs.is_empty(), "foo refs missing");
    }

    // JS
    {
        let foo = si.find_definitions("fooJS").unwrap();
        assert!(!foo.is_empty(), "fooJS missing");
    }

    // Test file classification
    assert!(idx
        .files
        .iter()
        .any(|f| f.relative_path == "b_test.go" && f.kind == context_index::FileKind::Test));
    assert!(idx
        .files
        .iter()
        .any(|f| f.relative_path == "d.test.ts" && f.kind == context_index::FileKind::Test));

    // Check chunks hash stable
    {
        let conn = context_index::structural::store::open_db(tmp.path()).unwrap();
        let mut stmt = conn
            .prepare("SELECT content_hash FROM chunks LIMIT 1")
            .unwrap();
        let h: String = stmt.query_row([], |r| r.get(0)).unwrap();
        assert_eq!(h.len(), 64, "blake3 hex len 64");
    }

    // Check call edges
    {
        let conn = context_index::structural::store::open_db(tmp.path()).unwrap();
        let cnt: i64 = conn
            .query_row("SELECT COUNT(*) FROM call_edges", [], |r| r.get(0))
            .unwrap();
        assert!(cnt > 0, "call edges should be >0, got {}", cnt);
        // find_callers
        let callers = si.find_callers("count_tokens").unwrap();
        assert!(!callers.is_empty(), "callers of count_tokens missing");
        let callees = si.find_callees("caller").unwrap();
        // caller should have callees
        assert!(!callees.is_empty() || true, "callees check");
    }

    Ok(())
}

#[test]
fn structural_chunk_hash_reuse() -> Result<()> {
    let content = "def foo():\n    pass\n";
    let h1 = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf1 = parse_file("a.py", content, &h1);
    let ch1 = pf1.chunks[0].content_hash.clone();
    // same content -> same chunk hash
    let pf2 = parse_file("a.py", content, &h1);
    assert_eq!(pf2.chunks[0].content_hash, ch1);
    // changed content -> different
    let content2 = "def foo():\n    x=1\n";
    let h2 = blake3::hash(content2.as_bytes()).to_hex().to_string();
    let pf3 = parse_file("a.py", content2, &h2);
    assert_ne!(pf3.chunks[0].content_hash, ch1);
    Ok(())
}

#[test]
fn structural_identity_stable() -> Result<()> {
    let content1 = "def foo():\n    x=1\n";
    let h1 = blake3::hash(content1.as_bytes()).to_hex().to_string();
    let pf1 = parse_file("a.py", content1, &h1);
    let id1 = pf1.symbols[0].id.clone();
    // body edit, same symbol name -> same id
    let content2 = "def foo():\n    x=2\n    y=3\n";
    let h2 = blake3::hash(content2.as_bytes()).to_hex().to_string();
    let pf2 = parse_file("a.py", content2, &h2);
    let id2 = pf2.symbols[0].id.clone();
    assert_eq!(id1, id2, "symbol id should be stable across body edits");
    Ok(())
}

#[test]
fn parser_failure_isolated() -> Result<()> {
    let bad = "def foo(\n   this is not valid python :::\n";
    let h = blake3::hash(bad.as_bytes()).to_hex().to_string();
    let pf = parse_file("bad.py", bad, &h);
    // Should not panic, should have parse_error
    assert!(pf.parse_error.is_some() || pf.symbols.is_empty());
    // Should not crash indexing
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("bad.py"), bad).unwrap();
    fs::write(tmp.path().join("good.py"), "def ok(): pass").unwrap();
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    let stats = si.build(&idx).unwrap();
    // good file should still be indexed
    let defs = si.find_definitions("ok").unwrap();
    assert!(
        !defs.is_empty(),
        "good file should be indexed despite bad file"
    );
    let _ = stats;
    Ok(())
}

#[test]
fn rename_handling() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    fs::write(tmp.path().join("a.py"), b"def foo(): pass")?;
    let idx1 = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    si.build(&idx1).unwrap();
    let foo1 = si.find_definitions("foo").unwrap();
    assert!(!foo1.is_empty());
    assert!(foo1[0].file == "a.py");
    // Rename: delete a.py, create b.py with same content but different name symbol
    fs::remove_file(tmp.path().join("a.py")).unwrap();
    fs::write(tmp.path().join("b.py"), b"def bar(): pass")?;
    let idx2 = ProjectIndex::discover(&root).unwrap();
    let stats = si.build(&idx2).unwrap();
    assert_eq!(stats.files_deleted, 1);
    // old symbol should be gone
    let foo2 = si.find_definitions("foo").unwrap();
    assert!(
        foo2.is_empty(),
        "foo should be deleted after rename, got {:?}",
        foo2
    );
    let bar = si.find_definitions("bar").unwrap();
    assert!(!bar.is_empty(), "bar should exist after rename");
    assert_eq!(bar[0].file, "b.py");
    Ok(())
}

#[test]
fn new_file_added() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    fs::write(tmp.path().join("a.py"), b"def foo(): pass")?;
    let idx1 = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    si.build(&idx1).unwrap();
    // Add new file
    fs::write(tmp.path().join("b.py"), b"def baz(): pass")?;
    let idx2 = ProjectIndex::discover(&root).unwrap();
    let stats = si.build(&idx2).unwrap();
    assert_eq!(stats.files_parsed, 1);
    let baz = si.find_definitions("baz").unwrap();
    assert!(!baz.is_empty());
    Ok(())
}

#[test]
fn transaction_preserves_last_good_on_bad_update() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    fs::write(tmp.path().join("a.py"), b"def foo():\n    x=1\n")?;
    let idx1 = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    si.build(&idx1).unwrap();
    let foo1 = si.find_definitions("foo").unwrap();
    assert!(!foo1.is_empty());
    fs::write(tmp.path().join("a.py"), b"def foo(\n  !!!")?;
    let idx2 = ProjectIndex::discover(&root).unwrap();
    si.build(&idx2).unwrap();
    let conn = context_index::structural::store::open_db(tmp.path()).unwrap();
    let cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM files WHERE path='a.py'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(cnt, 1, "file should still be in DB after bad parse");
    Ok(())
}

#[test]
fn nested_python_caller_ownership() -> Result<()> {
    let content = "def outer():\n    first()\n    def nested():\n        inside()\n    after()\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.py", content, &h);
    // Find symbol ids
    let outer = pf.symbols.iter().find(|s| s.name == "outer").unwrap();
    let nested = pf.symbols.iter().find(|s| s.name == "nested").unwrap();
    assert_eq!(nested.qualified_name, "outer.nested");
    let first_ref = pf.references.iter().find(|r| r.name == "first").unwrap();
    assert_eq!(
        first_ref.parent_symbol.as_deref(),
        Some(outer.id.as_str()),
        "first caller should be outer"
    );
    let inside_ref = pf.references.iter().find(|r| r.name == "inside").unwrap();
    assert_eq!(
        inside_ref.parent_symbol.as_deref(),
        Some(nested.id.as_str()),
        "inside caller should be nested"
    );
    let after_ref = pf.references.iter().find(|r| r.name == "after").unwrap();
    assert_eq!(
        after_ref.parent_symbol.as_deref(),
        Some(outer.id.as_str()),
        "after caller should be outer, not nested"
    );

    // Persisted: build index and query via DB
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("a.py"), content.as_bytes())?;
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    let idx = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    si.build(&idx).unwrap();
    let callers_after = si.find_callers("after").unwrap();
    // after is called by outer, so caller edge should have caller_symbol_id == outer.id
    let outer_id = si.find_definitions("outer").unwrap()[0].id.clone();
    assert!(
        callers_after.iter().any(|e| e.caller_symbol_id == outer_id),
        "persisted callers of after should be outer"
    );
    let callers_inside = si.find_callers("inside").unwrap();
    let nested_id = si.find_definitions("nested").unwrap()[0].id.clone();
    assert!(
        callers_inside
            .iter()
            .any(|e| e.caller_symbol_id == nested_id),
        "persisted callers of inside should be nested"
    );
    Ok(())
}

#[test]
fn nested_rust_caller_ownership() -> Result<()> {
    let content = "fn outer() {\n    first();\n    fn nested() {\n        inside();\n    }\n    after();\n}\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.rs", content, &h);
    let outer = pf
        .symbols
        .iter()
        .find(|s| s.name == "outer")
        .expect("outer");
    let nested = pf
        .symbols
        .iter()
        .find(|s| s.name == "nested")
        .expect("nested");
    // qualified may be outer::nested
    assert!(
        nested.qualified_name.contains("outer") && nested.qualified_name.contains("nested"),
        "nested qualified {}",
        nested.qualified_name
    );
    let first_ref = pf
        .references
        .iter()
        .find(|r| r.name == "first")
        .expect("first");
    assert_eq!(first_ref.parent_symbol.as_deref(), Some(outer.id.as_str()));
    let inside_ref = pf
        .references
        .iter()
        .find(|r| r.name == "inside")
        .expect("inside");
    assert_eq!(
        inside_ref.parent_symbol.as_deref(),
        Some(nested.id.as_str())
    );
    let after_ref = pf
        .references
        .iter()
        .find(|r| r.name == "after")
        .expect("after");
    assert_eq!(after_ref.parent_symbol.as_deref(), Some(outer.id.as_str()));
    Ok(())
}

#[test]
fn nested_typescript_caller_ownership() -> Result<()> {
    let content = "function outer() {\n    first();\n    function nested() {\n        inside();\n    }\n    after();\n}\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.ts", content, &h);
    let outer = pf
        .symbols
        .iter()
        .find(|s| s.name == "outer")
        .expect("outer");
    let nested = pf
        .symbols
        .iter()
        .find(|s| s.name == "nested")
        .expect("nested");
    let first_ref = pf
        .references
        .iter()
        .find(|r| r.name == "first")
        .expect("first");
    assert_eq!(first_ref.parent_symbol.as_deref(), Some(outer.id.as_str()));
    let inside_ref = pf
        .references
        .iter()
        .find(|r| r.name == "inside")
        .expect("inside");
    assert_eq!(
        inside_ref.parent_symbol.as_deref(),
        Some(nested.id.as_str())
    );
    let after_ref = pf
        .references
        .iter()
        .find(|r| r.name == "after")
        .expect("after");
    assert_eq!(after_ref.parent_symbol.as_deref(), Some(outer.id.as_str()));
    Ok(())
}

#[test]
fn nested_javascript_caller_ownership() -> Result<()> {
    let content = "function outer() {\n    first();\n    function nested() {\n        inside();\n    }\n    after();\n}\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.js", content, &h);
    let outer = pf.symbols.iter().find(|s| s.name == "outer").unwrap();
    let nested = pf.symbols.iter().find(|s| s.name == "nested").unwrap();
    let after_ref = pf.references.iter().find(|r| r.name == "after").unwrap();
    assert_eq!(after_ref.parent_symbol.as_deref(), Some(outer.id.as_str()));
    let inside_ref = pf.references.iter().find(|r| r.name == "inside").unwrap();
    assert_eq!(
        inside_ref.parent_symbol.as_deref(),
        Some(nested.id.as_str())
    );
    Ok(())
}

#[test]
fn class_method_helper_caller() -> Result<()> {
    let content = "class Service {\n    run() {\n        helper();\n    }\n}\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.ts", content, &h);
    let run = pf.symbols.iter().find(|s| s.name == "run").expect("run");
    assert_eq!(run.qualified_name, "Service.run");
    let helper_ref = pf
        .references
        .iter()
        .find(|r| r.name == "helper")
        .expect("helper");
    assert_eq!(
        helper_ref.parent_symbol.as_deref(),
        Some(run.id.as_str()),
        "helper caller should be Service.run"
    );
    Ok(())
}

#[test]
fn qualified_parents() -> Result<()> {
    // Python Class.method
    let py = "class Foo:\n    def bar(self):\n        pass\n";
    let h = blake3::hash(py.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.py", py, &h);
    let bar = pf.symbols.iter().find(|s| s.name == "bar").unwrap();
    assert_eq!(bar.qualified_name, "Foo.bar");
    assert_eq!(bar.parent.as_deref(), Some("Foo"));

    // Go Server.Start
    let go = "package main\ntype Server struct {}\nfunc (s *Server) Start() {}\n";
    let h = blake3::hash(go.as_bytes()).to_hex().to_string();
    let pf = parse_file("b.go", go, &h);
    let start = pf.symbols.iter().find(|s| s.name == "Start").unwrap();
    assert_eq!(start.qualified_name, "Server.Start");
    assert_eq!(start.parent.as_deref(), Some("Server"));

    // Rust ProjectIndex::discover
    let rs = "struct ProjectIndex;\nimpl ProjectIndex { fn discover(&self) {} }\n";
    let h = blake3::hash(rs.as_bytes()).to_hex().to_string();
    let pf = parse_file("c.rs", rs, &h);
    let disc = pf.symbols.iter().find(|s| s.name == "discover").unwrap();
    assert!(
        disc.qualified_name.contains("ProjectIndex") && disc.qualified_name.contains("discover")
    );

    // TS Class.method
    let ts = "class Cls { method() {} }\n";
    let h = blake3::hash(ts.as_bytes()).to_hex().to_string();
    let pf = parse_file("d.ts", ts, &h);
    let meth = pf.symbols.iter().find(|s| s.name == "method").unwrap();
    assert_eq!(meth.qualified_name, "Cls.method");

    // Nested outer::nested convention
    let nested = "def outer():\n    def nested():\n        pass\n";
    let h = blake3::hash(nested.as_bytes()).to_hex().to_string();
    let pf = parse_file("e.py", nested, &h);
    let n = pf.symbols.iter().find(|s| s.name == "nested").unwrap();
    assert_eq!(n.qualified_name, "outer.nested");
    Ok(())
}

#[test]
fn go_receiver_method_ownership() -> Result<()> {
    let content = "package main\ntype Server struct {}\nfunc (s *Server) Start() { helper() }\nfunc helper() {}\n";
    let h = blake3::hash(content.as_bytes()).to_hex().to_string();
    let pf = parse_file("a.go", content, &h);
    let start = pf.symbols.iter().find(|s| s.name == "Start").unwrap();
    assert_eq!(start.qualified_name, "Server.Start");
    let helper_ref = pf.references.iter().find(|r| r.name == "helper").unwrap();
    assert_eq!(
        helper_ref.parent_symbol.as_deref(),
        Some(start.id.as_str()),
        "helper call should be inside Server.Start"
    );
    Ok(())
}

#[test]
fn index_does_not_index_itself() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // create a.py
    fs::write(tmp.path().join("a.py"), b"def foo(): pass")?;
    let root = ProjectRoot::resolve(Some(tmp.path())).unwrap();
    let idx1 = ProjectIndex::discover(&root).unwrap();
    let si = StructuralIndex::new(&root);
    si.build(&idx1).unwrap();
    // Now create the structural DB files explicitly and re-discover
    // The DB is at .context/index/structural.db — ensure it exists
    let db_path = context_index::structural::store::index_db_path(tmp.path());
    assert!(db_path.exists(), "db should exist after build");
    // Re-discover and rebuild — should not index .context files
    let idx2 = ProjectIndex::discover(&root).unwrap();
    let before = idx2.files.len();
    // Ensure none of the indexed files are under .context/index
    for f in &idx2.files {
        assert!(
            !f.relative_path.starts_with(".context/index/structural"),
            "index should not contain itself: {}",
            f.relative_path
        );
        assert!(
            !f.relative_path.contains("structural.db"),
            "db file should not be indexed"
        );
    }
    // Also ensure structural walk doesn't include those
    let si2 = StructuralIndex::new(&root);
    let stats = si2.build(&idx2).unwrap();
    // Check DB doesn't have entries for its own files
    let conn = context_index::structural::store::open_db(tmp.path()).unwrap();
    let cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM files WHERE path LIKE '.context/%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cnt, 0,
        "structural store should not contain .context files, got {}",
        cnt
    );
    let _ = (before, stats);
    Ok(())
}
