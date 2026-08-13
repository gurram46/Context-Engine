use anyhow::Result;
use context_index::discovery::ProjectIndex;
use context_index::embed::{Embedder, OllamaEmbedder};
use context_index::project_root::ProjectRoot;
use context_index::structural::store::open_db;
use context_index::structural::StructuralIndex;
use context_index::vector::sync_vectors_for_file;

#[tokio::main]
async fn main() -> Result<()> {
    let root = ProjectRoot::resolve(None)?;
    println!("root: {}", root.path().display());
    let idx = ProjectIndex::discover(&root)?;
    println!("discovered {} files", idx.files.len());
    let si = StructuralIndex::new(&root);
    println!("building structural + BM25...");
    let stats = si.build(&idx)?;
    println!(
        "build done: parsed {}, skipped {}, symbols {}, chunks {}",
        stats.files_parsed, stats.files_skipped, stats.symbols, stats.chunks
    );

    // Populate vectors for nomic
    let embedder = OllamaEmbedder::nomic();
    let fp = embedder.fingerprint();
    println!(
        "populating vectors for {} dim {} version {}",
        fp.model_id, fp.dimension, fp.version
    );
    let mut conn = open_db(root.path())?;
    // Load chunks grouped by file
    let mut by_file: std::collections::HashMap<
        String,
        Vec<context_index::structural::types::Chunk>,
    > = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT file, id, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes FROM chunks")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)? as usize,
                row.get::<_, i64>(6)? as usize,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)? as usize,
            ))
        })?;
        for r in rows {
            let (file, id, lang_s, sl, el, sb, eb, parent, ch, ts) = r?;
            let lang = context_index::structural::language::Language::from_str(&lang_s);
            by_file.entry(file.clone()).or_default().push(
                context_index::structural::types::Chunk {
                    id,
                    file,
                    language: lang,
                    start_line: sl as u32,
                    end_line: el as u32,
                    start_byte: sb,
                    end_byte: eb,
                    parent_symbol: parent,
                    content_hash: ch,
                    text_size_bytes: ts,
                },
            );
        }
    }
    let total_files = by_file.len();
    let mut total_reused = 0;
    let mut total_embedded = 0;
    let mut file_count = 0;
    for (file, chunks) in by_file {
        file_count += 1;
        if file_count % 20 == 0 {
            println!(
                "  {}/{} files, reused {}, embedded {}",
                file_count, total_files, total_reused, total_embedded
            );
        }
        let abs = root.path().join(&file);
        let content = std::fs::read_to_string(&abs).unwrap_or_default();
        let (reused, embedded) =
            sync_vectors_for_file(&mut conn, &file, &chunks, &content, &embedder).await?;
        total_reused += reused;
        total_embedded += embedded;
    }
    println!(
        "vector populate done: reused {}, embedded {}",
        total_reused, total_embedded
    );
    let cnt = context_index::vector::count_vectors(&conn, &fp)?;
    println!("total vectors for {}: {}", fp.model_id, cnt);

    // Second candidate all-minilm
    let embedder2 = OllamaEmbedder::with_model("all-minilm", 384);
    let fp2 = embedder2.fingerprint();
    println!(
        "populating vectors for {} dim {} version {}",
        fp2.model_id, fp2.dimension, fp2.version
    );
    // Reload by_file (same)
    let mut conn2 = open_db(root.path())?;
    let mut by_file2: std::collections::HashMap<
        String,
        Vec<context_index::structural::types::Chunk>,
    > = std::collections::HashMap::new();
    {
        let mut stmt2 = conn2.prepare("SELECT file, id, language, start_line, end_line, start_byte, end_byte, parent_symbol, content_hash, text_size_bytes FROM chunks")?;
        let rows2 = stmt2.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)? as usize,
                row.get::<_, i64>(6)? as usize,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, i64>(9)? as usize,
            ))
        })?;
        for r in rows2 {
            let (file, id, lang_s, sl, el, sb, eb, parent, ch, ts) = r?;
            let lang = context_index::structural::language::Language::from_str(&lang_s);
            by_file2.entry(file.clone()).or_default().push(
                context_index::structural::types::Chunk {
                    id,
                    file,
                    language: lang,
                    start_line: sl as u32,
                    end_line: el as u32,
                    start_byte: sb,
                    end_byte: eb,
                    parent_symbol: parent,
                    content_hash: ch,
                    text_size_bytes: ts,
                },
            );
        }
    }
    let mut total_reused2 = 0;
    let mut total_embedded2 = 0;
    let mut file_count2 = 0;
    let total_files2 = by_file2.len();
    for (file, chunks) in by_file2 {
        file_count2 += 1;
        if file_count2 % 20 == 0 {
            println!(
                "  {}/{} files, reused {}, embedded {}",
                file_count2, total_files2, total_reused2, total_embedded2
            );
        }
        let abs = root.path().join(&file);
        let content = std::fs::read_to_string(&abs).unwrap_or_default();
        let (reused, embedded) =
            sync_vectors_for_file(&mut conn2, &file, &chunks, &content, &embedder2).await?;
        total_reused2 += reused;
        total_embedded2 += embedded;
    }
    println!(
        "vector populate done for all-minilm: reused {}, embedded {}",
        total_reused2, total_embedded2
    );
    let cnt2 = context_index::vector::count_vectors(&conn2, &fp2)?;
    println!("total vectors for {}: {}", fp2.model_id, cnt2);

    Ok(())
}
