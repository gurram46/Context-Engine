use crate::structural::language::{detect_language, Language};
use crate::structural::types::{
    chunk_id, symbol_id, CallConfidence, CallEdge, Chunk, Import, ParsedFile, Reference,
    ReferenceKind, Symbol, SymbolKind, Visibility,
};
use std::path::Path;
use tree_sitter::{Language as TsLanguage, Parser};

/// Parse file content into ParsedFile using tree-sitter.
pub fn parse_file(relative_path: &str, content: &str, content_hash: &str) -> ParsedFile {
    let lang = detect_language(Path::new(relative_path));
    if lang == Language::Unknown {
        return ParsedFile {
            file: relative_path.to_string(),
            language: lang,
            content_hash: content_hash.to_string(),
            symbols: vec![],
            references: vec![],
            imports: vec![],
            chunks: vec![],
            parse_error: None,
        };
    }

    let ts_lang = language_to_ts(lang);
    let mut parser = Parser::new();
    if let Some(l) = ts_lang {
        if parser.set_language(&l).is_err() {
            return ParsedFile {
                file: relative_path.to_string(),
                language: lang,
                content_hash: content_hash.to_string(),
                symbols: vec![],
                references: vec![],
                imports: vec![],
                chunks: vec![],
                parse_error: Some("failed to set language".into()),
            };
        }
    } else {
        return ParsedFile {
            file: relative_path.to_string(),
            language: lang,
            content_hash: content_hash.to_string(),
            symbols: vec![],
            references: vec![],
            imports: vec![],
            chunks: vec![],
            parse_error: Some("unknown language".into()),
        };
    }

    let tree = parser.parse(content, None);
    let tree = match tree {
        Some(t) => t,
        None => {
            return ParsedFile {
                file: relative_path.to_string(),
                language: lang,
                content_hash: content_hash.to_string(),
                symbols: vec![],
                references: vec![],
                imports: vec![],
                chunks: vec![],
                parse_error: Some("parse returned None".into()),
            }
        }
    };
    let has_error = tree.root_node().has_error();
    let parse_error = if has_error {
        Some("partial parse with errors".into())
    } else {
        None
    };

    let root = tree.root_node();
    let source = content.as_bytes();

    let mut symbols = Vec::new();
    let mut references = Vec::new();
    let mut imports = Vec::new();

    match lang {
        Language::Rust => extract_rust(
            relative_path,
            &root,
            source,
            &mut symbols,
            &mut imports,
            &mut references,
            lang,
        ),
        Language::Python => extract_python(
            relative_path,
            &root,
            source,
            &mut symbols,
            &mut imports,
            &mut references,
            lang,
        ),
        Language::Go => extract_go(
            relative_path,
            &root,
            source,
            &mut symbols,
            &mut imports,
            &mut references,
            lang,
        ),
        Language::TypeScript => extract_typescript(
            relative_path,
            &root,
            source,
            &mut symbols,
            &mut imports,
            &mut references,
            lang,
            true,
        ),
        Language::JavaScript => extract_javascript(
            relative_path,
            &root,
            source,
            &mut symbols,
            &mut imports,
            &mut references,
            lang,
        ),
        Language::Unknown => {}
    }

    let mut chunks = Vec::new();
    for sym in &symbols {
        let text_slice = if sym.start_byte < source.len() && sym.end_byte <= source.len() {
            &source[sym.start_byte..sym.end_byte]
        } else {
            b""
        };
        let c_hash = blake3::hash(text_slice).to_hex().to_string();
        let cid = chunk_id(relative_path, sym.start_byte, sym.end_byte, &c_hash);
        chunks.push(Chunk {
            id: cid,
            file: relative_path.to_string(),
            language: lang,
            start_line: sym.start_line,
            end_line: sym.end_line,
            start_byte: sym.start_byte,
            end_byte: sym.end_byte,
            parent_symbol: Some(sym.id.clone()),
            content_hash: c_hash,
            text_size_bytes: text_slice.len(),
        });
    }

    ParsedFile {
        file: relative_path.to_string(),
        language: lang,
        content_hash: content_hash.to_string(),
        symbols,
        references,
        imports,
        chunks,
        parse_error,
    }
}

fn language_to_ts(lang: Language) -> Option<TsLanguage> {
    match lang {
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Unknown => None,
    }
}

fn node_text<'a>(node: &tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

fn field_text(node: &tree_sitter::Node, field: &str, source: &[u8]) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| node_text(&n, source).to_string())
}

fn visibility_from_text(text: &str, lang: Language) -> Visibility {
    match lang {
        Language::Rust => {
            if text.trim_start().starts_with("pub") {
                Visibility::Public
            } else {
                Visibility::Private
            }
        }
        Language::Python => {
            if text.starts_with('_') {
                Visibility::Private
            } else {
                Visibility::Public
            }
        }
        _ => Visibility::Unknown,
    }
}

// --- Rust extraction ---
fn extract_rust(
    file: &str,
    root: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
) {
    let mut qual_stack: Vec<String> = Vec::new();
    let mut enclosing: Vec<String> = Vec::new();

    walk_rust(
        file,
        root,
        source,
        symbols,
        imports,
        references,
        lang,
        &mut qual_stack,
        &mut enclosing,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_rust(
    file: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
    qual_stack: &mut Vec<String>,
    enclosing: &mut Vec<String>,
) {
    let kind = node.kind();
    match kind {
        "function_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if qual_stack.is_empty() {
                    name.clone()
                } else {
                    format!("{}::{}", qual_stack.last().unwrap(), name)
                };
                let vis = field_text(node, "visibility", source)
                    .map(|v| visibility_from_text(&v, lang))
                    .unwrap_or(Visibility::Private);
                let start = node.start_position();
                let end = node.end_position();
                let sym_kind = if qual_stack.last().is_some_and(|p| p.contains("impl")) {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let parent = qual_stack.last().cloned();
                let id = symbol_id(file, lang, &qualified, &sym_kind, parent.as_deref());
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: sym_kind,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: vis,
                    parent: parent.clone(),
                });
                qual_stack.push(qualified.clone());
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_rust(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                enclosing.pop();
                qual_stack.pop();
                return;
            }
        }
        "struct_item" | "enum_item" | "trait_item" | "type_item" | "const_item" | "static_item"
        | "mod_item" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if qual_stack.is_empty() {
                    name.clone()
                } else {
                    format!("{}::{}", qual_stack.last().unwrap(), name)
                };
                let kind_map = match kind {
                    "struct_item" => SymbolKind::Struct,
                    "enum_item" => SymbolKind::Enum,
                    "trait_item" => SymbolKind::Trait,
                    "type_item" => SymbolKind::TypeAlias,
                    "const_item" => SymbolKind::Constant,
                    "static_item" => SymbolKind::Constant,
                    "mod_item" => SymbolKind::Module,
                    _ => SymbolKind::Unknown,
                };
                let vis = Visibility::Unknown;
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(file, lang, &qualified, &kind_map, parent.as_deref());
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: kind_map,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: vis,
                    parent: parent.clone(),
                });
                qual_stack.push(qualified);
                // struct/enum/trait not callers, so don't push enclosing
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_rust(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                qual_stack.pop();
                return;
            }
        }
        "impl_item" => {
            let mut type_name = String::new();
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_identifier" || child.kind() == "scoped_type_identifier"
                    {
                        type_name = node_text(&child, source).to_string();
                        break;
                    }
                }
            }
            if type_name.is_empty() {
                type_name = "impl".to_string();
            }
            let qualified = if qual_stack.is_empty() {
                type_name.clone()
            } else {
                format!("{}::{}", qual_stack.last().unwrap(), type_name)
            };
            qual_stack.push(qualified.clone());
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    walk_rust(
                        file, &child, source, symbols, imports, references, lang, qual_stack,
                        enclosing,
                    );
                }
            }
            qual_stack.pop();
            return;
        }
        "use_declaration" => {
            let txt = node_text(node, source).trim().to_string();
            let path = txt
                .trim_start_matches("use")
                .trim()
                .trim_end_matches(';')
                .trim()
                .to_string();
            let line = (node.start_position().row + 1) as u32;
            imports.push(Import {
                file: file.to_string(),
                import_path: path,
                alias: None,
                line,
                is_relative: false,
            });
        }
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                let callee = node_text(&func, source).to_string();
                let short = callee
                    .rsplit("::")
                    .next()
                    .unwrap_or(&callee)
                    .split('.')
                    .next_back()
                    .unwrap_or(&callee)
                    .to_string();
                let parent_id = enclosing.last().cloned();
                let line = (node.start_position().row + 1) as u32;
                references.push(Reference {
                    name: short,
                    file: file.to_string(),
                    line,
                    parent_symbol: parent_id,
                    kind: ReferenceKind::Call,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        _ => {}
    }

    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_rust(
                file, &child, source, symbols, imports, references, lang, qual_stack, enclosing,
            );
        }
    }
}

// --- Python extraction ---
fn extract_python(
    file: &str,
    root: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
) {
    let mut qual_stack: Vec<String> = Vec::new();
    let mut enclosing: Vec<String> = Vec::new();
    walk_py(
        file,
        root,
        source,
        symbols,
        imports,
        references,
        lang,
        &mut qual_stack,
        &mut enclosing,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_py(
    file: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
    qual_stack: &mut Vec<String>,
    enclosing: &mut Vec<String>,
) {
    let kind = node.kind();
    match kind {
        "function_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let vis = visibility_from_text(&name, lang);
                let start = node.start_position();
                let end = node.end_position();
                let sym_kind = if qual_stack.last().is_some() {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let parent = qual_stack.last().cloned();
                let id = symbol_id(file, lang, &qualified, &sym_kind, parent.as_deref());
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: sym_kind,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: vis,
                    parent: parent.clone(),
                });
                qual_stack.push(qualified.clone());
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_py(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                enclosing.pop();
                qual_stack.pop();
                return;
            }
        }
        "class_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Class,
                    parent.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: SymbolKind::Class,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: visibility_from_text(&name, lang),
                    parent: parent.clone(),
                });
                qual_stack.push(qualified);
                // class not a caller, don't push enclosing
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_py(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                qual_stack.pop();
                return;
            }
        }
        "import_statement" => {
            let txt = node_text(node, source).to_string();
            let line = (node.start_position().row + 1) as u32;
            let rest = txt.trim_start_matches("import").trim();
            for part in rest.split(',') {
                let p = part.trim().to_string();
                if !p.is_empty() {
                    imports.push(Import {
                        file: file.to_string(),
                        import_path: p,
                        alias: None,
                        line,
                        is_relative: false,
                    });
                }
            }
        }
        "import_from_statement" => {
            let txt = node_text(node, source).to_string();
            let line = (node.start_position().row + 1) as u32;
            let lower = txt.trim().to_string();
            imports.push(Import {
                file: file.to_string(),
                import_path: lower,
                alias: None,
                line,
                is_relative: txt.contains("from ."),
            });
        }
        "call" => {
            let func = node.child_by_field_name("function");
            if let Some(f) = func {
                let callee_full = node_text(&f, source).to_string();
                let short = callee_full
                    .split('.')
                    .next_back()
                    .unwrap_or(&callee_full)
                    .to_string();
                let line = (node.start_position().row + 1) as u32;
                let parent_id = enclosing.last().cloned();
                references.push(Reference {
                    name: short,
                    file: file.to_string(),
                    line,
                    parent_symbol: parent_id,
                    kind: ReferenceKind::Call,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_py(
                file, &child, source, symbols, imports, references, lang, qual_stack, enclosing,
            );
        }
    }
}

// --- Go extraction ---
fn extract_go(
    file: &str,
    root: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
) {
    let mut enclosing: Vec<String> = Vec::new();
    walk_go(
        file,
        root,
        source,
        symbols,
        imports,
        references,
        lang,
        &mut enclosing,
    );
}

#[allow(clippy::too_many_arguments)]
fn walk_go(
    file: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
    enclosing: &mut Vec<String>,
) {
    let kind = node.kind();
    match kind {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let receiver = node.child_by_field_name("receiver");
                let qualified = if let Some(recv) = receiver {
                    let rec_txt = node_text(&recv, source);
                    let type_name = rec_txt
                        .split_whitespace()
                        .next_back()
                        .unwrap_or("")
                        .trim_matches(|c| c == '*' || c == '(' || c == ')')
                        .to_string();
                    if !type_name.is_empty() {
                        format!("{}.{}", type_name, name)
                    } else {
                        name.clone()
                    }
                } else {
                    name.clone()
                };
                let sym_kind = if receiver.is_some() {
                    SymbolKind::Method
                } else {
                    SymbolKind::Function
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent_q = if sym_kind == SymbolKind::Method {
                    let rec_txt = node
                        .child_by_field_name("receiver")
                        .map(|r| node_text(&r, source).to_string())
                        .unwrap_or_default();
                    let tn = rec_txt
                        .split_whitespace()
                        .next_back()
                        .unwrap_or("")
                        .trim_matches(|c| c == '*' || c == '(' || c == ')')
                        .to_string();
                    if tn.is_empty() {
                        None
                    } else {
                        Some(tn)
                    }
                } else {
                    None
                };
                let id = symbol_id(file, lang, &qualified, &sym_kind, parent_q.as_deref());
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified,
                    kind: sym_kind,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: if name.chars().next().is_some_and(|c| c.is_uppercase()) {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    },
                    parent: parent_q,
                });
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_go(
                            file, &child, source, symbols, imports, references, lang, enclosing,
                        );
                    }
                }
                enclosing.pop();
                return;
            }
        }
        "method_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let receiver = node.child_by_field_name("receiver");
                let qualified = if let Some(recv) = receiver {
                    let rec_txt = node_text(&recv, source);
                    let type_name = rec_txt
                        .split_whitespace()
                        .next_back()
                        .unwrap_or("")
                        .trim_matches(|c| c == '*' || c == '(' || c == ')')
                        .to_string();
                    if !type_name.is_empty() {
                        format!("{}.{}", type_name, name)
                    } else {
                        name.clone()
                    }
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent_q = receiver.map(|r| {
                    let rt = node_text(&r, source).to_string();
                    rt.split_whitespace()
                        .next_back()
                        .unwrap_or("")
                        .trim_matches(|c| c == '*' || c == '(' || c == ')')
                        .to_string()
                });
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Method,
                    parent_q.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name,
                    qualified_name: qualified,
                    kind: SymbolKind::Method,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent: parent_q,
                });
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_go(
                            file, &child, source, symbols, imports, references, lang, enclosing,
                        );
                    }
                }
                enclosing.pop();
                return;
            }
        }
        "type_declaration" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "type_spec" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = node_text(&name_node, source).to_string();
                            let type_node = child.child_by_field_name("type");
                            let kind = type_node
                                .map(|t| match t.kind() {
                                    "struct_type" => SymbolKind::Struct,
                                    "interface_type" => SymbolKind::Interface,
                                    _ => SymbolKind::TypeAlias,
                                })
                                .unwrap_or(SymbolKind::TypeAlias);
                            let start = child.start_position();
                            let end = child.end_position();
                            let id = symbol_id(file, lang, &name, &kind, None);
                            symbols.push(Symbol {
                                id,
                                name: name.clone(),
                                qualified_name: name.clone(),
                                kind,
                                file: file.to_string(),
                                language: lang,
                                start_line: (start.row + 1) as u32,
                                end_line: (end.row + 1) as u32,
                                start_byte: child.start_byte(),
                                end_byte: child.end_byte(),
                                visibility: if name.chars().next().is_some_and(|c| c.is_uppercase())
                                {
                                    Visibility::Public
                                } else {
                                    Visibility::Private
                                },
                                parent: None,
                            });
                        }
                    }
                }
            }
        }
        "import_declaration" => {
            let txt = node_text(node, source).to_string();
            let line = (node.start_position().row + 1) as u32;
            for caps in regex::Regex::new(r#""([^"]+)""#)
                .unwrap()
                .captures_iter(&txt)
            {
                if let Some(m) = caps.get(1) {
                    imports.push(Import {
                        file: file.to_string(),
                        import_path: m.as_str().to_string(),
                        alias: None,
                        line,
                        is_relative: false,
                    });
                }
            }
        }
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                let callee_full = node_text(&func, source).to_string();
                let short = callee_full
                    .split('.')
                    .next_back()
                    .unwrap_or(&callee_full)
                    .to_string();
                let line = (node.start_position().row + 1) as u32;
                let parent_id = enclosing.last().cloned();
                references.push(Reference {
                    name: short,
                    file: file.to_string(),
                    line,
                    parent_symbol: parent_id,
                    kind: ReferenceKind::Call,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_go(
                file, &child, source, symbols, imports, references, lang, enclosing,
            );
        }
    }
}

// --- TypeScript extraction ---
#[allow(clippy::too_many_arguments)]
fn extract_typescript(
    file: &str,
    root: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
    is_ts: bool,
) {
    let mut qual_stack: Vec<String> = Vec::new();
    let mut enclosing: Vec<String> = Vec::new();
    walk_ts(
        file,
        root,
        source,
        symbols,
        imports,
        references,
        lang,
        &mut qual_stack,
        &mut enclosing,
    );
    let _ = is_ts;
}

#[allow(clippy::too_many_arguments)]
fn walk_ts(
    file: &str,
    node: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
    qual_stack: &mut Vec<String>,
    enclosing: &mut Vec<String>,
) {
    let kind = node.kind();
    match kind {
        "function_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Function,
                    parent.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: SymbolKind::Function,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent,
                });
                qual_stack.push(qualified.clone());
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_ts(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                enclosing.pop();
                qual_stack.pop();
                return;
            }
        }
        "function" => {
            // anonymous function assigned to variable handled via variable_declarator
            // but also standalone function expression inside outer
            // For nested named function in JS/TS, it's function_declaration above covers, but
            // some grammars use "function" for nested.
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else if let Some(encl) = enclosing.last() {
                    // fallback: use enclosing qualified? but we have qual_stack for that
                    format!("{}::{}", encl, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Function,
                    parent.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: SymbolKind::Function,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent,
                });
                qual_stack.push(qualified.clone());
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_ts(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                enclosing.pop();
                qual_stack.pop();
                return;
            }
        }
        "class_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Class,
                    parent.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: SymbolKind::Class,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent,
                });
                qual_stack.push(qualified);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_ts(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                qual_stack.pop();
                return;
            }
        }
        "method_definition" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(
                    file,
                    lang,
                    &qualified,
                    &SymbolKind::Method,
                    parent.as_deref(),
                );
                symbols.push(Symbol {
                    id: id.clone(),
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind: SymbolKind::Method,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent,
                });
                qual_stack.push(qualified.clone());
                enclosing.push(id);
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.id() == name_node.id() {
                            continue;
                        }
                        walk_ts(
                            file, &child, source, symbols, imports, references, lang, qual_stack,
                            enclosing,
                        );
                    }
                }
                enclosing.pop();
                qual_stack.pop();
                return;
            }
        }
        "interface_declaration" | "type_alias_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                let name = node_text(&name_node, source).to_string();
                let kind = if kind == "interface_declaration" {
                    SymbolKind::Interface
                } else {
                    SymbolKind::TypeAlias
                };
                let qualified = if let Some(parent) = qual_stack.last() {
                    format!("{}.{}", parent, name)
                } else {
                    name.clone()
                };
                let start = node.start_position();
                let end = node.end_position();
                let parent = qual_stack.last().cloned();
                let id = symbol_id(file, lang, &qualified, &kind, parent.as_deref());
                symbols.push(Symbol {
                    id,
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    kind,
                    file: file.to_string(),
                    language: lang,
                    start_line: (start.row + 1) as u32,
                    end_line: (end.row + 1) as u32,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    visibility: Visibility::Unknown,
                    parent,
                });
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    if child.kind() == "variable_declarator" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = node_text(&name_node, source).to_string();
                            let value = child.child_by_field_name("value");
                            let is_func = value.is_some_and(|v| {
                                matches!(
                                    v.kind(),
                                    "arrow_function" | "function_expression" | "function"
                                )
                            });
                            if is_func {
                                if let Some(v) = value {
                                    let qualified = if let Some(parent) = qual_stack.last() {
                                        format!("{}.{}", parent, name)
                                    } else {
                                        name.clone()
                                    };
                                    let start = child.start_position();
                                    let end = child.end_position();
                                    let parent = qual_stack.last().cloned();
                                    let id = symbol_id(
                                        file,
                                        lang,
                                        &qualified,
                                        &SymbolKind::Function,
                                        parent.as_deref(),
                                    );
                                    symbols.push(Symbol {
                                        id: id.clone(),
                                        name: name.clone(),
                                        qualified_name: qualified.clone(),
                                        kind: SymbolKind::Function,
                                        file: file.to_string(),
                                        language: lang,
                                        start_line: (start.row + 1) as u32,
                                        end_line: (end.row + 1) as u32,
                                        start_byte: child.start_byte(),
                                        end_byte: child.end_byte(),
                                        visibility: Visibility::Unknown,
                                        parent,
                                    });
                                    qual_stack.push(qualified.clone());
                                    enclosing.push(id);
                                    // walk the function value
                                    walk_ts(
                                        file, &v, source, symbols, imports, references, lang,
                                        qual_stack, enclosing,
                                    );
                                    enclosing.pop();
                                    qual_stack.pop();
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        "import_statement" => {
            let txt = node_text(node, source).to_string();
            let line = (node.start_position().row + 1) as u32;
            let import_path = regex::Regex::new(r#"from\s+["']([^"']+)["']"#)
                .ok()
                .and_then(|re| re.captures(&txt))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| txt.clone());
            imports.push(Import {
                file: file.to_string(),
                import_path,
                alias: None,
                line,
                is_relative: txt.contains("\"./")
                    || txt.contains("'./")
                    || txt.contains("\"../")
                    || txt.contains("'../"),
            });
        }
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                let callee_full = node_text(&func, source).to_string();
                let short = callee_full
                    .split('.')
                    .next_back()
                    .unwrap_or(&callee_full)
                    .split('(')
                    .next()
                    .unwrap_or(&callee_full)
                    .trim()
                    .to_string();
                let line = (node.start_position().row + 1) as u32;
                let parent_id = enclosing.last().cloned();
                references.push(Reference {
                    name: short,
                    file: file.to_string(),
                    line,
                    parent_symbol: parent_id,
                    kind: ReferenceKind::Call,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            walk_ts(
                file, &child, source, symbols, imports, references, lang, qual_stack, enclosing,
            );
        }
    }
}

fn extract_javascript(
    file: &str,
    root: &tree_sitter::Node,
    source: &[u8],
    symbols: &mut Vec<Symbol>,
    imports: &mut Vec<Import>,
    references: &mut Vec<Reference>,
    lang: Language,
) {
    extract_typescript(
        file, root, source, symbols, imports, references, lang, false,
    );
}

/// Build call edges from parsed files.
pub fn build_call_edges(parsed_files: &[ParsedFile]) -> Vec<CallEdge> {
    use std::collections::HashMap;
    let mut by_name: HashMap<String, Vec<&Symbol>> = HashMap::new();
    let mut by_qualified: HashMap<String, &Symbol> = HashMap::new();
    for pf in parsed_files {
        for sym in &pf.symbols {
            by_name.entry(sym.name.clone()).or_default().push(sym);
            by_qualified.insert(sym.qualified_name.clone(), sym);
        }
    }

    let mut edges = Vec::new();
    for pf in parsed_files {
        for r in &pf.references {
            if r.kind != ReferenceKind::Call {
                continue;
            }
            let caller = r.parent_symbol.clone().unwrap_or_default();
            let candidates = by_name.get(&r.name);
            let (resolved, confidence) = match candidates {
                Some(list) if list.len() == 1 => {
                    (Some(list[0].id.clone()), CallConfidence::Resolved)
                }
                Some(list) if list.len() > 1 => {
                    let same_file: Vec<_> = list.iter().filter(|s| s.file == r.file).collect();
                    if same_file.len() == 1 {
                        (Some(same_file[0].id.clone()), CallConfidence::Probable)
                    } else {
                        (None, CallConfidence::Unresolved)
                    }
                }
                _ => (None, CallConfidence::Unresolved),
            };
            let (resolved, confidence) = if resolved.is_none() {
                if let Some(sym) = by_qualified.get(&r.name) {
                    (Some(sym.id.clone()), CallConfidence::Resolved)
                } else {
                    (resolved, confidence)
                }
            } else {
                (resolved, confidence)
            };

            edges.push(CallEdge {
                caller_symbol_id: caller,
                callee_name: r.name.clone(),
                resolved_symbol_id: resolved,
                confidence,
                file: r.file.clone(),
                line: r.line,
            });
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_python_simple() {
        let content =
            "def count_tokens(text):\n    return len(text)\n\nclass Foo:\n    def bar(self):\n        count_tokens(\"hi\")\n";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let pf = parse_file("a.py", content, &hash);
        assert_eq!(pf.language, Language::Python);
        assert!(pf.symbols.iter().any(|s| s.name == "count_tokens"));
        assert!(pf.symbols.iter().any(|s| s.name == "Foo"));
        assert!(pf
            .symbols
            .iter()
            .any(|s| s.name == "bar" && s.qualified_name == "Foo.bar"));
        assert!(pf
            .references
            .iter()
            .any(|r| r.name == "count_tokens" && r.kind == ReferenceKind::Call));
        assert_eq!(pf.chunks.len(), pf.symbols.len());
    }
    #[test]
    fn parse_rust_simple() {
        let content =
            "fn retrieve_context(query: &str) {}\nstruct ProjectIndex {}\nimpl ProjectIndex { fn discover(&self) {} }\n";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let pf = parse_file("x.rs", content, &hash);
        assert!(pf.symbols.iter().any(|s| s.name == "retrieve_context"));
        assert!(pf
            .symbols
            .iter()
            .any(|s| s.name == "ProjectIndex" && matches!(s.kind, SymbolKind::Struct)));
    }
    #[test]
    fn parse_go_simple() {
        let content = "package main\nfunc NewRouter() {}\nfunc (s *Server) Start() { NewRouter() }\ntype Config struct {}\nimport \"fmt\"\n";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let pf = parse_file("a.go", content, &hash);
        assert!(pf.symbols.iter().any(|s| s.name == "NewRouter"));
        assert!(pf
            .symbols
            .iter()
            .any(|s| s.name == "Start" && s.qualified_name == "Server.Start"));
    }
    #[test]
    fn parse_js_simple() {
        let content =
            "function foo() { bar(); }\nconst baz = () => {}\nclass Cls { method() { foo(); } }\nimport x from \"y\";\n";
        let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
        let pf = parse_file("a.js", content, &hash);
        assert!(pf.symbols.iter().any(|s| s.name == "foo"));
        assert!(pf.symbols.iter().any(|s| s.name == "Cls"));
    }
}
