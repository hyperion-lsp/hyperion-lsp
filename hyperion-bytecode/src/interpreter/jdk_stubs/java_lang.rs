// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.lang package stub implementations
//!
//! Core Java language classes: Object, String, StringBuilder, primitive wrappers, etc.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.lang.Object class stub
pub fn create_object_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "getClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "notify:()V".to_string(),
        StubMethodDescriptor::instance_method("notify", "()V"),
    );
    methods.insert(
        "notifyAll:()V".to_string(),
        StubMethodDescriptor::instance_method("notifyAll", "()V"),
    );
    methods.insert(
        "wait:()V".to_string(),
        StubMethodDescriptor::instance_method("wait", "()V"),
    );
    methods.insert(
        "wait:(J)V".to_string(),
        StubMethodDescriptor::instance_method("wait", "(J)V"),
    );
    methods.insert(
        "wait:(JI)V".to_string(),
        StubMethodDescriptor::instance_method("wait", "(JI)V"),
    );

    StubClass {
        name: "java/lang/Object".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create the structural java.lang.Enum base class used by javac-generated
/// enum subclasses. Enum behavior itself is implemented by VM intrinsics.
pub fn create_enum_class() -> StubClass {
    let mut methods = HashMap::new();
    methods.insert(
        "<init>:(Ljava/lang/String;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;I)V"),
    );
    methods.insert(
        "name:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("name", "()Ljava/lang/String;"),
    );
    methods.insert(
        "ordinal:()I".to_string(),
        StubMethodDescriptor::instance_method("ordinal", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Enum;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Enum;)I"),
    );
    methods.insert(
        "getDeclaringClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getDeclaringClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/Class;Ljava/lang/String;)Ljava/lang/Enum;",
        ),
    );

    StubClass {
        name: "java/lang/Enum".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/lang/Comparable".to_string(),
            "java/io/Serializable".to_string(),
        ],
        methods,
    }
}

/// Create the structural java.lang.Record base class used by javac-generated
/// record classes. Component accessors and object methods live on each record.
pub fn create_record_class() -> StubClass {
    let mut methods = HashMap::new();
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/Record".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.String class stub
pub fn create_string_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:([C)V".to_string(),
        StubMethodDescriptor::constructor("([C)V"),
    );
    methods.insert(
        "<init>:([CII)V".to_string(),
        StubMethodDescriptor::constructor("([CII)V"),
    );
    methods.insert(
        "<init>:([B)V".to_string(),
        StubMethodDescriptor::constructor("([B)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/StringBuilder;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/StringBuilder;)V"),
    );

    // Instance methods
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "charAt:(I)C".to_string(),
        StubMethodDescriptor::instance_method("charAt", "(I)C"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "substring:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("substring", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "substring:(II)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("substring", "(II)Ljava/lang/String;"),
    );
    methods.insert(
        "concat:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("concat", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "contains:(Ljava/lang/CharSequence;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/CharSequence;)Z"),
    );
    methods.insert(
        "startsWith:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("startsWith", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "startsWith:(Ljava/lang/String;I)Z".to_string(),
        StubMethodDescriptor::instance_method("startsWith", "(Ljava/lang/String;I)Z"),
    );
    methods.insert(
        "endsWith:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("endsWith", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "indexOf:(I)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(I)I"),
    );
    methods.insert(
        "indexOf:(II)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(II)I"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/String;I)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/String;I)I"),
    );
    methods.insert(
        "lastIndexOf:(I)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(I)I"),
    );
    methods.insert(
        "lastIndexOf:(II)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(II)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/String;I)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/String;I)I"),
    );
    methods.insert(
        "toLowerCase:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toLowerCase", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toUpperCase:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toUpperCase", "()Ljava/lang/String;"),
    );
    methods.insert(
        "trim:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("trim", "()Ljava/lang/String;"),
    );
    methods.insert(
        "strip:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("strip", "()Ljava/lang/String;"),
    );
    methods.insert(
        "replace:(CC)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("replace", "(CC)Ljava/lang/String;"),
    );
    methods.insert(
        "replace:(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "replaceAll:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceAll",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "replaceFirst:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "replaceFirst",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "split:(Ljava/lang/String;)[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("split", "(Ljava/lang/String;)[Ljava/lang/String;"),
    );
    methods.insert(
        "split:(Ljava/lang/String;I)[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("split", "(Ljava/lang/String;I)[Ljava/lang/String;"),
    );
    methods.insert(
        "matches:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("matches", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "toCharArray:()[C".to_string(),
        StubMethodDescriptor::instance_method("toCharArray", "()[C"),
    );
    methods.insert(
        "getBytes:()[B".to_string(),
        StubMethodDescriptor::instance_method("getBytes", "()[B"),
    );
    methods.insert(
        "getBytes:(Ljava/lang/String;)[B".to_string(),
        StubMethodDescriptor::instance_method("getBytes", "(Ljava/lang/String;)[B"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "equalsIgnoreCase:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("equalsIgnoreCase", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "compareToIgnoreCase:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("compareToIgnoreCase", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "intern:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("intern", "()Ljava/lang/String;"),
    );
    methods.insert(
        "isBlank:()Z".to_string(),
        StubMethodDescriptor::instance_method("isBlank", "()Z"),
    );
    methods.insert(
        "lines:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("lines", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "repeat:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("repeat", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "formatted:([Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "formatted",
            "([Ljava/lang/Object;)Ljava/lang/String;",
        ),
    );

    // Static methods
    methods.insert(
        "valueOf:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(J)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(J)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(F)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(F)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(D)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(D)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(Z)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Z)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(C)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(C)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/Object;)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:([C)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "([C)Ljava/lang/String;"),
    );
    methods.insert(
        "valueOf:([CII)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "([CII)Ljava/lang/String;"),
    );
    methods.insert(
        "format:(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "join:(Ljava/lang/CharSequence;[Ljava/lang/CharSequence;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "join",
            "(Ljava/lang/CharSequence;[Ljava/lang/CharSequence;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "join:(Ljava/lang/CharSequence;Ljava/lang/Iterable;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "join",
            "(Ljava/lang/CharSequence;Ljava/lang/Iterable;)Ljava/lang/String;",
        ),
    );

    StubClass {
        name: "java/lang/String".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
            "java/lang/CharSequence".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.StringBuilder class stub
pub fn create_string_builder_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/CharSequence;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/CharSequence;)V"),
    );

    // Append methods (return StringBuilder for chaining)
    methods.insert(
        "append:(Ljava/lang/String;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        ),
    );
    methods.insert(
        "append:(I)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(I)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(J)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(J)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(F)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(F)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(D)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(D)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(Z)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(Z)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(C)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(Ljava/lang/Object;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        ),
    );
    methods.insert(
        "append:([C)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "([C)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:([CII)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("append", "([CII)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
        ),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;II)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/lang/StringBuilder;",
        ),
    );

    // Insert methods
    methods.insert(
        "insert:(ILjava/lang/String;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "insert",
            "(ILjava/lang/String;)Ljava/lang/StringBuilder;",
        ),
    );
    methods.insert(
        "insert:(IC)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("insert", "(IC)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "insert:(II)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("insert", "(II)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "insert:(IJ)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("insert", "(IJ)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "insert:(ILjava/lang/Object;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "insert",
            "(ILjava/lang/Object;)Ljava/lang/StringBuilder;",
        ),
    );

    // Other methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "capacity:()I".to_string(),
        StubMethodDescriptor::instance_method("capacity", "()I"),
    );
    methods.insert(
        "charAt:(I)C".to_string(),
        StubMethodDescriptor::instance_method("charAt", "(I)C"),
    );
    methods.insert(
        "setCharAt:(IC)V".to_string(),
        StubMethodDescriptor::instance_method("setCharAt", "(IC)V"),
    );
    methods.insert(
        "delete:(II)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("delete", "(II)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "deleteCharAt:(I)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("deleteCharAt", "(I)Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "replace:(IILjava/lang/String;)Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "replace",
            "(IILjava/lang/String;)Ljava/lang/StringBuilder;",
        ),
    );
    methods.insert(
        "substring:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("substring", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "substring:(II)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("substring", "(II)Ljava/lang/String;"),
    );
    methods.insert(
        "reverse:()Ljava/lang/StringBuilder;".to_string(),
        StubMethodDescriptor::instance_method("reverse", "()Ljava/lang/StringBuilder;"),
    );
    methods.insert(
        "setLength:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setLength", "(I)V"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "indexOf:(Ljava/lang/String;I)I".to_string(),
        StubMethodDescriptor::instance_method("indexOf", "(Ljava/lang/String;I)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "lastIndexOf:(Ljava/lang/String;I)I".to_string(),
        StubMethodDescriptor::instance_method("lastIndexOf", "(Ljava/lang/String;I)I"),
    );

    StubClass {
        name: "java/lang/StringBuilder".to_string(),
        superclass: Some("java/lang/AbstractStringBuilder".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
            "java/lang/CharSequence".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.StringBuffer class stub (thread-safe version of StringBuilder)
pub fn create_string_buffer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Append methods
    methods.insert(
        "append:(Ljava/lang/String;)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/String;)Ljava/lang/StringBuffer;",
        ),
    );
    methods.insert(
        "append:(I)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(I)Ljava/lang/StringBuffer;"),
    );
    methods.insert(
        "append:(J)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(J)Ljava/lang/StringBuffer;"),
    );
    methods.insert(
        "append:(D)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(D)Ljava/lang/StringBuffer;"),
    );
    methods.insert(
        "append:(Z)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(Z)Ljava/lang/StringBuffer;"),
    );
    methods.insert(
        "append:(C)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/lang/StringBuffer;"),
    );
    methods.insert(
        "append:(Ljava/lang/Object;)Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/Object;)Ljava/lang/StringBuffer;",
        ),
    );

    // Other methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "capacity:()I".to_string(),
        StubMethodDescriptor::instance_method("capacity", "()I"),
    );
    methods.insert(
        "reverse:()Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("reverse", "()Ljava/lang/StringBuffer;"),
    );

    StubClass {
        name: "java/lang/StringBuffer".to_string(),
        superclass: Some("java/lang/AbstractStringBuilder".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
            "java/lang/CharSequence".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.Number abstract class stub
pub fn create_number_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "byteValue:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValue", "()B"),
    );
    methods.insert(
        "shortValue:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValue", "()S"),
    );

    StubClass {
        name: "java/lang/Number".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.lang.Integer class stub
pub fn create_integer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors (deprecated but still used)
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "byteValue:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValue", "()B"),
    );
    methods.insert(
        "shortValue:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValue", "()S"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Integer;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Integer;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(I)Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(I)Ljava/lang/Integer;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Integer;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;I)Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;I)Ljava/lang/Integer;"),
    );
    methods.insert(
        "parseInt:(Ljava/lang/String;)I".to_string(),
        StubMethodDescriptor::static_method("parseInt", "(Ljava/lang/String;)I"),
    );
    methods.insert(
        "parseInt:(Ljava/lang/String;I)I".to_string(),
        StubMethodDescriptor::static_method("parseInt", "(Ljava/lang/String;I)I"),
    );
    methods.insert(
        "toString:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "toString:(II)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(II)Ljava/lang/String;"),
    );
    methods.insert(
        "toBinaryString:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toBinaryString", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "toHexString:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toHexString", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "toOctalString:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toOctalString", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(II)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(II)I"),
    );
    methods.insert(
        "max:(II)I".to_string(),
        StubMethodDescriptor::static_method("max", "(II)I"),
    );
    methods.insert(
        "min:(II)I".to_string(),
        StubMethodDescriptor::static_method("min", "(II)I"),
    );
    methods.insert(
        "sum:(II)I".to_string(),
        StubMethodDescriptor::static_method("sum", "(II)I"),
    );
    methods.insert(
        "signum:(I)I".to_string(),
        StubMethodDescriptor::static_method("signum", "(I)I"),
    );
    methods.insert(
        "reverse:(I)I".to_string(),
        StubMethodDescriptor::static_method("reverse", "(I)I"),
    );
    methods.insert(
        "reverseBytes:(I)I".to_string(),
        StubMethodDescriptor::static_method("reverseBytes", "(I)I"),
    );
    methods.insert(
        "bitCount:(I)I".to_string(),
        StubMethodDescriptor::static_method("bitCount", "(I)I"),
    );
    methods.insert(
        "highestOneBit:(I)I".to_string(),
        StubMethodDescriptor::static_method("highestOneBit", "(I)I"),
    );
    methods.insert(
        "lowestOneBit:(I)I".to_string(),
        StubMethodDescriptor::static_method("lowestOneBit", "(I)I"),
    );
    methods.insert(
        "numberOfLeadingZeros:(I)I".to_string(),
        StubMethodDescriptor::static_method("numberOfLeadingZeros", "(I)I"),
    );
    methods.insert(
        "numberOfTrailingZeros:(I)I".to_string(),
        StubMethodDescriptor::static_method("numberOfTrailingZeros", "(I)I"),
    );
    methods.insert(
        "rotateLeft:(II)I".to_string(),
        StubMethodDescriptor::static_method("rotateLeft", "(II)I"),
    );
    methods.insert(
        "rotateRight:(II)I".to_string(),
        StubMethodDescriptor::static_method("rotateRight", "(II)I"),
    );

    StubClass {
        name: "java/lang/Integer".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Long class stub
pub fn create_long_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(J)V".to_string(),
        StubMethodDescriptor::constructor("(J)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Long;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Long;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(J)Ljava/lang/Long;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(J)Ljava/lang/Long;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Long;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Long;"),
    );
    methods.insert(
        "parseLong:(Ljava/lang/String;)J".to_string(),
        StubMethodDescriptor::static_method("parseLong", "(Ljava/lang/String;)J"),
    );
    methods.insert(
        "parseLong:(Ljava/lang/String;I)J".to_string(),
        StubMethodDescriptor::static_method("parseLong", "(Ljava/lang/String;I)J"),
    );
    methods.insert(
        "toString:(J)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(J)Ljava/lang/String;"),
    );
    methods.insert(
        "toString:(JI)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(JI)Ljava/lang/String;"),
    );
    methods.insert(
        "toBinaryString:(J)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toBinaryString", "(J)Ljava/lang/String;"),
    );
    methods.insert(
        "toHexString:(J)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toHexString", "(J)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(JJ)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(JJ)I"),
    );
    methods.insert(
        "max:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("max", "(JJ)J"),
    );
    methods.insert(
        "min:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("min", "(JJ)J"),
    );
    methods.insert(
        "sum:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("sum", "(JJ)J"),
    );
    methods.insert(
        "signum:(J)I".to_string(),
        StubMethodDescriptor::static_method("signum", "(J)I"),
    );
    methods.insert(
        "bitCount:(J)I".to_string(),
        StubMethodDescriptor::static_method("bitCount", "(J)I"),
    );

    StubClass {
        name: "java/lang/Long".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Double class stub
pub fn create_double_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(D)V".to_string(),
        StubMethodDescriptor::constructor("(D)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Double;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Double;)I"),
    );
    methods.insert(
        "isNaN:()Z".to_string(),
        StubMethodDescriptor::instance_method("isNaN", "()Z"),
    );
    methods.insert(
        "isInfinite:()Z".to_string(),
        StubMethodDescriptor::instance_method("isInfinite", "()Z"),
    );

    // Static methods
    methods.insert(
        "valueOf:(D)Ljava/lang/Double;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(D)Ljava/lang/Double;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Double;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Double;"),
    );
    methods.insert(
        "parseDouble:(Ljava/lang/String;)D".to_string(),
        StubMethodDescriptor::static_method("parseDouble", "(Ljava/lang/String;)D"),
    );
    methods.insert(
        "toString:(D)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(D)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(DD)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(DD)I"),
    );
    methods.insert(
        "max:(DD)D".to_string(),
        StubMethodDescriptor::static_method("max", "(DD)D"),
    );
    methods.insert(
        "min:(DD)D".to_string(),
        StubMethodDescriptor::static_method("min", "(DD)D"),
    );
    methods.insert(
        "sum:(DD)D".to_string(),
        StubMethodDescriptor::static_method("sum", "(DD)D"),
    );
    methods.insert(
        "isNaN:(D)Z".to_string(),
        StubMethodDescriptor::static_method("isNaN", "(D)Z"),
    );
    methods.insert(
        "isInfinite:(D)Z".to_string(),
        StubMethodDescriptor::static_method("isInfinite", "(D)Z"),
    );
    methods.insert(
        "isFinite:(D)Z".to_string(),
        StubMethodDescriptor::static_method("isFinite", "(D)Z"),
    );
    methods.insert(
        "doubleToLongBits:(D)J".to_string(),
        StubMethodDescriptor::static_method("doubleToLongBits", "(D)J"),
    );
    methods.insert(
        "doubleToRawLongBits:(D)J".to_string(),
        StubMethodDescriptor::static_method("doubleToRawLongBits", "(D)J"),
    );
    methods.insert(
        "longBitsToDouble:(J)D".to_string(),
        StubMethodDescriptor::static_method("longBitsToDouble", "(J)D"),
    );

    StubClass {
        name: "java/lang/Double".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Float class stub
pub fn create_float_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(F)V".to_string(),
        StubMethodDescriptor::constructor("(F)V"),
    );
    methods.insert(
        "<init>:(D)V".to_string(),
        StubMethodDescriptor::constructor("(D)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Float;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Float;)I"),
    );
    methods.insert(
        "isNaN:()Z".to_string(),
        StubMethodDescriptor::instance_method("isNaN", "()Z"),
    );
    methods.insert(
        "isInfinite:()Z".to_string(),
        StubMethodDescriptor::instance_method("isInfinite", "()Z"),
    );

    // Static methods
    methods.insert(
        "valueOf:(F)Ljava/lang/Float;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(F)Ljava/lang/Float;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Float;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Float;"),
    );
    methods.insert(
        "parseFloat:(Ljava/lang/String;)F".to_string(),
        StubMethodDescriptor::static_method("parseFloat", "(Ljava/lang/String;)F"),
    );
    methods.insert(
        "toString:(F)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(F)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(FF)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(FF)I"),
    );
    methods.insert(
        "max:(FF)F".to_string(),
        StubMethodDescriptor::static_method("max", "(FF)F"),
    );
    methods.insert(
        "min:(FF)F".to_string(),
        StubMethodDescriptor::static_method("min", "(FF)F"),
    );
    methods.insert(
        "sum:(FF)F".to_string(),
        StubMethodDescriptor::static_method("sum", "(FF)F"),
    );
    methods.insert(
        "isNaN:(F)Z".to_string(),
        StubMethodDescriptor::static_method("isNaN", "(F)Z"),
    );
    methods.insert(
        "isInfinite:(F)Z".to_string(),
        StubMethodDescriptor::static_method("isInfinite", "(F)Z"),
    );
    methods.insert(
        "isFinite:(F)Z".to_string(),
        StubMethodDescriptor::static_method("isFinite", "(F)Z"),
    );
    methods.insert(
        "floatToIntBits:(F)I".to_string(),
        StubMethodDescriptor::static_method("floatToIntBits", "(F)I"),
    );
    methods.insert(
        "floatToRawIntBits:(F)I".to_string(),
        StubMethodDescriptor::static_method("floatToRawIntBits", "(F)I"),
    );
    methods.insert(
        "intBitsToFloat:(I)F".to_string(),
        StubMethodDescriptor::static_method("intBitsToFloat", "(I)F"),
    );

    StubClass {
        name: "java/lang/Float".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Boolean class stub
pub fn create_boolean_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Z)V".to_string(),
        StubMethodDescriptor::constructor("(Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "booleanValue:()Z".to_string(),
        StubMethodDescriptor::instance_method("booleanValue", "()Z"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Boolean;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Boolean;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(Z)Ljava/lang/Boolean;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Z)Ljava/lang/Boolean;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Boolean;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Boolean;"),
    );
    methods.insert(
        "parseBoolean:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::static_method("parseBoolean", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "toString:(Z)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(Z)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(ZZ)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(ZZ)I"),
    );
    methods.insert(
        "logicalAnd:(ZZ)Z".to_string(),
        StubMethodDescriptor::static_method("logicalAnd", "(ZZ)Z"),
    );
    methods.insert(
        "logicalOr:(ZZ)Z".to_string(),
        StubMethodDescriptor::static_method("logicalOr", "(ZZ)Z"),
    );
    methods.insert(
        "logicalXor:(ZZ)Z".to_string(),
        StubMethodDescriptor::static_method("logicalXor", "(ZZ)Z"),
    );

    StubClass {
        name: "java/lang/Boolean".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.Character class stub
pub fn create_character_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(C)V".to_string(),
        StubMethodDescriptor::constructor("(C)V"),
    );

    // Instance methods
    methods.insert(
        "charValue:()C".to_string(),
        StubMethodDescriptor::instance_method("charValue", "()C"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Character;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Character;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(C)Ljava/lang/Character;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(C)Ljava/lang/Character;"),
    );
    methods.insert(
        "toString:(C)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(C)Ljava/lang/String;"),
    );
    methods.insert(
        "isDigit:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isDigit", "(C)Z"),
    );
    methods.insert(
        "isLetter:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isLetter", "(C)Z"),
    );
    methods.insert(
        "isLetterOrDigit:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isLetterOrDigit", "(C)Z"),
    );
    methods.insert(
        "isUpperCase:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isUpperCase", "(C)Z"),
    );
    methods.insert(
        "isLowerCase:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isLowerCase", "(C)Z"),
    );
    methods.insert(
        "isWhitespace:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isWhitespace", "(C)Z"),
    );
    methods.insert(
        "isSpaceChar:(C)Z".to_string(),
        StubMethodDescriptor::static_method("isSpaceChar", "(C)Z"),
    );
    methods.insert(
        "toUpperCase:(C)C".to_string(),
        StubMethodDescriptor::static_method("toUpperCase", "(C)C"),
    );
    methods.insert(
        "toLowerCase:(C)C".to_string(),
        StubMethodDescriptor::static_method("toLowerCase", "(C)C"),
    );
    methods.insert(
        "compare:(CC)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(CC)I"),
    );
    methods.insert(
        "digit:(CI)I".to_string(),
        StubMethodDescriptor::static_method("digit", "(CI)I"),
    );
    methods.insert(
        "forDigit:(II)C".to_string(),
        StubMethodDescriptor::static_method("forDigit", "(II)C"),
    );
    methods.insert(
        "getNumericValue:(C)I".to_string(),
        StubMethodDescriptor::static_method("getNumericValue", "(C)I"),
    );
    methods.insert(
        "getType:(C)I".to_string(),
        StubMethodDescriptor::static_method("getType", "(C)I"),
    );

    StubClass {
        name: "java/lang/Character".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.Byte class stub
pub fn create_byte_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(B)V".to_string(),
        StubMethodDescriptor::constructor("(B)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "byteValue:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValue", "()B"),
    );
    methods.insert(
        "shortValue:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValue", "()S"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Byte;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Byte;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(B)Ljava/lang/Byte;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(B)Ljava/lang/Byte;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Byte;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Byte;"),
    );
    methods.insert(
        "parseByte:(Ljava/lang/String;)B".to_string(),
        StubMethodDescriptor::static_method("parseByte", "(Ljava/lang/String;)B"),
    );
    methods.insert(
        "parseByte:(Ljava/lang/String;I)B".to_string(),
        StubMethodDescriptor::static_method("parseByte", "(Ljava/lang/String;I)B"),
    );
    methods.insert(
        "toString:(B)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(B)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(BB)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(BB)I"),
    );
    methods.insert(
        "toUnsignedInt:(B)I".to_string(),
        StubMethodDescriptor::static_method("toUnsignedInt", "(B)I"),
    );
    methods.insert(
        "toUnsignedLong:(B)J".to_string(),
        StubMethodDescriptor::static_method("toUnsignedLong", "(B)J"),
    );

    StubClass {
        name: "java/lang/Byte".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Short class stub
pub fn create_short_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(S)V".to_string(),
        StubMethodDescriptor::constructor("(S)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "shortValue:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValue", "()S"),
    );
    methods.insert(
        "byteValue:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValue", "()B"),
    );
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "compareTo:(Ljava/lang/Short;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Short;)I"),
    );

    // Static methods
    methods.insert(
        "valueOf:(S)Ljava/lang/Short;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(S)Ljava/lang/Short;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/lang/Short;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(Ljava/lang/String;)Ljava/lang/Short;"),
    );
    methods.insert(
        "parseShort:(Ljava/lang/String;)S".to_string(),
        StubMethodDescriptor::static_method("parseShort", "(Ljava/lang/String;)S"),
    );
    methods.insert(
        "parseShort:(Ljava/lang/String;I)S".to_string(),
        StubMethodDescriptor::static_method("parseShort", "(Ljava/lang/String;I)S"),
    );
    methods.insert(
        "toString:(S)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("toString", "(S)Ljava/lang/String;"),
    );
    methods.insert(
        "compare:(SS)I".to_string(),
        StubMethodDescriptor::static_method("compare", "(SS)I"),
    );
    methods.insert(
        "toUnsignedInt:(S)I".to_string(),
        StubMethodDescriptor::static_method("toUnsignedInt", "(S)I"),
    );
    methods.insert(
        "toUnsignedLong:(S)J".to_string(),
        StubMethodDescriptor::static_method("toUnsignedLong", "(S)J"),
    );
    methods.insert(
        "reverseBytes:(S)S".to_string(),
        StubMethodDescriptor::static_method("reverseBytes", "(S)S"),
    );

    StubClass {
        name: "java/lang/Short".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.Class class stub
pub fn create_class_class() -> StubClass {
    let mut methods = HashMap::new();

    // Instance methods (Class has no public constructors)
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getSimpleName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getSimpleName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getCanonicalName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getCanonicalName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getTypeName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getTypeName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getSuperclass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getSuperclass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getInterfaces:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getInterfaces", "()[Ljava/lang/Class;"),
    );
    methods.insert(
        "isInstance:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("isInstance", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "isAssignableFrom:(Ljava/lang/Class;)Z".to_string(),
        StubMethodDescriptor::instance_method("isAssignableFrom", "(Ljava/lang/Class;)Z"),
    );
    methods.insert(
        "isInterface:()Z".to_string(),
        StubMethodDescriptor::instance_method("isInterface", "()Z"),
    );
    methods.insert(
        "isArray:()Z".to_string(),
        StubMethodDescriptor::instance_method("isArray", "()Z"),
    );
    methods.insert(
        "isPrimitive:()Z".to_string(),
        StubMethodDescriptor::instance_method("isPrimitive", "()Z"),
    );
    methods.insert(
        "isEnum:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEnum", "()Z"),
    );
    methods.insert(
        "isAnnotation:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAnnotation", "()Z"),
    );
    methods.insert(
        "newInstance:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("newInstance", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getComponentType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getComponentType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getClassLoader:()Ljava/lang/ClassLoader;".to_string(),
        StubMethodDescriptor::instance_method("getClassLoader", "()Ljava/lang/ClassLoader;"),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );
    methods.insert(
        "getPackage:()Ljava/lang/Package;".to_string(),
        StubMethodDescriptor::instance_method("getPackage", "()Ljava/lang/Package;"),
    );
    methods.insert(
        "getPackageName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getPackageName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    // Reflection methods - methods
    methods.insert(
        "getDeclaredMethods:()[Ljava/lang/reflect/Method;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredMethods",
            "()[Ljava/lang/reflect/Method;",
        ),
    );
    methods.insert(
        "getMethods:()[Ljava/lang/reflect/Method;".to_string(),
        StubMethodDescriptor::instance_method("getMethods", "()[Ljava/lang/reflect/Method;"),
    );
    methods.insert(
        "getDeclaredMethod:(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredMethod",
            "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        ),
    );
    methods.insert(
        "getMethod:(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMethod",
            "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        ),
    );

    // Reflection methods - fields
    methods.insert(
        "getDeclaredFields:()[Ljava/lang/reflect/Field;".to_string(),
        StubMethodDescriptor::instance_method("getDeclaredFields", "()[Ljava/lang/reflect/Field;"),
    );
    methods.insert(
        "getFields:()[Ljava/lang/reflect/Field;".to_string(),
        StubMethodDescriptor::instance_method("getFields", "()[Ljava/lang/reflect/Field;"),
    );
    methods.insert(
        "getDeclaredField:(Ljava/lang/String;)Ljava/lang/reflect/Field;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredField",
            "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
        ),
    );
    methods.insert(
        "getField:(Ljava/lang/String;)Ljava/lang/reflect/Field;".to_string(),
        StubMethodDescriptor::instance_method(
            "getField",
            "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
        ),
    );

    // Reflection methods - constructors
    methods.insert(
        "getDeclaredConstructors:()[Ljava/lang/reflect/Constructor;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredConstructors",
            "()[Ljava/lang/reflect/Constructor;",
        ),
    );
    methods.insert(
        "getConstructors:()[Ljava/lang/reflect/Constructor;".to_string(),
        StubMethodDescriptor::instance_method(
            "getConstructors",
            "()[Ljava/lang/reflect/Constructor;",
        ),
    );
    methods.insert(
        "getDeclaredConstructor:([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredConstructor",
            "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        ),
    );
    methods.insert(
        "getConstructor:([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;".to_string(),
        StubMethodDescriptor::instance_method(
            "getConstructor",
            "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        ),
    );

    // Reflection methods - annotations
    methods.insert(
        "getAnnotations:()[Ljava/lang/annotation/Annotation;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAnnotations",
            "()[Ljava/lang/annotation/Annotation;",
        ),
    );
    methods.insert(
        "getDeclaredAnnotations:()[Ljava/lang/annotation/Annotation;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaredAnnotations",
            "()[Ljava/lang/annotation/Annotation;",
        ),
    );

    // Static methods
    methods.insert(
        "forName:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::static_method("forName", "(Ljava/lang/String;)Ljava/lang/Class;"),
    );
    methods.insert(
        "forName:(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::static_method(
            "forName",
            "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
        ),
    );

    StubClass {
        name: "java/lang/Class".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/reflect/GenericDeclaration".to_string(),
            "java/lang/reflect/Type".to_string(),
            "java/lang/reflect/AnnotatedElement".to_string(),
        ],
        methods,
    }
}

/// Create java.lang.System class stub
pub fn create_system_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static methods only (System cannot be instantiated)
    methods.insert(
        "currentTimeMillis:()J".to_string(),
        StubMethodDescriptor::static_method("currentTimeMillis", "()J"),
    );
    methods.insert(
        "nanoTime:()J".to_string(),
        StubMethodDescriptor::static_method("nanoTime", "()J"),
    );
    methods.insert(
        "arraycopy:(Ljava/lang/Object;ILjava/lang/Object;II)V".to_string(),
        StubMethodDescriptor::static_method(
            "arraycopy",
            "(Ljava/lang/Object;ILjava/lang/Object;II)V",
        ),
    );
    methods.insert(
        "exit:(I)V".to_string(),
        StubMethodDescriptor::static_method("exit", "(I)V"),
    );
    methods.insert(
        "gc:()V".to_string(),
        StubMethodDescriptor::static_method("gc", "()V"),
    );
    methods.insert(
        "runFinalization:()V".to_string(),
        StubMethodDescriptor::static_method("runFinalization", "()V"),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "getProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "getenv:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("getenv", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "getenv:()Ljava/util/Map;".to_string(),
        StubMethodDescriptor::static_method("getenv", "()Ljava/util/Map;"),
    );
    methods.insert(
        "identityHashCode:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("identityHashCode", "(Ljava/lang/Object;)I"),
    );
    methods.insert(
        "lineSeparator:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("lineSeparator", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/System".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Math class stub
pub fn create_math_class() -> StubClass {
    let mut methods = HashMap::new();

    // All static methods (Math cannot be instantiated)
    methods.insert(
        "abs:(I)I".to_string(),
        StubMethodDescriptor::static_method("abs", "(I)I"),
    );
    methods.insert(
        "abs:(J)J".to_string(),
        StubMethodDescriptor::static_method("abs", "(J)J"),
    );
    methods.insert(
        "abs:(F)F".to_string(),
        StubMethodDescriptor::static_method("abs", "(F)F"),
    );
    methods.insert(
        "abs:(D)D".to_string(),
        StubMethodDescriptor::static_method("abs", "(D)D"),
    );
    methods.insert(
        "max:(II)I".to_string(),
        StubMethodDescriptor::static_method("max", "(II)I"),
    );
    methods.insert(
        "max:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("max", "(JJ)J"),
    );
    methods.insert(
        "max:(FF)F".to_string(),
        StubMethodDescriptor::static_method("max", "(FF)F"),
    );
    methods.insert(
        "max:(DD)D".to_string(),
        StubMethodDescriptor::static_method("max", "(DD)D"),
    );
    methods.insert(
        "min:(II)I".to_string(),
        StubMethodDescriptor::static_method("min", "(II)I"),
    );
    methods.insert(
        "min:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("min", "(JJ)J"),
    );
    methods.insert(
        "min:(FF)F".to_string(),
        StubMethodDescriptor::static_method("min", "(FF)F"),
    );
    methods.insert(
        "min:(DD)D".to_string(),
        StubMethodDescriptor::static_method("min", "(DD)D"),
    );
    methods.insert(
        "sqrt:(D)D".to_string(),
        StubMethodDescriptor::static_method("sqrt", "(D)D"),
    );
    methods.insert(
        "cbrt:(D)D".to_string(),
        StubMethodDescriptor::static_method("cbrt", "(D)D"),
    );
    methods.insert(
        "pow:(DD)D".to_string(),
        StubMethodDescriptor::static_method("pow", "(DD)D"),
    );
    methods.insert(
        "exp:(D)D".to_string(),
        StubMethodDescriptor::static_method("exp", "(D)D"),
    );
    methods.insert(
        "log:(D)D".to_string(),
        StubMethodDescriptor::static_method("log", "(D)D"),
    );
    methods.insert(
        "log10:(D)D".to_string(),
        StubMethodDescriptor::static_method("log10", "(D)D"),
    );
    methods.insert(
        "sin:(D)D".to_string(),
        StubMethodDescriptor::static_method("sin", "(D)D"),
    );
    methods.insert(
        "cos:(D)D".to_string(),
        StubMethodDescriptor::static_method("cos", "(D)D"),
    );
    methods.insert(
        "tan:(D)D".to_string(),
        StubMethodDescriptor::static_method("tan", "(D)D"),
    );
    methods.insert(
        "asin:(D)D".to_string(),
        StubMethodDescriptor::static_method("asin", "(D)D"),
    );
    methods.insert(
        "acos:(D)D".to_string(),
        StubMethodDescriptor::static_method("acos", "(D)D"),
    );
    methods.insert(
        "atan:(D)D".to_string(),
        StubMethodDescriptor::static_method("atan", "(D)D"),
    );
    methods.insert(
        "atan2:(DD)D".to_string(),
        StubMethodDescriptor::static_method("atan2", "(DD)D"),
    );
    methods.insert(
        "sinh:(D)D".to_string(),
        StubMethodDescriptor::static_method("sinh", "(D)D"),
    );
    methods.insert(
        "cosh:(D)D".to_string(),
        StubMethodDescriptor::static_method("cosh", "(D)D"),
    );
    methods.insert(
        "tanh:(D)D".to_string(),
        StubMethodDescriptor::static_method("tanh", "(D)D"),
    );
    methods.insert(
        "ceil:(D)D".to_string(),
        StubMethodDescriptor::static_method("ceil", "(D)D"),
    );
    methods.insert(
        "floor:(D)D".to_string(),
        StubMethodDescriptor::static_method("floor", "(D)D"),
    );
    methods.insert(
        "round:(F)I".to_string(),
        StubMethodDescriptor::static_method("round", "(F)I"),
    );
    methods.insert(
        "round:(D)J".to_string(),
        StubMethodDescriptor::static_method("round", "(D)J"),
    );
    methods.insert(
        "rint:(D)D".to_string(),
        StubMethodDescriptor::static_method("rint", "(D)D"),
    );
    methods.insert(
        "random:()D".to_string(),
        StubMethodDescriptor::static_method("random", "()D"),
    );
    methods.insert(
        "toRadians:(D)D".to_string(),
        StubMethodDescriptor::static_method("toRadians", "(D)D"),
    );
    methods.insert(
        "toDegrees:(D)D".to_string(),
        StubMethodDescriptor::static_method("toDegrees", "(D)D"),
    );
    methods.insert(
        "signum:(D)D".to_string(),
        StubMethodDescriptor::static_method("signum", "(D)D"),
    );
    methods.insert(
        "signum:(F)F".to_string(),
        StubMethodDescriptor::static_method("signum", "(F)F"),
    );
    methods.insert(
        "hypot:(DD)D".to_string(),
        StubMethodDescriptor::static_method("hypot", "(DD)D"),
    );
    methods.insert(
        "expm1:(D)D".to_string(),
        StubMethodDescriptor::static_method("expm1", "(D)D"),
    );
    methods.insert(
        "log1p:(D)D".to_string(),
        StubMethodDescriptor::static_method("log1p", "(D)D"),
    );
    methods.insert(
        "copySign:(DD)D".to_string(),
        StubMethodDescriptor::static_method("copySign", "(DD)D"),
    );
    methods.insert(
        "copySign:(FF)F".to_string(),
        StubMethodDescriptor::static_method("copySign", "(FF)F"),
    );
    methods.insert(
        "getExponent:(D)I".to_string(),
        StubMethodDescriptor::static_method("getExponent", "(D)I"),
    );
    methods.insert(
        "getExponent:(F)I".to_string(),
        StubMethodDescriptor::static_method("getExponent", "(F)I"),
    );
    methods.insert(
        "scalb:(DI)D".to_string(),
        StubMethodDescriptor::static_method("scalb", "(DI)D"),
    );
    methods.insert(
        "scalb:(FI)F".to_string(),
        StubMethodDescriptor::static_method("scalb", "(FI)F"),
    );
    methods.insert(
        "nextAfter:(DD)D".to_string(),
        StubMethodDescriptor::static_method("nextAfter", "(DD)D"),
    );
    methods.insert(
        "nextAfter:(FD)F".to_string(),
        StubMethodDescriptor::static_method("nextAfter", "(FD)F"),
    );
    methods.insert(
        "nextUp:(D)D".to_string(),
        StubMethodDescriptor::static_method("nextUp", "(D)D"),
    );
    methods.insert(
        "nextUp:(F)F".to_string(),
        StubMethodDescriptor::static_method("nextUp", "(F)F"),
    );
    methods.insert(
        "nextDown:(D)D".to_string(),
        StubMethodDescriptor::static_method("nextDown", "(D)D"),
    );
    methods.insert(
        "nextDown:(F)F".to_string(),
        StubMethodDescriptor::static_method("nextDown", "(F)F"),
    );
    methods.insert(
        "ulp:(D)D".to_string(),
        StubMethodDescriptor::static_method("ulp", "(D)D"),
    );
    methods.insert(
        "ulp:(F)F".to_string(),
        StubMethodDescriptor::static_method("ulp", "(F)F"),
    );
    methods.insert(
        "IEEEremainder:(DD)D".to_string(),
        StubMethodDescriptor::static_method("IEEEremainder", "(DD)D"),
    );
    methods.insert(
        "floorDiv:(II)I".to_string(),
        StubMethodDescriptor::static_method("floorDiv", "(II)I"),
    );
    methods.insert(
        "floorDiv:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("floorDiv", "(JJ)J"),
    );
    methods.insert(
        "floorMod:(II)I".to_string(),
        StubMethodDescriptor::static_method("floorMod", "(II)I"),
    );
    methods.insert(
        "floorMod:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("floorMod", "(JJ)J"),
    );
    methods.insert(
        "addExact:(II)I".to_string(),
        StubMethodDescriptor::static_method("addExact", "(II)I"),
    );
    methods.insert(
        "addExact:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("addExact", "(JJ)J"),
    );
    methods.insert(
        "subtractExact:(II)I".to_string(),
        StubMethodDescriptor::static_method("subtractExact", "(II)I"),
    );
    methods.insert(
        "subtractExact:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("subtractExact", "(JJ)J"),
    );
    methods.insert(
        "multiplyExact:(II)I".to_string(),
        StubMethodDescriptor::static_method("multiplyExact", "(II)I"),
    );
    methods.insert(
        "multiplyExact:(JJ)J".to_string(),
        StubMethodDescriptor::static_method("multiplyExact", "(JJ)J"),
    );
    methods.insert(
        "negateExact:(I)I".to_string(),
        StubMethodDescriptor::static_method("negateExact", "(I)I"),
    );
    methods.insert(
        "negateExact:(J)J".to_string(),
        StubMethodDescriptor::static_method("negateExact", "(J)J"),
    );
    methods.insert(
        "toIntExact:(J)I".to_string(),
        StubMethodDescriptor::static_method("toIntExact", "(J)I"),
    );
    methods.insert(
        "fma:(DDD)D".to_string(),
        StubMethodDescriptor::static_method("fma", "(DDD)D"),
    );
    methods.insert(
        "fma:(FFF)F".to_string(),
        StubMethodDescriptor::static_method("fma", "(FFF)F"),
    );

    StubClass {
        name: "java/lang/Math".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Throwable class stub
pub fn create_throwable_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/Throwable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Throwable;)V"),
    );

    // Instance methods
    methods.insert(
        "getMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMessage", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getLocalizedMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getLocalizedMessage", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getCause:()Ljava/lang/Throwable;".to_string(),
        StubMethodDescriptor::instance_method("getCause", "()Ljava/lang/Throwable;"),
    );
    methods.insert(
        "initCause:(Ljava/lang/Throwable;)Ljava/lang/Throwable;".to_string(),
        StubMethodDescriptor::instance_method(
            "initCause",
            "(Ljava/lang/Throwable;)Ljava/lang/Throwable;",
        ),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "printStackTrace:()V".to_string(),
        StubMethodDescriptor::instance_method("printStackTrace", "()V"),
    );
    methods.insert(
        "printStackTrace:(Ljava/io/PrintStream;)V".to_string(),
        StubMethodDescriptor::instance_method("printStackTrace", "(Ljava/io/PrintStream;)V"),
    );
    methods.insert(
        "printStackTrace:(Ljava/io/PrintWriter;)V".to_string(),
        StubMethodDescriptor::instance_method("printStackTrace", "(Ljava/io/PrintWriter;)V"),
    );
    methods.insert(
        "fillInStackTrace:()Ljava/lang/Throwable;".to_string(),
        StubMethodDescriptor::instance_method("fillInStackTrace", "()Ljava/lang/Throwable;"),
    );
    methods.insert(
        "getStackTrace:()[Ljava/lang/StackTraceElement;".to_string(),
        StubMethodDescriptor::instance_method("getStackTrace", "()[Ljava/lang/StackTraceElement;"),
    );
    methods.insert(
        "setStackTrace:([Ljava/lang/StackTraceElement;)V".to_string(),
        StubMethodDescriptor::instance_method("setStackTrace", "([Ljava/lang/StackTraceElement;)V"),
    );
    methods.insert(
        "getSuppressed:()[Ljava/lang/Throwable;".to_string(),
        StubMethodDescriptor::instance_method("getSuppressed", "()[Ljava/lang/Throwable;"),
    );
    methods.insert(
        "addSuppressed:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::instance_method("addSuppressed", "(Ljava/lang/Throwable;)V"),
    );

    StubClass {
        name: "java/lang/Throwable".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.lang.Exception class stub
pub fn create_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors (inherits everything from Throwable)
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/Throwable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Throwable;)V"),
    );

    StubClass {
        name: "java/lang/Exception".to_string(),
        superclass: Some("java/lang/Throwable".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.RuntimeException class stub
pub fn create_runtime_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/Throwable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Throwable;)V"),
    );

    StubClass {
        name: "java/lang/RuntimeException".to_string(),
        superclass: Some("java/lang/Exception".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Comparable interface stub
pub fn create_comparable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "compareTo:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Object;)I"),
    );

    StubClass {
        name: "java/lang/Comparable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.AutoCloseable interface stub.
pub fn create_auto_closeable_interface() -> StubClass {
    let mut methods = HashMap::new();
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/lang/AutoCloseable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.CharSequence interface stub
pub fn create_charsequence_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "charAt:(I)C".to_string(),
        StubMethodDescriptor::instance_method("charAt", "(I)C"),
    );
    methods.insert(
        "subSequence:(II)Ljava/lang/CharSequence;".to_string(),
        StubMethodDescriptor::instance_method("subSequence", "(II)Ljava/lang/CharSequence;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/CharSequence".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Runnable interface stub
pub fn create_runnable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "run:()V".to_string(),
        StubMethodDescriptor::instance_method("run", "()V"),
    );

    StubClass {
        name: "java/lang/Runnable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Thread class stub
///
/// Thread provides the basis for concurrent execution in the JVM.
/// This implementation provides both a stub for type resolution and
/// native method support for actual thread execution.
pub fn create_thread_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Runnable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Runnable;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Runnable;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;Ljava/lang/String;)V",
        ),
    );

    // Instance methods - thread lifecycle
    methods.insert(
        "start:()V".to_string(),
        StubMethodDescriptor::instance_method("start", "()V"),
    );
    methods.insert(
        "run:()V".to_string(),
        StubMethodDescriptor::instance_method("run", "()V"),
    );
    methods.insert(
        "join:()V".to_string(),
        StubMethodDescriptor::instance_method("join", "()V"),
    );
    methods.insert(
        "join:(J)V".to_string(),
        StubMethodDescriptor::instance_method("join", "(J)V"),
    );
    methods.insert(
        "join:(JI)V".to_string(),
        StubMethodDescriptor::instance_method("join", "(JI)V"),
    );
    methods.insert(
        "interrupt:()V".to_string(),
        StubMethodDescriptor::instance_method("interrupt", "()V"),
    );
    methods.insert(
        "isInterrupted:()Z".to_string(),
        StubMethodDescriptor::instance_method("isInterrupted", "()Z"),
    );
    methods.insert(
        "isAlive:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAlive", "()Z"),
    );

    // Instance methods - thread properties
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "setName:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("setName", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "getId:()J".to_string(),
        StubMethodDescriptor::instance_method("getId", "()J"),
    );
    methods.insert(
        "getPriority:()I".to_string(),
        StubMethodDescriptor::instance_method("getPriority", "()I"),
    );
    methods.insert(
        "setPriority:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setPriority", "(I)V"),
    );
    methods.insert(
        "isDaemon:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDaemon", "()Z"),
    );
    methods.insert(
        "setDaemon:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("setDaemon", "(Z)V"),
    );
    methods.insert(
        "getState:()Ljava/lang/Thread$State;".to_string(),
        StubMethodDescriptor::instance_method("getState", "()Ljava/lang/Thread$State;"),
    );
    methods.insert(
        "getThreadGroup:()Ljava/lang/ThreadGroup;".to_string(),
        StubMethodDescriptor::instance_method("getThreadGroup", "()Ljava/lang/ThreadGroup;"),
    );

    // Static methods
    methods.insert(
        "currentThread:()Ljava/lang/Thread;".to_string(),
        StubMethodDescriptor::static_method("currentThread", "()Ljava/lang/Thread;"),
    );
    methods.insert(
        "sleep:(J)V".to_string(),
        StubMethodDescriptor::static_method("sleep", "(J)V"),
    );
    methods.insert(
        "sleep:(JI)V".to_string(),
        StubMethodDescriptor::static_method("sleep", "(JI)V"),
    );
    methods.insert(
        "yield:()V".to_string(),
        StubMethodDescriptor::static_method("yield", "()V"),
    );
    methods.insert(
        "interrupted:()Z".to_string(),
        StubMethodDescriptor::static_method("interrupted", "()Z"),
    );
    methods.insert(
        "activeCount:()I".to_string(),
        StubMethodDescriptor::static_method("activeCount", "()I"),
    );
    methods.insert(
        "holdsLock:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::static_method("holdsLock", "(Ljava/lang/Object;)Z"),
    );

    // Object methods (inherited but commonly used)
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/Thread".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/lang/Runnable".to_string()],
        methods,
    }
}

/// Create java.lang.ThreadGroup class stub
pub fn create_thread_group_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ThreadGroup;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ThreadGroup;Ljava/lang/String;)V"),
    );

    // Instance methods
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getParent:()Ljava/lang/ThreadGroup;".to_string(),
        StubMethodDescriptor::instance_method("getParent", "()Ljava/lang/ThreadGroup;"),
    );
    methods.insert(
        "activeCount:()I".to_string(),
        StubMethodDescriptor::instance_method("activeCount", "()I"),
    );
    methods.insert(
        "activeGroupCount:()I".to_string(),
        StubMethodDescriptor::instance_method("activeGroupCount", "()I"),
    );
    methods.insert(
        "interrupt:()V".to_string(),
        StubMethodDescriptor::instance_method("interrupt", "()V"),
    );

    StubClass {
        name: "java/lang/ThreadGroup".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.reflect.Method class stub
pub fn create_reflect_method_class() -> StubClass {
    let mut methods = HashMap::new();

    // Instance methods
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getParameterTypes:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getParameterTypes", "()[Ljava/lang/Class;"),
    );
    methods.insert(
        "getReturnType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getReturnType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );
    methods.insert(
        "invoke:(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invoke",
            "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getAnnotations:()[Ljava/lang/annotation/Annotation;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAnnotations",
            "()[Ljava/lang/annotation/Annotation;",
        ),
    );
    methods.insert(
        "isVarArgs:()Z".to_string(),
        StubMethodDescriptor::instance_method("isVarArgs", "()Z"),
    );
    methods.insert(
        "getDeclaringClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getDeclaringClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/reflect/Method".to_string(),
        superclass: Some("java/lang/reflect/AccessibleObject".to_string()),
        interfaces: vec!["java/lang/reflect/Member".to_string()],
        methods,
    }
}

/// Create java.lang.reflect.Field class stub
pub fn create_reflect_field_class() -> StubClass {
    let mut methods = HashMap::new();

    // Instance methods
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(Ljava/lang/Object;Ljava/lang/Object;)V"),
    );
    methods.insert(
        "getAnnotations:()[Ljava/lang/annotation/Annotation;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAnnotations",
            "()[Ljava/lang/annotation/Annotation;",
        ),
    );
    methods.insert(
        "getDeclaringClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getDeclaringClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/reflect/Field".to_string(),
        superclass: Some("java/lang/reflect/AccessibleObject".to_string()),
        interfaces: vec!["java/lang/reflect/Member".to_string()],
        methods,
    }
}

/// Create java.lang.reflect.Constructor class stub
pub fn create_reflect_constructor_class() -> StubClass {
    let mut methods = HashMap::new();

    // Instance methods
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getParameterTypes:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getParameterTypes", "()[Ljava/lang/Class;"),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );
    methods.insert(
        "newInstance:([Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "newInstance",
            "([Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getAnnotations:()[Ljava/lang/annotation/Annotation;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAnnotations",
            "()[Ljava/lang/annotation/Annotation;",
        ),
    );
    methods.insert(
        "getDeclaringClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getDeclaringClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/lang/reflect/Constructor".to_string(),
        superclass: Some("java/lang/reflect/AccessibleObject".to_string()),
        interfaces: vec!["java/lang/reflect/Member".to_string()],
        methods,
    }
}

/// Create java.lang.reflect.Array class stub
pub fn create_reflect_array_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static methods only
    methods.insert(
        "newInstance:(Ljava/lang/Class;I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method(
            "newInstance",
            "(Ljava/lang/Class;I)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "get:(Ljava/lang/Object;I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::static_method("get", "(Ljava/lang/Object;I)Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(Ljava/lang/Object;ILjava/lang/Object;)V".to_string(),
        StubMethodDescriptor::static_method("set", "(Ljava/lang/Object;ILjava/lang/Object;)V"),
    );
    methods.insert(
        "getLength:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::static_method("getLength", "(Ljava/lang/Object;)I"),
    );

    StubClass {
        name: "java/lang/reflect/Array".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.reflect.Modifier class stub
pub fn create_reflect_modifier_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static methods only
    methods.insert(
        "isPublic:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isPublic", "(I)Z"),
    );
    methods.insert(
        "isPrivate:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isPrivate", "(I)Z"),
    );
    methods.insert(
        "isProtected:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isProtected", "(I)Z"),
    );
    methods.insert(
        "isStatic:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isStatic", "(I)Z"),
    );
    methods.insert(
        "isFinal:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isFinal", "(I)Z"),
    );
    methods.insert(
        "isAbstract:(I)Z".to_string(),
        StubMethodDescriptor::static_method("isAbstract", "(I)Z"),
    );

    StubClass {
        name: "java/lang/reflect/Modifier".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Runtime class stub
pub fn create_runtime_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static methods
    methods.insert(
        "getRuntime:()Ljava/lang/Runtime;".to_string(),
        StubMethodDescriptor::static_method("getRuntime", "()Ljava/lang/Runtime;"),
    );

    // Instance methods - exec variants
    methods.insert(
        "exec:(Ljava/lang/String;)Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method("exec", "(Ljava/lang/String;)Ljava/lang/Process;"),
    );
    methods.insert(
        "exec:([Ljava/lang/String;)Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method("exec", "([Ljava/lang/String;)Ljava/lang/Process;"),
    );
    methods.insert(
        "exec:([Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method(
            "exec",
            "([Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Process;",
        ),
    );
    methods.insert(
        "exec:([Ljava/lang/String;[Ljava/lang/String;Ljava/io/File;)Ljava/lang/Process;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "exec",
            "([Ljava/lang/String;[Ljava/lang/String;Ljava/io/File;)Ljava/lang/Process;",
        ),
    );
    methods.insert(
        "exec:(Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method(
            "exec",
            "(Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Process;",
        ),
    );
    methods.insert(
        "exec:(Ljava/lang/String;[Ljava/lang/String;Ljava/io/File;)Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method(
            "exec",
            "(Ljava/lang/String;[Ljava/lang/String;Ljava/io/File;)Ljava/lang/Process;",
        ),
    );

    // Memory/processor info
    methods.insert(
        "availableProcessors:()I".to_string(),
        StubMethodDescriptor::instance_method("availableProcessors", "()I"),
    );
    methods.insert(
        "freeMemory:()J".to_string(),
        StubMethodDescriptor::instance_method("freeMemory", "()J"),
    );
    methods.insert(
        "totalMemory:()J".to_string(),
        StubMethodDescriptor::instance_method("totalMemory", "()J"),
    );
    methods.insert(
        "maxMemory:()J".to_string(),
        StubMethodDescriptor::instance_method("maxMemory", "()J"),
    );

    // GC and exit
    methods.insert(
        "gc:()V".to_string(),
        StubMethodDescriptor::instance_method("gc", "()V"),
    );
    methods.insert(
        "exit:(I)V".to_string(),
        StubMethodDescriptor::instance_method("exit", "(I)V"),
    );
    methods.insert(
        "halt:(I)V".to_string(),
        StubMethodDescriptor::instance_method("halt", "(I)V"),
    );

    // Shutdown hooks
    methods.insert(
        "addShutdownHook:(Ljava/lang/Thread;)V".to_string(),
        StubMethodDescriptor::instance_method("addShutdownHook", "(Ljava/lang/Thread;)V"),
    );
    methods.insert(
        "removeShutdownHook:(Ljava/lang/Thread;)Z".to_string(),
        StubMethodDescriptor::instance_method("removeShutdownHook", "(Ljava/lang/Thread;)Z"),
    );

    // Library loading
    methods.insert(
        "load:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("load", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "loadLibrary:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("loadLibrary", "(Ljava/lang/String;)V"),
    );

    // Finalization
    methods.insert(
        "runFinalization:()V".to_string(),
        StubMethodDescriptor::instance_method("runFinalization", "()V"),
    );

    // Version info (Java 9+)
    methods.insert(
        "version:()Ljava/lang/Runtime$Version;".to_string(),
        StubMethodDescriptor::static_method("version", "()Ljava/lang/Runtime$Version;"),
    );

    StubClass {
        name: "java/lang/Runtime".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.Process class stub (abstract class)
pub fn create_process_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor (protected in real JDK)
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Abstract stream methods
    methods.insert(
        "getInputStream:()Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::instance_method("getInputStream", "()Ljava/io/InputStream;"),
    );
    methods.insert(
        "getErrorStream:()Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::instance_method("getErrorStream", "()Ljava/io/InputStream;"),
    );
    methods.insert(
        "getOutputStream:()Ljava/io/OutputStream;".to_string(),
        StubMethodDescriptor::instance_method("getOutputStream", "()Ljava/io/OutputStream;"),
    );

    // Wait for completion
    methods.insert(
        "waitFor:()I".to_string(),
        StubMethodDescriptor::instance_method("waitFor", "()I"),
    );
    methods.insert(
        "waitFor:(JLjava/util/concurrent/TimeUnit;)Z".to_string(),
        StubMethodDescriptor::instance_method("waitFor", "(JLjava/util/concurrent/TimeUnit;)Z"),
    );

    // Exit value and lifecycle
    methods.insert(
        "exitValue:()I".to_string(),
        StubMethodDescriptor::instance_method("exitValue", "()I"),
    );
    methods.insert(
        "destroy:()V".to_string(),
        StubMethodDescriptor::instance_method("destroy", "()V"),
    );
    methods.insert(
        "destroyForcibly:()Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method("destroyForcibly", "()Ljava/lang/Process;"),
    );
    methods.insert(
        "isAlive:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAlive", "()Z"),
    );

    // Process info (Java 9+)
    methods.insert(
        "pid:()J".to_string(),
        StubMethodDescriptor::instance_method("pid", "()J"),
    );
    methods.insert(
        "toHandle:()Ljava/lang/ProcessHandle;".to_string(),
        StubMethodDescriptor::instance_method("toHandle", "()Ljava/lang/ProcessHandle;"),
    );
    methods.insert(
        "info:()Ljava/lang/ProcessHandle$Info;".to_string(),
        StubMethodDescriptor::instance_method("info", "()Ljava/lang/ProcessHandle$Info;"),
    );
    methods.insert(
        "children:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("children", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "descendants:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("descendants", "()Ljava/util/stream/Stream;"),
    );

    // Completion handlers (Java 9+)
    methods.insert(
        "onExit:()Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "onExit",
            "()Ljava/util/concurrent/CompletableFuture;",
        ),
    );

    // Input/output streams (Java 9+)
    methods.insert(
        "inputReader:()Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::instance_method("inputReader", "()Ljava/io/BufferedReader;"),
    );
    methods.insert(
        "inputReader:(Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::instance_method(
            "inputReader",
            "(Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;",
        ),
    );
    methods.insert(
        "errorReader:()Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::instance_method("errorReader", "()Ljava/io/BufferedReader;"),
    );
    methods.insert(
        "errorReader:(Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::instance_method(
            "errorReader",
            "(Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;",
        ),
    );
    methods.insert(
        "outputWriter:()Ljava/io/BufferedWriter;".to_string(),
        StubMethodDescriptor::instance_method("outputWriter", "()Ljava/io/BufferedWriter;"),
    );
    methods.insert(
        "outputWriter:(Ljava/nio/charset/Charset;)Ljava/io/BufferedWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "outputWriter",
            "(Ljava/nio/charset/Charset;)Ljava/io/BufferedWriter;",
        ),
    );

    StubClass {
        name: "java/lang/Process".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.ProcessBuilder class stub
pub fn create_process_builder_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/util/List;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/List;)V"),
    );
    methods.insert(
        "<init>:([Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("([Ljava/lang/String;)V"),
    );

    // Command manipulation
    methods.insert(
        "command:(Ljava/util/List;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "command",
            "(Ljava/util/List;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "command:([Ljava/lang/String;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "command",
            "([Ljava/lang/String;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "command:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("command", "()Ljava/util/List;"),
    );

    // Environment
    methods.insert(
        "environment:()Ljava/util/Map;".to_string(),
        StubMethodDescriptor::instance_method("environment", "()Ljava/util/Map;"),
    );

    // Working directory
    methods.insert(
        "directory:(Ljava/io/File;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "directory",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "directory:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("directory", "()Ljava/io/File;"),
    );

    // Error stream redirection
    methods.insert(
        "redirectErrorStream:(Z)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectErrorStream",
            "(Z)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectErrorStream:()Z".to_string(),
        StubMethodDescriptor::instance_method("redirectErrorStream", "()Z"),
    );

    // Start process
    methods.insert(
        "start:()Ljava/lang/Process;".to_string(),
        StubMethodDescriptor::instance_method("start", "()Ljava/lang/Process;"),
    );

    // Static startPipeline (Java 9+)
    methods.insert(
        "startPipeline:(Ljava/util/List;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("startPipeline", "(Ljava/util/List;)Ljava/util/List;"),
    );

    // I/O redirection
    methods.insert(
        "inheritIO:()Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method("inheritIO", "()Ljava/lang/ProcessBuilder;"),
    );
    methods.insert(
        "redirectInput:(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectInput",
            "(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectInput:(Ljava/io/File;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectInput",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectInput:()Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectInput",
            "()Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );
    methods.insert(
        "redirectOutput:(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "redirectOutput",
            "(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectOutput:(Ljava/io/File;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectOutput",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectOutput:()Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectOutput",
            "()Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );
    methods.insert(
        "redirectError:(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectError",
            "(Ljava/lang/ProcessBuilder$Redirect;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectError:(Ljava/io/File;)Ljava/lang/ProcessBuilder;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectError",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder;",
        ),
    );
    methods.insert(
        "redirectError:()Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::instance_method(
            "redirectError",
            "()Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );

    StubClass {
        name: "java/lang/ProcessBuilder".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.ProcessBuilder$Redirect class stub
pub fn create_process_builder_redirect_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "from:(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::static_method(
            "from",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );
    methods.insert(
        "to:(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::static_method(
            "to",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );
    methods.insert(
        "appendTo:(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;".to_string(),
        StubMethodDescriptor::static_method(
            "appendTo",
            "(Ljava/io/File;)Ljava/lang/ProcessBuilder$Redirect;",
        ),
    );

    // Instance methods
    methods.insert(
        "type:()Ljava/lang/ProcessBuilder$Redirect$Type;".to_string(),
        StubMethodDescriptor::instance_method("type", "()Ljava/lang/ProcessBuilder$Redirect$Type;"),
    );
    methods.insert(
        "file:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("file", "()Ljava/io/File;"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );

    StubClass {
        name: "java/lang/ProcessBuilder$Redirect".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.ProcessHandle interface stub
pub fn create_process_handle_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Static methods
    methods.insert(
        "current:()Ljava/lang/ProcessHandle;".to_string(),
        StubMethodDescriptor::static_method("current", "()Ljava/lang/ProcessHandle;"),
    );
    methods.insert(
        "of:(J)Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::static_method("of", "(J)Ljava/util/Optional;"),
    );
    methods.insert(
        "allProcesses:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method("allProcesses", "()Ljava/util/stream/Stream;"),
    );

    // Instance methods
    methods.insert(
        "pid:()J".to_string(),
        StubMethodDescriptor::instance_method("pid", "()J"),
    );
    methods.insert(
        "parent:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("parent", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "children:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("children", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "descendants:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("descendants", "()Ljava/util/stream/Stream;"),
    );
    methods.insert(
        "info:()Ljava/lang/ProcessHandle$Info;".to_string(),
        StubMethodDescriptor::instance_method("info", "()Ljava/lang/ProcessHandle$Info;"),
    );
    methods.insert(
        "isAlive:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAlive", "()Z"),
    );
    methods.insert(
        "destroy:()Z".to_string(),
        StubMethodDescriptor::instance_method("destroy", "()Z"),
    );
    methods.insert(
        "destroyForcibly:()Z".to_string(),
        StubMethodDescriptor::instance_method("destroyForcibly", "()Z"),
    );
    methods.insert(
        "supportsNormalTermination:()Z".to_string(),
        StubMethodDescriptor::instance_method("supportsNormalTermination", "()Z"),
    );
    methods.insert(
        "onExit:()Ljava/util/concurrent/CompletableFuture;".to_string(),
        StubMethodDescriptor::instance_method(
            "onExit",
            "()Ljava/util/concurrent/CompletableFuture;",
        ),
    );
    methods.insert(
        "compareTo:(Ljava/lang/ProcessHandle;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/ProcessHandle;)I"),
    );

    StubClass {
        name: "java/lang/ProcessHandle".to_string(),
        superclass: None,
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.lang.ProcessHandle$Info interface stub
pub fn create_process_handle_info_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "command:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("command", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "commandLine:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("commandLine", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "arguments:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("arguments", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "startInstant:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("startInstant", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "totalCpuDuration:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("totalCpuDuration", "()Ljava/util/Optional;"),
    );
    methods.insert(
        "user:()Ljava/util/Optional;".to_string(),
        StubMethodDescriptor::instance_method("user", "()Ljava/util/Optional;"),
    );

    StubClass {
        name: "java/lang/ProcessHandle$Info".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.ClassLoader class stub (abstract class)
pub fn create_classloader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors (protected in real JDK)
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ClassLoader;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ClassLoader;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/ClassLoader;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/ClassLoader;)V"),
    );

    // Class loading methods
    methods.insert(
        "loadClass:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("loadClass", "(Ljava/lang/String;)Ljava/lang/Class;"),
    );
    methods.insert(
        "loadClass:(Ljava/lang/String;Z)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "loadClass",
            "(Ljava/lang/String;Z)Ljava/lang/Class;",
        ),
    );
    methods.insert(
        "findClass:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("findClass", "(Ljava/lang/String;)Ljava/lang/Class;"),
    );
    methods.insert(
        "defineClass:(Ljava/lang/String;[BII)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "defineClass",
            "(Ljava/lang/String;[BII)Ljava/lang/Class;",
        ),
    );
    methods.insert(
        "defineClass:(Ljava/lang/String;Ljava/nio/ByteBuffer;Ljava/security/ProtectionDomain;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("defineClass", "(Ljava/lang/String;Ljava/nio/ByteBuffer;Ljava/security/ProtectionDomain;)Ljava/lang/Class;"),
    );
    methods.insert(
        "resolveClass:(Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::instance_method("resolveClass", "(Ljava/lang/Class;)V"),
    );
    methods.insert(
        "findLoadedClass:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "findLoadedClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
        ),
    );
    methods.insert(
        "setSigners:(Ljava/lang/Class;[Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setSigners",
            "(Ljava/lang/Class;[Ljava/lang/Object;)V",
        ),
    );

    // Hierarchy methods
    methods.insert(
        "getParent:()Ljava/lang/ClassLoader;".to_string(),
        StubMethodDescriptor::instance_method("getParent", "()Ljava/lang/ClassLoader;"),
    );
    methods.insert(
        "getSystemClassLoader:()Ljava/lang/ClassLoader;".to_string(),
        StubMethodDescriptor::static_method("getSystemClassLoader", "()Ljava/lang/ClassLoader;"),
    );
    methods.insert(
        "getPlatformClassLoader:()Ljava/lang/ClassLoader;".to_string(),
        StubMethodDescriptor::static_method("getPlatformClassLoader", "()Ljava/lang/ClassLoader;"),
    );

    // Resource methods
    methods.insert(
        "getResource:(Ljava/lang/String;)Ljava/net/URL;".to_string(),
        StubMethodDescriptor::instance_method("getResource", "(Ljava/lang/String;)Ljava/net/URL;"),
    );
    methods.insert(
        "getResources:(Ljava/lang/String;)Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::instance_method(
            "getResources",
            "(Ljava/lang/String;)Ljava/util/Enumeration;",
        ),
    );
    methods.insert(
        "getResourceAsStream:(Ljava/lang/String;)Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "getResourceAsStream",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
        ),
    );
    methods.insert(
        "getSystemResource:(Ljava/lang/String;)Ljava/net/URL;".to_string(),
        StubMethodDescriptor::static_method(
            "getSystemResource",
            "(Ljava/lang/String;)Ljava/net/URL;",
        ),
    );
    methods.insert(
        "getSystemResources:(Ljava/lang/String;)Ljava/util/Enumeration;".to_string(),
        StubMethodDescriptor::static_method(
            "getSystemResources",
            "(Ljava/lang/String;)Ljava/util/Enumeration;",
        ),
    );
    methods.insert(
        "getSystemResourceAsStream:(Ljava/lang/String;)Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::static_method(
            "getSystemResourceAsStream",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
        ),
    );

    // Package methods
    methods.insert(
        "definePackage:(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/net/URL;)Ljava/lang/Package;".to_string(),
        StubMethodDescriptor::instance_method("definePackage", "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/net/URL;)Ljava/lang/Package;"),
    );
    methods.insert(
        "getPackage:(Ljava/lang/String;)Ljava/lang/Package;".to_string(),
        StubMethodDescriptor::instance_method(
            "getPackage",
            "(Ljava/lang/String;)Ljava/lang/Package;",
        ),
    );
    methods.insert(
        "getPackages:()[Ljava/lang/Package;".to_string(),
        StubMethodDescriptor::instance_method("getPackages", "()[Ljava/lang/Package;"),
    );
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );

    // Assertion control methods
    methods.insert(
        "setDefaultAssertionStatus:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("setDefaultAssertionStatus", "(Z)V"),
    );
    methods.insert(
        "setPackageAssertionStatus:(Ljava/lang/String;Z)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setPackageAssertionStatus",
            "(Ljava/lang/String;Z)V",
        ),
    );
    methods.insert(
        "setClassAssertionStatus:(Ljava/lang/String;Z)V".to_string(),
        StubMethodDescriptor::instance_method("setClassAssertionStatus", "(Ljava/lang/String;Z)V"),
    );
    methods.insert(
        "clearAssertionStatus:()V".to_string(),
        StubMethodDescriptor::instance_method("clearAssertionStatus", "()V"),
    );

    StubClass {
        name: "java/lang/ClassLoader".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.ThreadLocal class stub
pub fn create_threadlocal_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Instance methods
    methods.insert(
        "initialValue:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("initialValue", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "remove:()V".to_string(),
        StubMethodDescriptor::instance_method("remove", "()V"),
    );

    // Static factory method (Java 8+)
    methods.insert(
        "withInitial:(Ljava/util/function/Supplier;)Ljava/lang/ThreadLocal;".to_string(),
        StubMethodDescriptor::static_method(
            "withInitial",
            "(Ljava/util/function/Supplier;)Ljava/lang/ThreadLocal;",
        ),
    );

    StubClass {
        name: "java/lang/ThreadLocal".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.lang.InheritableThreadLocal class stub (extends ThreadLocal)
pub fn create_inheritable_threadlocal_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Instance method (in addition to inherited methods from ThreadLocal)
    methods.insert(
        "childValue:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "childValue",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "java/lang/InheritableThreadLocal".to_string(),
        superclass: Some("java/lang/ThreadLocal".to_string()),
        interfaces: vec![],
        methods,
    }
}
