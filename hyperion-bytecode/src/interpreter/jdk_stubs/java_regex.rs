// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util.regex package stub implementations
//!
//! Regular expression support: Pattern, Matcher, MatchResult

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.util.regex.MatchResult interface stub
pub fn create_match_result_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "start:()I".to_string(),
        StubMethodDescriptor::instance_method("start", "()I"),
    );
    methods.insert(
        "start:(I)I".to_string(),
        StubMethodDescriptor::instance_method("start", "(I)I"),
    );
    methods.insert(
        "end:()I".to_string(),
        StubMethodDescriptor::instance_method("end", "()I"),
    );
    methods.insert(
        "end:(I)I".to_string(),
        StubMethodDescriptor::instance_method("end", "(I)I"),
    );
    methods.insert(
        "group:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("group", "()Ljava/lang/String;"),
    );
    methods.insert(
        "group:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("group", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "groupCount:()I".to_string(),
        StubMethodDescriptor::instance_method("groupCount", "()I"),
    );

    StubClass {
        name: "java/util/regex/MatchResult".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.regex.Pattern class stub
pub fn create_pattern_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "compile:(Ljava/lang/String;)Ljava/util/regex/Pattern;".to_string(),
        StubMethodDescriptor::static_method(
            "compile",
            "(Ljava/lang/String;)Ljava/util/regex/Pattern;",
        ),
    );
    methods.insert(
        "compile:(Ljava/lang/String;I)Ljava/util/regex/Pattern;".to_string(),
        StubMethodDescriptor::static_method(
            "compile",
            "(Ljava/lang/String;I)Ljava/util/regex/Pattern;",
        ),
    );

    // Static utility methods
    methods.insert(
        "matches:(Ljava/lang/String;Ljava/lang/CharSequence;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "matches",
            "(Ljava/lang/String;Ljava/lang/CharSequence;)Z",
        ),
    );
    methods.insert(
        "quote:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("quote", "(Ljava/lang/String;)Ljava/lang/String;"),
    );

    // Instance methods
    methods.insert(
        "pattern:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("pattern", "()Ljava/lang/String;"),
    );
    methods.insert(
        "flags:()I".to_string(),
        StubMethodDescriptor::instance_method("flags", "()I"),
    );
    methods.insert(
        "matcher:(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "matcher",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        ),
    );
    methods.insert(
        "split:(Ljava/lang/CharSequence;)[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "split",
            "(Ljava/lang/CharSequence;)[Ljava/lang/String;",
        ),
    );
    methods.insert(
        "split:(Ljava/lang/CharSequence;I)[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "split",
            "(Ljava/lang/CharSequence;I)[Ljava/lang/String;",
        ),
    );

    // Java 8+ methods
    methods.insert(
        "asPredicate:()Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::instance_method("asPredicate", "()Ljava/util/function/Predicate;"),
    );
    methods.insert(
        "splitAsStream:(Ljava/lang/CharSequence;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method(
            "splitAsStream",
            "(Ljava/lang/CharSequence;)Ljava/util/stream/Stream;",
        ),
    );

    // Java 11+ methods
    methods.insert(
        "asMatchPredicate:()Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "asMatchPredicate",
            "()Ljava/util/function/Predicate;",
        ),
    );

    // Object methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/regex/Pattern".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.util.regex.Matcher class stub
pub fn create_matcher_class() -> StubClass {
    let mut methods = HashMap::new();

    // Pattern access
    methods.insert(
        "pattern:()Ljava/util/regex/Pattern;".to_string(),
        StubMethodDescriptor::instance_method("pattern", "()Ljava/util/regex/Pattern;"),
    );

    // Match operations
    methods.insert(
        "matches:()Z".to_string(),
        StubMethodDescriptor::instance_method("matches", "()Z"),
    );
    methods.insert(
        "find:()Z".to_string(),
        StubMethodDescriptor::instance_method("find", "()Z"),
    );
    methods.insert(
        "find:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("find", "(I)Z"),
    );
    methods.insert(
        "lookingAt:()Z".to_string(),
        StubMethodDescriptor::instance_method("lookingAt", "()Z"),
    );

    // Group operations (from MatchResult interface)
    methods.insert(
        "group:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("group", "()Ljava/lang/String;"),
    );
    methods.insert(
        "group:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("group", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "group:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("group", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "groupCount:()I".to_string(),
        StubMethodDescriptor::instance_method("groupCount", "()I"),
    );

    // Position operations
    methods.insert(
        "start:()I".to_string(),
        StubMethodDescriptor::instance_method("start", "()I"),
    );
    methods.insert(
        "start:(I)I".to_string(),
        StubMethodDescriptor::instance_method("start", "(I)I"),
    );
    methods.insert(
        "start:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("start", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "end:()I".to_string(),
        StubMethodDescriptor::instance_method("end", "()I"),
    );
    methods.insert(
        "end:(I)I".to_string(),
        StubMethodDescriptor::instance_method("end", "(I)I"),
    );
    methods.insert(
        "end:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("end", "(Ljava/lang/String;)I"),
    );

    // Reset operations
    methods.insert(
        "reset:()Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method("reset", "()Ljava/util/regex/Matcher;"),
    );
    methods.insert(
        "reset:(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "reset",
            "(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;",
        ),
    );

    // Replacement operations
    methods.insert(
        "replaceAll:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceAll",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "replaceFirst:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceFirst",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "appendReplacement:(Ljava/lang/StringBuffer;Ljava/lang/String;)Ljava/util/regex/Matcher;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "appendReplacement",
            "(Ljava/lang/StringBuffer;Ljava/lang/String;)Ljava/util/regex/Matcher;",
        ),
    );
    methods.insert(
        "appendTail:(Ljava/lang/StringBuffer;)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "appendTail",
            "(Ljava/lang/StringBuffer;)Ljava/lang/StringBuffer;",
        ),
    );

    // Java 9+ StringBuilder variants
    methods.insert(
        "appendReplacement:(Ljava/lang/StringBuilder;Ljava/lang/String;)Ljava/util/regex/Matcher;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "appendReplacement",
            "(Ljava/lang/StringBuilder;Ljava/lang/String;)Ljava/util/regex/Matcher;",
        ),
    );
    methods.insert(
        "appendTail:(Ljava/lang/StringBuilder;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "appendTail",
            "(Ljava/lang/StringBuilder;)Ljava/lang/StringBuilder;",
        ),
    );

    // Java 9+ functional replacement
    methods.insert(
        "replaceAll:(Ljava/util/function/Function;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceAll",
            "(Ljava/util/function/Function;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "replaceFirst:(Ljava/util/function/Function;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceFirst",
            "(Ljava/util/function/Function;)Ljava/lang/String;",
        ),
    );

    // Region operations
    methods.insert(
        "region:(II)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method("region", "(II)Ljava/util/regex/Matcher;"),
    );
    methods.insert(
        "regionStart:()I".to_string(),
        StubMethodDescriptor::instance_method("regionStart", "()I"),
    );
    methods.insert(
        "regionEnd:()I".to_string(),
        StubMethodDescriptor::instance_method("regionEnd", "()I"),
    );
    methods.insert(
        "hasTransparentBounds:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasTransparentBounds", "()Z"),
    );
    methods.insert(
        "useTransparentBounds:(Z)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "useTransparentBounds",
            "(Z)Ljava/util/regex/Matcher;",
        ),
    );
    methods.insert(
        "hasAnchoringBounds:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasAnchoringBounds", "()Z"),
    );
    methods.insert(
        "useAnchoringBounds:(Z)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method("useAnchoringBounds", "(Z)Ljava/util/regex/Matcher;"),
    );

    // Query operations
    methods.insert(
        "hitEnd:()Z".to_string(),
        StubMethodDescriptor::instance_method("hitEnd", "()Z"),
    );
    methods.insert(
        "requireEnd:()Z".to_string(),
        StubMethodDescriptor::instance_method("requireEnd", "()Z"),
    );

    // Result conversion
    methods.insert(
        "toMatchResult:()Ljava/util/regex/MatchResult;".to_string(),
        StubMethodDescriptor::instance_method("toMatchResult", "()Ljava/util/regex/MatchResult;"),
    );

    // Java 9+ methods
    methods.insert(
        "results:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("results", "()Ljava/util/stream/Stream;"),
    );

    // Pattern switching
    methods.insert(
        "usePattern:(Ljava/util/regex/Pattern;)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "usePattern",
            "(Ljava/util/regex/Pattern;)Ljava/util/regex/Matcher;",
        ),
    );

    // Object methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/regex/Matcher".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/util/regex/MatchResult".to_string()],
        methods,
    }
}

/// Create java.util.regex.PatternSyntaxException class stub
pub fn create_pattern_syntax_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/String;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/String;I)V"),
    );

    // Instance methods
    methods.insert(
        "getDescription:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getDescription", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getIndex:()I".to_string(),
        StubMethodDescriptor::instance_method("getIndex", "()I"),
    );
    methods.insert(
        "getPattern:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getPattern", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMessage", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/util/regex/PatternSyntaxException".to_string(),
        superclass: Some("java/lang/IllegalArgumentException".to_string()),
        interfaces: vec![],
        methods,
    }
}
