//! Public sets are the only ordering contract between top-level features.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct Token {
    text: String,
    line: usize,
}

/// Tokenize code, ignoring comments and literals while retaining source lines.
fn tokens(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let (mut at, mut line) = (0, 1);
    let mut out = Vec::new();
    while at < bytes.len() {
        let start = at;
        if bytes[at].is_ascii_whitespace() {
            line += usize::from(bytes[at] == b'\n');
            at += 1;
            continue;
        }
        if bytes[at..].starts_with(b"//") {
            while at < bytes.len() && bytes[at] != b'\n' { at += 1; }
            continue;
        }
        if bytes[at..].starts_with(b"/*") {
            let mut depth = 1;
            at += 2;
            while at < bytes.len() && depth > 0 {
                if bytes[at..].starts_with(b"/*") { depth += 1; at += 2; }
                else if bytes[at..].starts_with(b"*/") { depth -= 1; at += 2; }
                else { line += usize::from(bytes[at] == b'\n'); at += 1; }
            }
            continue;
        }
        // Raw string literals (also reached after the b in br#"..."#).
        if bytes[at] == b'r' {
            let mut quote = at + 1;
            while quote < bytes.len() && bytes[quote] == b'#' { quote += 1; }
            if quote < bytes.len() && bytes[quote] == b'"' {
                let hashes = quote - at - 1;
                at = quote + 1;
                while at < bytes.len() {
                    if bytes[at] == b'"' && bytes.get(at + 1..at + 1 + hashes).is_some_and(|s| s.iter().all(|b| *b == b'#')) {
                        at += 1 + hashes;
                        break;
                    }
                    line += usize::from(bytes[at] == b'\n');
                    at += 1;
                }
                continue;
            }
        }
        // Apostrophes followed by names are lifetimes unless a closing quote
        // follows the character. Escaped and Unicode characters also work.
        let character = bytes[at] == b'\'' && {
            let mut end = at + 1;
            if bytes.get(end) == Some(&b'\\') { end += 2; }
            else if let Some(c) = source.get(end..).and_then(|s| s.chars().next()) { end += c.len_utf8(); }
            bytes.get(end) == Some(&b'\'')
        };
        if bytes[at] == b'"' || character {
            let quote = bytes[at];
            at += 1;
            while at < bytes.len() {
                if bytes[at] == b'\\' { at = (at + 2).min(bytes.len()); }
                else if bytes[at] == quote { at += 1; break; }
                else { line += usize::from(bytes[at] == b'\n'); at += 1; }
            }
            continue;
        }
        if bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_' {
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') { at += 1; }
        } else if bytes[at..].starts_with(b"::") { at += 2; }
        else { at += source[at..].chars().next().unwrap().len_utf8(); }
        out.push(Token { text: source[start..at].to_string(), line });
    }
    out
}

fn matching(tokens: &[Token], open: usize, left: &str, right: &str) -> usize {
    let mut depth = 0;
    for (i, token) in tokens.iter().enumerate().skip(open) {
        if token.text == left { depth += 1; }
        if token.text == right {
            depth -= 1;
            if depth == 0 { return i; }
        }
    }
    panic!("unclosed {left} on line {}", tokens[open].line);
}

/// Exclude inline #[cfg(test)] items, not just files named tests.rs.
fn production_tokens(source: &str) -> Vec<Token> {
    let all = tokens(source);
    let mut out = Vec::new();
    let mut at = 0;
    while at < all.len() {
        if all[at].text == "#" && all.get(at + 1).is_some_and(|t| t.text == "[") {
            let end = matching(&all, at + 1, "[", "]");
            let attribute: String = all[at + 2..end].iter().map(|t| t.text.as_str()).collect();
            if attribute == "cfg(test)" {
                let mut body = end + 1;
                while body < all.len() && all[body].text != "{" && all[body].text != ";" { body += 1; }
                at = if body == all.len() { body } else if all[body].text == ";" { body + 1 } else { matching(&all, body, "{", "}") + 1 };
                continue;
            }
        }
        out.push(Token { text: all[at].text.clone(), line: all[at].line });
        at += 1;
    }
    out
}

fn identifier(text: &str) -> bool {
    text.as_bytes().first().is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
}
fn function_segment(text: &str) -> bool {
    identifier(text) && text.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Each path in an ordering argument, including nested tuples and newlines.
fn edges(tokens: &[Token]) -> Vec<(usize, usize, Vec<String>)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 < tokens.len() {
        if tokens[at].text == "." && matches!(tokens[at + 1].text.as_str(), "after" | "before") && tokens[at + 2].text == "(" {
            let end = matching(tokens, at + 2, "(", ")");
            let mut cursor = at + 3;
            while cursor < end {
                if identifier(&tokens[cursor].text) {
                    let index = cursor;
                    let line = tokens[cursor].line;
                    let mut path = vec![tokens[cursor].text.clone()];
                    cursor += 1;
                    while cursor + 1 < end && tokens[cursor].text == "::" && identifier(&tokens[cursor + 1].text) {
                        path.push(tokens[cursor + 1].text.clone());
                        cursor += 2;
                    }
                    if path.iter().all(|p| function_segment(p)) { out.push((index, line, path)); }
                } else { cursor += 1; }
            }
            at = end + 1;
        } else { at += 1; }
    }
    out
}

fn module(file: &Path) -> Vec<String> {
    let mut parts: Vec<String> = file.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let stem = file.file_stem().unwrap().to_string_lossy().into_owned();
    parts.pop();
    if !matches!(stem.as_str(), "mod" | "lib" | "main") { parts.push(stem); }
    parts
}

fn absolute(path: &[String], caller: &[String], roots: &BTreeSet<String>) -> Vec<String> {
    let mut base = caller.to_vec();
    let mut at = 0;
    if path.first().is_some_and(|p| p == "crate") { base.clear(); at = 1; }
    else if path.first().is_some_and(|p| p == "self") { at = 1; }
    else if path.first().is_some_and(|p| roots.contains(p)) { base.clear(); }
    while path.get(at).is_some_and(|p| p == "super") { base.pop(); at += 1; }
    base.extend_from_slice(&path[at..]);
    base
}

/// Expand use trees, including nested braces and aliases, into visible names.
fn use_tree(tokens: &[Token], cursor: &mut usize, prefix: &[String], imports: &mut BTreeMap<String, Vec<String>>) {
    let mut path = prefix.to_vec();
    while *cursor < tokens.len() && identifier(&tokens[*cursor].text) && tokens[*cursor].text != "as" {
        path.push(tokens[*cursor].text.clone());
        *cursor += 1;
        if tokens.get(*cursor).is_some_and(|t| t.text == "::") { *cursor += 1; }
        else { break; }
    }
    if tokens.get(*cursor).is_some_and(|t| t.text == "{") {
        *cursor += 1;
        while *cursor < tokens.len() && tokens[*cursor].text != "}" {
            let before = *cursor;
            use_tree(tokens, cursor, &path, imports);
            if *cursor == before || tokens.get(*cursor).is_some_and(|t| t.text == ",") { *cursor += 1; }
        }
        *cursor += 1;
        return;
    }
    if tokens.get(*cursor).is_some_and(|t| t.text == "*") {
        imports.insert(format!("*{}", path.join("::")), path);
        *cursor += 1;
        return;
    }
    if path.last().is_some_and(|p| p == "self") { path.pop(); }
    let alias = if tokens.get(*cursor).is_some_and(|t| t.text == "as") {
        *cursor += 1;
        let name = tokens.get(*cursor).map(|t| t.text.clone());
        *cursor += 1;
        name
    } else { path.last().cloned() };
    if let Some(alias) = alias { imports.insert(alias, path); }
}

/// Scope at each token. Import trees use braces too, but their use keyword is
/// still in the surrounding code scope and use_tree consumes the whole tree.
fn scopes(tokens: &[Token]) -> Vec<Vec<usize>> {
    let mut stack = Vec::new();
    tokens.iter().enumerate().map(|(index, token)| {
        if token.text == "}" { stack.pop(); }
        let scope = stack.clone();
        if token.text == "{" { stack.push(index); }
        scope
    }).collect()
}

fn imports_in_scope(tokens: &[Token], caller: &[String], roots: &BTreeSet<String>, target: &[usize]) -> BTreeMap<String, Vec<String>> {
    let mut imports = BTreeMap::new();
    let scopes = scopes(tokens);
    // Visit outer scopes first so a nearer explicit import shadows them.
    for depth in 0..=target.len() {
        for (at, token) in tokens.iter().enumerate() {
            if scopes[at] != target[..depth] { continue; }
            if token.text == "mod" && tokens.get(at + 1).is_some_and(|t| identifier(&t.text)) {
                let name = tokens[at + 1].text.clone();
                imports.insert(name.clone(), vec!["self".to_string(), name]);
            }
            if token.text == "use" {
                let mut cursor = at + 1;
                use_tree(tokens, &mut cursor, &[], &mut imports);
            }
        }
    }
    imports.into_iter().map(|(alias, path)| (alias, absolute(&path, caller, roots))).collect()
}

fn imports(tokens: &[Token], caller: &[String], roots: &BTreeSet<String>) -> BTreeMap<String, Vec<String>> {
    imports_in_scope(tokens, caller, roots, &[])
}

type Symbols = BTreeMap<Vec<String>, BTreeMap<String, Vec<String>>>;

/// Resolve glob imports through other source modules, including root reexports.
/// Local declarations and explicit imports take precedence over glob names.
fn symbol_tables(sources: &[(Vec<String>, String)], roots: &BTreeSet<String>) -> Symbols {
    let mut tables = Symbols::new();
    for (caller, source) in sources {
        let tokens = production_tokens(source);
        let mut names = imports(&tokens, caller, roots);
        let scopes = scopes(&tokens);
        for (at, pair) in tokens.windows(2).enumerate() {
            if scopes[at].is_empty() && matches!(pair[0].text.as_str(), "fn" | "mod" | "struct" | "enum" | "type" | "const" | "static") && identifier(&pair[1].text) {
                let mut path = caller.clone();
                path.push(pair[1].text.clone());
                names.insert(pair[1].text.clone(), path);
            }
        }
        tables.insert(caller.clone(), names);
    }
    loop {
        let before = tables.clone();
        for names in tables.values_mut() {
            let globs: Vec<Vec<String>> = names.iter().filter(|(name, _)| name.starts_with('*')).map(|(_, path)| path.clone()).collect();
            for target in globs {
                if let Some(exports) = before.get(&target) {
                    for (name, path) in exports.iter().filter(|(name, _)| !name.starts_with('*')) {
                        names.entry(name.clone()).or_insert_with(|| path.clone());
                    }
                }
            }
        }
        if tables == before { return tables; }
    }
}

fn violations_with_symbols(source: &str, file: &Path, roots: &BTreeSet<String>, tables: &Symbols) -> Vec<String> {
    let caller = module(file);
    let tokens = production_tokens(source);
    let token_scopes = scopes(&tokens);
    edges(&tokens).into_iter().filter_map(|(index, line, path)| {
        let mut names = tables.get(&caller).cloned().unwrap_or_default();
        names.extend(imports_in_scope(&tokens, &caller, roots, &token_scopes[index]));
        let globs: Vec<Vec<String>> = names.iter().filter(|(name, _)| name.starts_with('*')).map(|(_, path)| path.clone()).collect();
        for target in globs {
            if let Some(exports) = tables.get(&target) {
                for (name, path) in exports.iter().filter(|(name, _)| !name.starts_with('*')) {
                    names.entry(name.clone()).or_insert_with(|| path.clone());
                }
            }
        }
        let resolved = if let Some(import) = names.get(&path[0]) {
            let mut resolved = import.clone();
            resolved.extend_from_slice(&path[1..]);
            resolved
        } else { absolute(&path, &caller, roots) };
        (resolved.first() != caller.first()).then(|| format!("{}:{line}: {} resolves to {}; order against a public SystemSet", file.display(), path.join("::"), resolved.join("::")))
    }).collect()
}

fn violations(source: &str, file: &Path, roots: &BTreeSet<String>) -> Vec<String> {
    violations_with_symbols(source, file, roots, &Symbols::new())
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "tests") { continue; }
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let name = path.file_stem().unwrap().to_string_lossy();
            if name == "tests" || name.ends_with("_tests") || name == "ordering_guard" { continue; }
            out.push(path);
        }
    }
}

#[test]
fn features_order_only_against_public_sets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    // A top-level foo.rs and foo/** are one feature (notably builder).
    let roots = files.iter().filter_map(|f| module(f.strip_prefix(&root).unwrap()).first().cloned()).collect();
    let contents: Vec<(PathBuf, String)> = files.into_iter().map(|file| {
        let source = std::fs::read_to_string(&file).unwrap();
        (file.strip_prefix(&root).unwrap().to_path_buf(), source)
    }).collect();
    let modules: Vec<(Vec<String>, String)> = contents.iter().filter(|(file, _)| file != Path::new("main.rs")).map(|(file, source)| (module(file), source.clone())).collect();
    let tables = symbol_tables(&modules, &roots);
    let mut offenders = Vec::new();
    for (file, source) in contents {
        offenders.extend(violations_with_symbols(&source, &file, &roots, &tables));
    }
    // Empty allowlist: every cross-feature ordering contract uses a public set.
    assert!(offenders.is_empty(), "cross-feature private ordering edges:\n{}", offenders.join("\n"));
}

#[test]
fn guard_handles_paths_tuples_aliases_and_test_modules() {
    let roots = ["cad", "app", "robot", "builder"].into_iter().map(str::to_string).collect();
    let source = r#"
        use crate::{app::actions as rest, robot::apply_frames as frames};
        fn build() { sys.after((
            rest::serve, crate::robot::apply_frames,
            frames, super::local::keys, self::keys,
            CadSet::SyncResults,
        )); }
        #[cfg(test)] mod tests { fn test() { sys.after(crate::app::actions::serve); } }
        // sys.after(crate::app::actions::serve)
        const TEXT: &str = "sys.after(crate::app::actions::serve)";
    "#;
    let hits = violations(source, Path::new("cad/panel.rs"), &roots);
    assert_eq!(hits.len(), 3, "{hits:?}");
    assert!(hits.iter().all(|h| h.contains("app::actions::serve") || h.contains("robot::apply_frames")));
    assert!(violations("mod app; sys.after(app::keys)", Path::new("cad/mod.rs"), &roots).is_empty());
    assert!(violations("sys.before(super::actions::apply)", Path::new("builder/keys.rs"), &roots).is_empty());
    assert!(violations("sys.after(builder::keys::keys)", Path::new("builder.rs"), &roots).is_empty());
}

#[test]
fn guard_resolves_bare_functions_through_glob_reexports() {
    let roots = ["builder", "inspect_view"].into_iter().map(str::to_string).collect();
    let sources = vec![
        (vec![], "pub(crate) use inspect_view::{update_parts, SpatialScene};".to_string()),
        (vec!["builder".to_string()], "use super::*; fn sync() {}".to_string()),
    ];
    let tables = symbol_tables(&sources, &roots);
    let hits = violations_with_symbols("use super::*; sys.before(update_parts).after(sync)", Path::new("builder.rs"), &roots, &tables);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].contains("inspect_view::update_parts"));
}

#[test]
fn guard_keeps_imports_in_their_lexical_scope() {
    let roots = ["builder", "inspect_view"].into_iter().map(str::to_string).collect();
    let sources = vec![
        (vec![], "pub(crate) use inspect_view::update_parts;".to_string()),
        (vec!["builder".to_string()], "use super::*; fn sync() {}".to_string()),
    ];
    let tables = symbol_tables(&sources, &roots);
    let source = "use super::*; fn sibling() { use self::sync as update_parts; sys.after(update_parts); } fn build() { sys.after(update_parts); }";
    let hits = violations_with_symbols(source, Path::new("builder.rs"), &roots, &tables);
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].contains("inspect_view::update_parts"));
}
