use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::service::{ContextService, Direction, SearchOptions};

#[derive(Parser, Debug)]
#[command(
    name = "contextd",
    version,
    about = "Context Engine - local repository intelligence"
)]
pub struct Cli {
    /// Project root
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<PathBuf>,

    /// Token budget for packed context
    #[arg(long, global = true)]
    pub budget: Option<usize>,

    /// Max results
    #[arg(long, global = true, value_name = "N")]
    pub max_results: Option<usize>,

    /// Enable debug tracing
    #[arg(long, global = true)]
    pub debug: bool,

    /// JSON output
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Natural language repository search
    Search {
        /// Natural language query
        query: String,
    },
    /// Lookup symbol definition
    Symbol { symbol: String },
    /// Trace dependency callers/callees
    Dependency {
        symbol: String,
        #[arg(long, default_value = "callers")]
        direction: String,
    },
    /// Find tests covering query/symbol
    Tests { query: String },
    /// Show index/status diagnostics
    Status,
    /// Run MCP stdio server (long-lived)
    Mcp,
}

fn opts(cli: &Cli) -> SearchOptions {
    SearchOptions {
        budget_tokens: cli.budget.unwrap_or(10000),
        max_results: cli.max_results.unwrap_or(10),
        debug: cli.debug,
    }
}

pub async fn run_cli(cli: Cli) -> Result<()> {
    // tracing to stderr only
    let filter = if cli.debug {
        "debug".to_string()
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .try_init();

    let root = cli.root.clone();
    let is_json = cli.json;
    let search_opts = opts(&cli);
    // Handle MCP via separate path — caller will handle, but keep here for safety
    match cli.command {
        Some(Command::Mcp) => {
            // MCP is handled in main.rs directly to avoid double init; fallback
            crate::mcp::run_mcp(root).await?;
            return Ok(());
        }
        Some(Command::Search { query }) => {
            let svc = ContextService::new(root).await?;
            let res = svc.search(&query, search_opts).await?;
            if is_json {
                let mut out = serde_json::json!({
                    "query": res.query,
                    "type": res.query_type.as_str(),
                    "evidence": res.evidence.iter().map(|e| serde_json::json!({
                        "file": e.file,
                        "lines": e.start_line.map(|s| format!("{}-{}", s, e.end_line.unwrap_or(s))).unwrap_or_default(),
                        "symbol": e.symbol,
                        "relation": e.relation.map(|r| r.as_str()),
                        "source": e.source.as_str(),
                        "score": e.score,
                        "authorityScore": e.authority_score,
                        "finalScore": e.final_score,
                    })).collect::<Vec<_>>(),
                    "context": res.packed.markdown,
                    "stats": {
                        "candidate_count": res.stats.candidate_count,
                        "evidence_count": res.stats.evidence_count,
                        "files_returned": res.stats.files_returned,
                        "packed_tokens": res.stats.packed_tokens,
                        "retrievers": res.stats.retrievers_used,
                        "elapsed_ms": res.stats.elapsed_ms,
                        "exact_ms": res.stats.exact_ms,
                        "structural_ms": res.stats.structural_ms,
                        "bm25_ms": res.stats.bm25_ms,
                        "semantic_ms": res.stats.semantic_ms,
                        "rank_ms": res.stats.rank_ms,
                        "pack_ms": res.stats.pack_ms,
                    }
                });
                // Debug observability only when --debug is set, not in normal JSON output
                if cli.debug {
                    let classified = context_rank::classify_query(&query);
                    let plan = context_rank::build_retrieval_plan(&query);
                    out["debug"] = serde_json::json!({
                        "classification": classified.query_type.as_str(),
                        "hints": classified.hints,
                        "identifiers": context_rank::extract_identifiers(&query),
                        "exact_queries": plan.exact_queries.iter().map(|q| q.as_str().to_string()).collect::<Vec<_>>(),
                        "symbol_queries": plan.symbol_queries,
                        "graph_queries": plan.graph_queries.iter().map(|g| serde_json::json!({"symbol": g.symbol, "direction": g.direction})).collect::<Vec<_>>(),
                        "test_queries": plan.test_queries,
                        "semantic_queries": plan.semantic_queries,
                    });
                }
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                // human readable
                if res.evidence.is_empty() {
                    println!("No evidence found for: {}", res.query);
                } else {
                    for (i, ev) in res.evidence.iter().enumerate() {
                        let loc = ev
                            .start_line
                            .map(|s| format!("{}:{}-{}", ev.file, s, ev.end_line.unwrap_or(s)))
                            .unwrap_or_else(|| ev.file.clone());
                        let sym = ev.symbol.clone().unwrap_or_default();
                        let rel = ev.relation.map(|r| r.as_str()).unwrap_or("unknown");
                        println!(
                            "{}. {} [{}] {} ({}) score={:.2} auth={:?}",
                            i + 1,
                            loc,
                            rel,
                            sym,
                            ev.source.as_str(),
                            ev.score.unwrap_or(0.0),
                            ev.authority_score
                        );
                        if let Some(t) = &ev.text {
                            let snippet: String = t.chars().take(200).collect();
                            println!("   {}", snippet.replace('\n', " "));
                        }
                    }
                    println!(
                        "\n--- Packed Context ({} tokens, {} files) ---",
                        res.stats.packed_tokens, res.stats.files_returned
                    );
                    println!(
                        "{}",
                        res.packed.markdown.chars().take(2000).collect::<String>()
                    );
                }
            }
        }
        Some(Command::Symbol { symbol }) => {
            let svc = ContextService::new(root).await?;
            let res = svc.symbol(&symbol, search_opts).await?;
            if is_json {
                let out = serde_json::json!({
                    "query": res.query,
                    "type": res.query_type.as_str(),
                    "evidence": res.evidence,
                    "context": res.packed.markdown,
                    "stats": res.stats
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                for ev in &res.evidence {
                    println!("{} {:?}", ev.file, ev.symbol);
                }
                println!(
                    "{}",
                    res.packed.markdown.chars().take(2000).collect::<String>()
                );
            }
        }
        Some(Command::Dependency { symbol, direction }) => {
            let svc = ContextService::new(root).await?;
            let dir = Direction::from_str(&direction);
            let res = svc.dependency(&symbol, dir, search_opts).await?;
            if is_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "query": res.query,
                        "type": res.query_type.as_str(),
                        "evidence": res.evidence,
                        "context": res.packed.markdown,
                        "stats": res.stats
                    }))?
                );
            } else {
                for ev in &res.evidence {
                    println!("{} {:?} {:?}", ev.file, ev.relation, ev.symbol);
                }
                println!(
                    "{}",
                    res.packed.markdown.chars().take(2000).collect::<String>()
                );
            }
        }
        Some(Command::Tests { query }) => {
            let svc = ContextService::new(root).await?;
            let res = svc.tests(&query, search_opts).await?;
            if is_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "query": res.query,
                        "type": res.query_type.as_str(),
                        "evidence": res.evidence,
                        "context": res.packed.markdown,
                        "stats": res.stats
                    }))?
                );
            } else {
                for ev in &res.evidence {
                    println!("{} {:?}", ev.file, ev.symbol);
                }
                println!(
                    "{}",
                    res.packed.markdown.chars().take(2000).collect::<String>()
                );
            }
        }
        Some(Command::Status) => {
            let svc = ContextService::new(root).await?;
            let st = svc.status().await?;
            if is_json {
                println!("{}", serde_json::to_string_pretty(&st)?);
            } else {
                println!("version: {}", st.version);
                println!("project root: {}", st.project_root);
                if let Some(b) = &st.git_branch {
                    println!("branch: {}", b);
                }
                println!("files indexed: {}", st.files_indexed);
                println!("symbols: {}", st.symbols);
                println!("BM25 documents: {}", st.bm25_documents);
                println!("vectors: {}", st.vector_count);
                println!("embedding model: {}", st.embedding_model);
                println!("embedding runtime: {}", st.embedding_runtime);
                println!("semantic available: {}", st.semantic_available);
                println!("watcher: {}", st.watcher_state);
                if let Some(g) = st.index_generation {
                    println!("generation: {}", g);
                }
                if let Some(v) = st.store_schema_version {
                    println!("schema version: {}", v);
                }
            }
        }
        None => {
            // no subcommand — print help
            use clap::CommandFactory;
            Cli::command().print_help()?;
            println!();
        }
    }
    Ok(())
}
