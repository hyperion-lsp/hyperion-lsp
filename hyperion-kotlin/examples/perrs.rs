// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use std::collections::BTreeMap;
fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().unwrap() != ".git" {
                walk(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "kt") {
            out.push(p);
        }
    }
}
fn main() {
    let mut files = vec![];
    for a in std::env::args().skip(1) {
        let p = std::path::Path::new(&a);
        if p.is_file() {
            files.push(p.to_path_buf());
        } else {
            walk(p, &mut files);
        }
    }
    let mut kinds: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut total = 0;
    let mut bad = 0;
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        let t = hyperion_kotlin::parse(&src);
        if !t.errors.is_empty() {
            bad += 1;
        }
        // Structural check: anything at file level that is not a declaration means
        // the parser lost the plot without reporting an error.
        {
            use hyperion_kotlin::SyntaxKind as K;
            let root = hyperion_kotlin::SyntaxNode::new_root(t.green_node.clone());
            for el in root.children_with_tokens() {
                let stray = match &el {
                    rowan::NodeOrToken::Node(n) => !matches!(
                        n.kind(),
                        K::PACKAGE_DECL
                            | K::IMPORT_DECL
                            | K::MODIFIERS
                            | K::CLASS_DECL
                            | K::INTERFACE_DECL
                            | K::OBJECT_DECL
                            | K::FUNCTION_DECL
                            | K::PROPERTY_DECL
                            | K::TYPEALIAS_DECL
                            | K::ANNOTATION
                    ),
                    rowan::NodeOrToken::Token(tok) => {
                        !tok.kind().is_trivia() && tok.kind() != K::SEMICOLON
                    }
                };
                if stray {
                    let off = (u32::from(el.text_range().start()) as usize).min(src.len());
                    let line = src[..off].matches('\n').count() + 1;
                    let ent = kinds
                        .entry(format!("STRAY {:?}", el.kind()))
                        .or_insert((0, format!("{}:{}", f.display(), line)));
                    ent.0 += 1;
                    total += 1;
                }
            }
        }
        // Loose tokens inside class bodies and ERROR nodes are silent misparses too.
        {
            use hyperion_kotlin::SyntaxKind as K;
            let root = hyperion_kotlin::SyntaxNode::new_root(t.green_node.clone());
            for node in root.descendants() {
                let mut note = |what: String, at: usize| {
                    let line = src[..at.min(src.len())].matches('\n').count() + 1;
                    let ent = kinds
                        .entry(what)
                        .or_insert((0, format!("{}:{}", f.display(), line)));
                    ent.0 += 1;
                    total += 1;
                };
                if node.kind() == K::ERROR {
                    note(
                        "ERROR node".to_string(),
                        u32::from(node.text_range().start()) as usize,
                    );
                }
                if node.kind() == K::CLASS_BODY {
                    for token in node
                        .children_with_tokens()
                        .filter_map(rowan::NodeOrToken::into_token)
                    {
                        if !token.kind().is_trivia()
                            && !matches!(
                                token.kind(),
                                K::L_BRACE | K::R_BRACE | K::SEMICOLON | K::COMMA
                            )
                        {
                            note(
                                format!("STRAY-IN-BODY {:?}", token.kind()),
                                u32::from(token.text_range().start()) as usize,
                            );
                        }
                    }
                }
            }
        }
        for e in &t.errors {
            total += 1;
            let key: String = e.message.chars().take(60).collect();
            let line = src[..(u32::from(e.range.start()) as usize).min(src.len())]
                .matches('\n')
                .count()
                + 1;
            let ent = kinds
                .entry(key)
                .or_insert((0, format!("{}:{}", f.display(), line)));
            ent.0 += 1;
        }
    }
    println!(
        "files {} with errors {} total errors {}",
        files.len(),
        bad,
        total
    );
    let mut v: Vec<_> = kinds.into_iter().collect();
    v.sort_by_key(|item| std::cmp::Reverse(item.1 .0));
    for (k, (n, ex)) in v.iter().take(40) {
        println!("{n:5} {k}   [{ex}]");
    }
}
