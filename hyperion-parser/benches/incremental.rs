// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Benchmark for parsing after an edit vs parsing a complete source buffer.
//!
//! `incremental_parse` currently applies an edit and reparses the whole file.
//! These measurements expose that cost for typical editing scenarios.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hyperion_parser::{incremental_parse, lexer::Lexer, parser::Parser, TextEdit};
use rowan::{TextRange, TextSize};

/// A realistic medium-sized Java class (~50 lines)
const MEDIUM_CLASS: &str = r#"
package com.example.service;

import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

/**
 * User service for managing user data.
 */
public class UserService {
    private final UserRepository repository;
    private final Cache cache;
    private final Logger logger;

    public UserService(UserRepository repository, Cache cache, Logger logger) {
        this.repository = repository;
        this.cache = cache;
        this.logger = logger;
    }

    public CompletableFuture<User> getUserById(String userId) {
        logger.debug("Fetching user: {}", userId);

        // Check cache first
        User cached = cache.get(userId);
        if (cached != null) {
            return CompletableFuture.completedFuture(cached);
        }

        // Fetch from database
        return repository.findById(userId)
            .thenApply(user -> {
                cache.put(userId, user);
                logger.info("User fetched: {}", userId);
                return user;
            });
    }

    public void updateUser(User user) {
        repository.save(user);
        cache.invalidate(user.getId());
        logger.info("User updated: {}", user.getId());
    }

    public List<User> getAllUsers() {
        return repository.findAll();
    }
}
"#;

/// A large Java class (~200 lines) for stress testing
fn generate_large_class(num_methods: usize) -> String {
    let mut class = String::from(
        r#"
package com.example.large;

import java.util.*;

public class LargeClass {
    private final Map<String, Object> data = new HashMap<>();
"#,
    );

    for i in 0..num_methods {
        class.push_str(&format!(
            r#"
    public void method{}() {{
        System.out.println("Method {} called");
        data.put("key{}", "value{}");
    }}
"#,
            i, i, i, i
        ));
    }

    class.push_str("}\n");
    class
}

fn bench_full_parse_medium(c: &mut Criterion) {
    c.bench_function("full_parse_medium", |b| {
        b.iter(|| {
            let tokens = Lexer::new(black_box(MEDIUM_CLASS)).collect();
            let (tree, errors) = Parser::parse(black_box(tokens));
            black_box((tree, errors));
        });
    });
}

fn bench_incremental_parse_string_change(c: &mut Criterion) {
    // Pre-parse the class
    let tokens = Lexer::new(MEDIUM_CLASS).collect();
    let (old_tree, _) = Parser::parse(tokens);

    // Find position of "Fetching user" string to edit
    let old_str_start = MEDIUM_CLASS.find("Fetching user").unwrap();
    let old_str_end = old_str_start + "Fetching user".len();

    let edit = TextEdit::new(
        TextRange::new(
            TextSize::from(old_str_start as u32),
            TextSize::from(old_str_end as u32),
        ),
        "Loading user data".to_string(),
    );

    c.bench_function("incremental_parse_string_change", |b| {
        b.iter(|| {
            let (tree, errors) = incremental_parse(
                black_box(&old_tree),
                black_box(MEDIUM_CLASS),
                black_box(&edit),
            );
            black_box((tree, errors));
        });
    });
}

fn bench_full_parse_large(c: &mut Criterion) {
    let large_class = generate_large_class(50);

    c.bench_function("full_parse_large_50_methods", |b| {
        b.iter(|| {
            let tokens = Lexer::new(black_box(&large_class)).collect();
            let (tree, errors) = Parser::parse(black_box(tokens));
            black_box((tree, errors));
        });
    });
}

fn bench_incremental_parse_large(c: &mut Criterion) {
    let large_class = generate_large_class(50);

    // Pre-parse
    let tokens = Lexer::new(&large_class).collect();
    let (old_tree, _) = Parser::parse(tokens);

    // Edit a string in the middle
    let old_str_start = large_class.find("Method 25").unwrap();
    let old_str_end = old_str_start + "Method 25".len();

    let edit = TextEdit::new(
        TextRange::new(
            TextSize::from(old_str_start as u32),
            TextSize::from(old_str_end as u32),
        ),
        "Updated method 25".to_string(),
    );

    c.bench_function("incremental_parse_large_50_methods", |b| {
        b.iter(|| {
            let (tree, errors) = incremental_parse(
                black_box(&old_tree),
                black_box(&large_class),
                black_box(&edit),
            );
            black_box((tree, errors));
        });
    });
}

criterion_group!(
    benches,
    bench_full_parse_medium,
    bench_incremental_parse_string_change,
    bench_full_parse_large,
    bench_incremental_parse_large
);
criterion_main!(benches);
