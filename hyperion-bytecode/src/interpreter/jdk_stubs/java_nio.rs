// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.nio package stub implementations
//!
//! NIO (New I/O) API introduced in Java 1.4 and enhanced in Java 7 (NIO.2).
//! Critical for modern file operations, especially in Gradle builds.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

// ==================== java.nio.file Package ====================

/// Create java.nio.file.Path interface stub
pub fn create_path_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Path manipulation methods
    methods.insert(
        "getFileName:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("getFileName", "()Ljava/nio/file/Path;"),
    );
    methods.insert(
        "getParent:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("getParent", "()Ljava/nio/file/Path;"),
    );
    methods.insert(
        "getRoot:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("getRoot", "()Ljava/nio/file/Path;"),
    );
    methods.insert(
        "getNameCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getNameCount", "()I"),
    );
    methods.insert(
        "getName:(I)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("getName", "(I)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "subpath:(II)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("subpath", "(II)Ljava/nio/file/Path;"),
    );

    // Path matching
    methods.insert(
        "startsWith:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::instance_method("startsWith", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "startsWith:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("startsWith", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "endsWith:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::instance_method("endsWith", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "endsWith:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("endsWith", "(Ljava/lang/String;)Z"),
    );

    // Path resolution
    methods.insert(
        "normalize:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("normalize", "()Ljava/nio/file/Path;"),
    );
    methods.insert(
        "resolve:(Ljava/nio/file/Path;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "resolve",
            "(Ljava/nio/file/Path;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "resolve:(Ljava/lang/String;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "resolve",
            "(Ljava/lang/String;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "resolveSibling:(Ljava/nio/file/Path;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "resolveSibling",
            "(Ljava/nio/file/Path;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "resolveSibling:(Ljava/lang/String;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "resolveSibling",
            "(Ljava/lang/String;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "relativize:(Ljava/nio/file/Path;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "relativize",
            "(Ljava/nio/file/Path;)Ljava/nio/file/Path;",
        ),
    );

    // Conversions
    methods.insert(
        "toUri:()Ljava/net/URI;".to_string(),
        StubMethodDescriptor::instance_method("toUri", "()Ljava/net/URI;"),
    );
    methods.insert(
        "toAbsolutePath:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("toAbsolutePath", "()Ljava/nio/file/Path;"),
    );
    methods.insert(
        "toRealPath:([Ljava/nio/file/LinkOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "toRealPath",
            "([Ljava/nio/file/LinkOption;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "toFile:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("toFile", "()Ljava/io/File;"),
    );

    // Comparison and representation
    methods.insert(
        "compareTo:(Ljava/nio/file/Path;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/nio/file/Path;)I"),
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

    // Iterator
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );

    StubClass {
        name: "java/nio/file/Path".to_string(),
        superclass: None,
        interfaces: vec![
            "java/lang/Comparable".to_string(),
            "java/lang/Iterable".to_string(),
        ],
        methods,
    }
}

/// Create java.nio.file.Paths factory class stub
pub fn create_paths_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "get:(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method(
            "get",
            "(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "get:(Ljava/net/URI;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("get", "(Ljava/net/URI;)Ljava/nio/file/Path;"),
    );

    StubClass {
        name: "java/nio/file/Paths".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.Files utility class stub
pub fn create_files_class() -> StubClass {
    let mut methods = HashMap::new();

    // Copy/move/delete operations
    methods.insert(
        "copy:(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("copy", "(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "move:(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("move", "(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "delete:(Ljava/nio/file/Path;)V".to_string(),
        StubMethodDescriptor::static_method("delete", "(Ljava/nio/file/Path;)V"),
    );
    methods.insert(
        "deleteIfExists:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("deleteIfExists", "(Ljava/nio/file/Path;)Z"),
    );

    // Directory creation
    methods.insert(
        "createDirectory:(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createDirectory", "(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "createDirectories:(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createDirectories", "(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );

    // File creation
    methods.insert(
        "createFile:(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createFile", "(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "createTempFile:(Ljava/nio/file/Path;Ljava/lang/String;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createTempFile", "(Ljava/nio/file/Path;Ljava/lang/String;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "createTempFile:(Ljava/lang/String;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createTempFile", "(Ljava/lang/String;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "createTempDirectory:(Ljava/nio/file/Path;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createTempDirectory", "(Ljava/nio/file/Path;Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "createTempDirectory:(Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("createTempDirectory", "(Ljava/lang/String;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/file/Path;"),
    );

    // File existence/type checks
    methods.insert(
        "exists:(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "exists",
            "(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z",
        ),
    );
    methods.insert(
        "notExists:(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "notExists",
            "(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z",
        ),
    );
    methods.insert(
        "isDirectory:(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "isDirectory",
            "(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z",
        ),
    );
    methods.insert(
        "isRegularFile:(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "isRegularFile",
            "(Ljava/nio/file/Path;[Ljava/nio/file/LinkOption;)Z",
        ),
    );
    methods.insert(
        "isSymbolicLink:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("isSymbolicLink", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "isHidden:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("isHidden", "(Ljava/nio/file/Path;)Z"),
    );

    // Permission checks
    methods.insert(
        "isReadable:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("isReadable", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "isWritable:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("isWritable", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "isExecutable:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method("isExecutable", "(Ljava/nio/file/Path;)Z"),
    );

    // File comparison
    methods.insert(
        "isSameFile:(Ljava/nio/file/Path;Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::static_method(
            "isSameFile",
            "(Ljava/nio/file/Path;Ljava/nio/file/Path;)Z",
        ),
    );

    // File size
    methods.insert(
        "size:(Ljava/nio/file/Path;)J".to_string(),
        StubMethodDescriptor::static_method("size", "(Ljava/nio/file/Path;)J"),
    );

    // Read operations
    methods.insert(
        "readAllBytes:(Ljava/nio/file/Path;)[B".to_string(),
        StubMethodDescriptor::static_method("readAllBytes", "(Ljava/nio/file/Path;)[B"),
    );
    methods.insert(
        "readString:(Ljava/nio/file/Path;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "readString",
            "(Ljava/nio/file/Path;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "readString:(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method(
            "readString",
            "(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/lang/String;",
        ),
    );
    methods.insert(
        "readAllLines:(Ljava/nio/file/Path;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method(
            "readAllLines",
            "(Ljava/nio/file/Path;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "readAllLines:(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method(
            "readAllLines",
            "(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/util/List;",
        ),
    );

    // Write operations
    methods.insert(
        "write:(Ljava/nio/file/Path;[B[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method(
            "write",
            "(Ljava/nio/file/Path;[B[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "writeString:(Ljava/nio/file/Path;Ljava/lang/CharSequence;[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("writeString", "(Ljava/nio/file/Path;Ljava/lang/CharSequence;[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "writeString:(Ljava/nio/file/Path;Ljava/lang/CharSequence;Ljava/nio/charset/Charset;[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::static_method("writeString", "(Ljava/nio/file/Path;Ljava/lang/CharSequence;Ljava/nio/charset/Charset;[Ljava/nio/file/OpenOption;)Ljava/nio/file/Path;"),
    );

    // Stream and reader/writer creation
    methods.insert(
        "newBufferedReader:(Ljava/nio/file/Path;)Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::static_method(
            "newBufferedReader",
            "(Ljava/nio/file/Path;)Ljava/io/BufferedReader;",
        ),
    );
    methods.insert(
        "newBufferedReader:(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;".to_string(),
        StubMethodDescriptor::static_method("newBufferedReader", "(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/io/BufferedReader;"),
    );
    methods.insert(
        "newBufferedWriter:(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/BufferedWriter;".to_string(),
        StubMethodDescriptor::static_method("newBufferedWriter", "(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/BufferedWriter;"),
    );
    methods.insert(
        "newBufferedWriter:(Ljava/nio/file/Path;Ljava/nio/charset/Charset;[Ljava/nio/file/OpenOption;)Ljava/io/BufferedWriter;".to_string(),
        StubMethodDescriptor::static_method("newBufferedWriter", "(Ljava/nio/file/Path;Ljava/nio/charset/Charset;[Ljava/nio/file/OpenOption;)Ljava/io/BufferedWriter;"),
    );
    methods.insert(
        "newInputStream:(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/InputStream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newInputStream",
            "(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/InputStream;",
        ),
    );
    methods.insert(
        "newOutputStream:(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/OutputStream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newOutputStream",
            "(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/io/OutputStream;",
        ),
    );

    // Directory streams
    methods.insert(
        "list:(Ljava/nio/file/Path;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "list",
            "(Ljava/nio/file/Path;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "walk:(Ljava/nio/file/Path;[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "walk",
            "(Ljava/nio/file/Path;[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "walk:(Ljava/nio/file/Path;I[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "walk",
            "(Ljava/nio/file/Path;I[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "find:(Ljava/nio/file/Path;ILjava/util/function/BiPredicate;[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method("find", "(Ljava/nio/file/Path;ILjava/util/function/BiPredicate;[Ljava/nio/file/FileVisitOption;)Ljava/util/stream/Stream;"),
    );

    // Line streams
    methods.insert(
        "lines:(Ljava/nio/file/Path;)Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::static_method(
            "lines",
            "(Ljava/nio/file/Path;)Ljava/util/stream/Stream;",
        ),
    );
    methods.insert(
        "lines:(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/util/stream/Stream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "lines",
            "(Ljava/nio/file/Path;Ljava/nio/charset/Charset;)Ljava/util/stream/Stream;",
        ),
    );

    StubClass {
        name: "java/nio/file/Files".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ==================== java.nio Buffer Classes ====================

/// Create java.nio.ByteBuffer class stub
pub fn create_byte_buffer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "allocate:(I)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::static_method("allocate", "(I)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "allocateDirect:(I)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::static_method("allocateDirect", "(I)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "wrap:([B)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::static_method("wrap", "([B)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "wrap:([BII)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::static_method("wrap", "([BII)Ljava/nio/ByteBuffer;"),
    );

    // Get methods (relative)
    methods.insert(
        "get:()B".to_string(),
        StubMethodDescriptor::instance_method("get", "()B"),
    );
    methods.insert(
        "get:([B)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("get", "([B)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "get:([BII)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("get", "([BII)Ljava/nio/ByteBuffer;"),
    );

    // Get methods (absolute)
    methods.insert(
        "get:(I)B".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)B"),
    );

    // Put methods (relative)
    methods.insert(
        "put:(B)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "(B)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "put:([B)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "([B)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "put:([BII)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "([BII)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "put:(Ljava/nio/ByteBuffer;)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "put",
            "(Ljava/nio/ByteBuffer;)Ljava/nio/ByteBuffer;",
        ),
    );

    // Put methods (absolute)
    methods.insert(
        "put:(IB)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "(IB)Ljava/nio/ByteBuffer;"),
    );

    // Primitive type views
    methods.insert(
        "getChar:()C".to_string(),
        StubMethodDescriptor::instance_method("getChar", "()C"),
    );
    methods.insert(
        "getChar:(I)C".to_string(),
        StubMethodDescriptor::instance_method("getChar", "(I)C"),
    );
    methods.insert(
        "putChar:(C)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putChar", "(C)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putChar:(IC)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putChar", "(IC)Ljava/nio/ByteBuffer;"),
    );

    methods.insert(
        "getShort:()S".to_string(),
        StubMethodDescriptor::instance_method("getShort", "()S"),
    );
    methods.insert(
        "getShort:(I)S".to_string(),
        StubMethodDescriptor::instance_method("getShort", "(I)S"),
    );
    methods.insert(
        "putShort:(S)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putShort", "(S)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putShort:(IS)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putShort", "(IS)Ljava/nio/ByteBuffer;"),
    );

    methods.insert(
        "getInt:()I".to_string(),
        StubMethodDescriptor::instance_method("getInt", "()I"),
    );
    methods.insert(
        "getInt:(I)I".to_string(),
        StubMethodDescriptor::instance_method("getInt", "(I)I"),
    );
    methods.insert(
        "putInt:(I)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putInt", "(I)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putInt:(II)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putInt", "(II)Ljava/nio/ByteBuffer;"),
    );

    methods.insert(
        "getLong:()J".to_string(),
        StubMethodDescriptor::instance_method("getLong", "()J"),
    );
    methods.insert(
        "getLong:(I)J".to_string(),
        StubMethodDescriptor::instance_method("getLong", "(I)J"),
    );
    methods.insert(
        "putLong:(J)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putLong", "(J)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putLong:(IJ)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putLong", "(IJ)Ljava/nio/ByteBuffer;"),
    );

    methods.insert(
        "getFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("getFloat", "()F"),
    );
    methods.insert(
        "getFloat:(I)F".to_string(),
        StubMethodDescriptor::instance_method("getFloat", "(I)F"),
    );
    methods.insert(
        "putFloat:(F)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putFloat", "(F)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putFloat:(IF)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putFloat", "(IF)Ljava/nio/ByteBuffer;"),
    );

    methods.insert(
        "getDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("getDouble", "()D"),
    );
    methods.insert(
        "getDouble:(I)D".to_string(),
        StubMethodDescriptor::instance_method("getDouble", "(I)D"),
    );
    methods.insert(
        "putDouble:(D)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putDouble", "(D)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "putDouble:(ID)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("putDouble", "(ID)Ljava/nio/ByteBuffer;"),
    );

    // Buffer state manipulation
    methods.insert(
        "flip:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("flip", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "clear:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("clear", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "rewind:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("rewind", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "compact:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("compact", "()Ljava/nio/ByteBuffer;"),
    );

    // Buffer properties
    methods.insert(
        "hasRemaining:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasRemaining", "()Z"),
    );
    methods.insert(
        "remaining:()I".to_string(),
        StubMethodDescriptor::instance_method("remaining", "()I"),
    );
    methods.insert(
        "capacity:()I".to_string(),
        StubMethodDescriptor::instance_method("capacity", "()I"),
    );
    methods.insert(
        "position:()I".to_string(),
        StubMethodDescriptor::instance_method("position", "()I"),
    );
    methods.insert(
        "position:(I)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("position", "(I)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "limit:()I".to_string(),
        StubMethodDescriptor::instance_method("limit", "()I"),
    );
    methods.insert(
        "limit:(I)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("limit", "(I)Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "mark:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("mark", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "reset:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("reset", "()Ljava/nio/ByteBuffer;"),
    );

    // Array access
    methods.insert(
        "array:()[B".to_string(),
        StubMethodDescriptor::instance_method("array", "()[B"),
    );
    methods.insert(
        "hasArray:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasArray", "()Z"),
    );
    methods.insert(
        "arrayOffset:()I".to_string(),
        StubMethodDescriptor::instance_method("arrayOffset", "()I"),
    );

    // Buffer characteristics
    methods.insert(
        "isDirect:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDirect", "()Z"),
    );
    methods.insert(
        "isReadOnly:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReadOnly", "()Z"),
    );

    // Buffer slicing
    methods.insert(
        "duplicate:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("duplicate", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "slice:()Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("slice", "()Ljava/nio/ByteBuffer;"),
    );
    methods.insert(
        "slice:(II)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method("slice", "(II)Ljava/nio/ByteBuffer;"),
    );

    // Byte order
    methods.insert(
        "order:()Ljava/nio/ByteOrder;".to_string(),
        StubMethodDescriptor::instance_method("order", "()Ljava/nio/ByteOrder;"),
    );
    methods.insert(
        "order:(Ljava/nio/ByteOrder;)Ljava/nio/ByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "order",
            "(Ljava/nio/ByteOrder;)Ljava/nio/ByteBuffer;",
        ),
    );

    // Comparison
    methods.insert(
        "compareTo:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/nio/ByteBuffer;)I"),
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
        name: "java/nio/ByteBuffer".to_string(),
        superclass: Some("java/nio/Buffer".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.nio.CharBuffer class stub
pub fn create_char_buffer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "allocate:(I)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::static_method("allocate", "(I)Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "wrap:([C)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::static_method("wrap", "([C)Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "wrap:([CII)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::static_method("wrap", "([CII)Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "wrap:(Ljava/lang/CharSequence;)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::static_method(
            "wrap",
            "(Ljava/lang/CharSequence;)Ljava/nio/CharBuffer;",
        ),
    );
    methods.insert(
        "wrap:(Ljava/lang/CharSequence;II)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::static_method(
            "wrap",
            "(Ljava/lang/CharSequence;II)Ljava/nio/CharBuffer;",
        ),
    );

    // Get/put methods
    methods.insert(
        "get:()C".to_string(),
        StubMethodDescriptor::instance_method("get", "()C"),
    );
    methods.insert(
        "get:(I)C".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)C"),
    );
    methods.insert(
        "put:(C)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "(C)Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "put:(IC)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("put", "(IC)Ljava/nio/CharBuffer;"),
    );

    // CharSequence methods
    methods.insert(
        "charAt:(I)C".to_string(),
        StubMethodDescriptor::instance_method("charAt", "(I)C"),
    );
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "subSequence:(II)Ljava/lang/CharSequence;".to_string(),
        StubMethodDescriptor::instance_method("subSequence", "(II)Ljava/lang/CharSequence;"),
    );

    // Appendable methods
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/nio/CharBuffer;",
        ),
    );
    methods.insert(
        "append:(C)Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/nio/CharBuffer;"),
    );

    // Buffer operations
    methods.insert(
        "flip:()Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("flip", "()Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "clear:()Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("clear", "()Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "rewind:()Ljava/nio/CharBuffer;".to_string(),
        StubMethodDescriptor::instance_method("rewind", "()Ljava/nio/CharBuffer;"),
    );
    methods.insert(
        "array:()[C".to_string(),
        StubMethodDescriptor::instance_method("array", "()[C"),
    );
    methods.insert(
        "hasArray:()Z".to_string(),
        StubMethodDescriptor::instance_method("hasArray", "()Z"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/nio/CharBuffer".to_string(),
        superclass: Some("java/nio/Buffer".to_string()),
        interfaces: vec![
            "java/lang/Comparable".to_string(),
            "java/lang/Appendable".to_string(),
            "java/lang/CharSequence".to_string(),
            "java/lang/Readable".to_string(),
        ],
        methods,
    }
}

// ==================== java.nio.channels Package ====================

/// Create java.nio.channels.FileChannel class stub
pub fn create_file_channel_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "open:(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/nio/channels/FileChannel;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "open",
            "(Ljava/nio/file/Path;[Ljava/nio/file/OpenOption;)Ljava/nio/channels/FileChannel;",
        ),
    );
    methods.insert(
        "open:(Ljava/nio/file/Path;Ljava/util/Set;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/channels/FileChannel;".to_string(),
        StubMethodDescriptor::static_method("open", "(Ljava/nio/file/Path;Ljava/util/Set;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/channels/FileChannel;"),
    );

    // Read/write operations
    methods.insert(
        "read:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("read", "(Ljava/nio/ByteBuffer;)I"),
    );
    methods.insert(
        "read:(Ljava/nio/ByteBuffer;J)I".to_string(),
        StubMethodDescriptor::instance_method("read", "(Ljava/nio/ByteBuffer;J)I"),
    );
    methods.insert(
        "read:([Ljava/nio/ByteBuffer;)J".to_string(),
        StubMethodDescriptor::instance_method("read", "([Ljava/nio/ByteBuffer;)J"),
    );
    methods.insert(
        "write:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/nio/ByteBuffer;)I"),
    );
    methods.insert(
        "write:(Ljava/nio/ByteBuffer;J)I".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/nio/ByteBuffer;J)I"),
    );
    methods.insert(
        "write:([Ljava/nio/ByteBuffer;)J".to_string(),
        StubMethodDescriptor::instance_method("write", "([Ljava/nio/ByteBuffer;)J"),
    );

    // Position and size
    methods.insert(
        "position:()J".to_string(),
        StubMethodDescriptor::instance_method("position", "()J"),
    );
    methods.insert(
        "position:(J)Ljava/nio/channels/FileChannel;".to_string(),
        StubMethodDescriptor::instance_method("position", "(J)Ljava/nio/channels/FileChannel;"),
    );
    methods.insert(
        "size:()J".to_string(),
        StubMethodDescriptor::instance_method("size", "()J"),
    );
    methods.insert(
        "truncate:(J)Ljava/nio/channels/FileChannel;".to_string(),
        StubMethodDescriptor::instance_method("truncate", "(J)Ljava/nio/channels/FileChannel;"),
    );

    // Force and lock
    methods.insert(
        "force:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("force", "(Z)V"),
    );
    methods.insert(
        "lock:()Ljava/nio/channels/FileLock;".to_string(),
        StubMethodDescriptor::instance_method("lock", "()Ljava/nio/channels/FileLock;"),
    );
    methods.insert(
        "lock:(JJZ)Ljava/nio/channels/FileLock;".to_string(),
        StubMethodDescriptor::instance_method("lock", "(JJZ)Ljava/nio/channels/FileLock;"),
    );
    methods.insert(
        "tryLock:()Ljava/nio/channels/FileLock;".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "()Ljava/nio/channels/FileLock;"),
    );
    methods.insert(
        "tryLock:(JJZ)Ljava/nio/channels/FileLock;".to_string(),
        StubMethodDescriptor::instance_method("tryLock", "(JJZ)Ljava/nio/channels/FileLock;"),
    );

    // Transfer
    methods.insert(
        "transferTo:(JJLjava/nio/channels/WritableByteChannel;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "transferTo",
            "(JJLjava/nio/channels/WritableByteChannel;)J",
        ),
    );
    methods.insert(
        "transferFrom:(Ljava/nio/channels/ReadableByteChannel;JJ)J".to_string(),
        StubMethodDescriptor::instance_method(
            "transferFrom",
            "(Ljava/nio/channels/ReadableByteChannel;JJ)J",
        ),
    );

    // Memory mapping
    methods.insert(
        "map:(Ljava/nio/channels/FileChannel$MapMode;JJ)Ljava/nio/MappedByteBuffer;".to_string(),
        StubMethodDescriptor::instance_method(
            "map",
            "(Ljava/nio/channels/FileChannel$MapMode;JJ)Ljava/nio/MappedByteBuffer;",
        ),
    );

    // Channel operations
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "isOpen:()Z".to_string(),
        StubMethodDescriptor::instance_method("isOpen", "()Z"),
    );

    StubClass {
        name: "java/nio/channels/FileChannel".to_string(),
        superclass: Some("java/nio/channels/spi/AbstractInterruptibleChannel".to_string()),
        interfaces: vec![
            "java/nio/channels/SeekableByteChannel".to_string(),
            "java/nio/channels/GatheringByteChannel".to_string(),
            "java/nio/channels/ScatteringByteChannel".to_string(),
        ],
        methods,
    }
}

// ==================== java.nio.file Enums and Options ====================

/// Create java.nio.file.StandardOpenOption enum stub
pub fn create_standard_open_option_enum() -> StubClass {
    let mut methods = HashMap::new();

    // Enum methods
    methods.insert(
        "values:()[Ljava/nio/file/StandardOpenOption;".to_string(),
        StubMethodDescriptor::static_method("values", "()[Ljava/nio/file/StandardOpenOption;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/nio/file/StandardOpenOption;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/String;)Ljava/nio/file/StandardOpenOption;",
        ),
    );

    StubClass {
        name: "java/nio/file/StandardOpenOption".to_string(),
        superclass: Some("java/lang/Enum".to_string()),
        interfaces: vec!["java/nio/file/OpenOption".to_string()],
        methods,
    }
}

/// Create java.nio.ByteOrder class stub
pub fn create_byte_order_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static fields (represented as methods for stubbing)
    methods.insert(
        "BIG_ENDIAN:()Ljava/nio/ByteOrder;".to_string(),
        StubMethodDescriptor::static_method("BIG_ENDIAN", "()Ljava/nio/ByteOrder;"),
    );
    methods.insert(
        "LITTLE_ENDIAN:()Ljava/nio/ByteOrder;".to_string(),
        StubMethodDescriptor::static_method("LITTLE_ENDIAN", "()Ljava/nio/ByteOrder;"),
    );

    // Static method
    methods.insert(
        "nativeOrder:()Ljava/nio/ByteOrder;".to_string(),
        StubMethodDescriptor::static_method("nativeOrder", "()Ljava/nio/ByteOrder;"),
    );

    // Instance method
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/nio/ByteOrder".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ==================== java.nio.file FileSystem Classes ====================

/// Create java.nio.file.FileSystem abstract class stub
pub fn create_file_system_class() -> StubClass {
    let mut methods = HashMap::new();

    // Protected constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Abstract methods
    methods.insert(
        "provider:()Ljava/nio/file/spi/FileSystemProvider;".to_string(),
        StubMethodDescriptor::instance_method(
            "provider",
            "()Ljava/nio/file/spi/FileSystemProvider;",
        ),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "isOpen:()Z".to_string(),
        StubMethodDescriptor::instance_method("isOpen", "()Z"),
    );
    methods.insert(
        "isReadOnly:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReadOnly", "()Z"),
    );
    methods.insert(
        "getSeparator:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getSeparator", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getRootDirectories:()Ljava/lang/Iterable;".to_string(),
        StubMethodDescriptor::instance_method("getRootDirectories", "()Ljava/lang/Iterable;"),
    );
    methods.insert(
        "getFileStores:()Ljava/lang/Iterable;".to_string(),
        StubMethodDescriptor::instance_method("getFileStores", "()Ljava/lang/Iterable;"),
    );
    methods.insert(
        "supportedFileAttributeViews:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("supportedFileAttributeViews", "()Ljava/util/Set;"),
    );
    methods.insert(
        "getPath:(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method(
            "getPath",
            "(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;",
        ),
    );
    methods.insert(
        "getPathMatcher:(Ljava/lang/String;)Ljava/nio/file/PathMatcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "getPathMatcher",
            "(Ljava/lang/String;)Ljava/nio/file/PathMatcher;",
        ),
    );
    methods.insert(
        "getUserPrincipalLookupService:()Ljava/nio/file/attribute/UserPrincipalLookupService;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getUserPrincipalLookupService",
            "()Ljava/nio/file/attribute/UserPrincipalLookupService;",
        ),
    );
    methods.insert(
        "newWatchService:()Ljava/nio/file/WatchService;".to_string(),
        StubMethodDescriptor::instance_method("newWatchService", "()Ljava/nio/file/WatchService;"),
    );

    StubClass {
        name: "java/nio/file/FileSystem".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Closeable".to_string()],
        methods,
    }
}

/// Create java.nio.file.FileSystems utility class stub
pub fn create_file_systems_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static factory methods
    methods.insert(
        "getDefault:()Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::static_method("getDefault", "()Ljava/nio/file/FileSystem;"),
    );
    methods.insert(
        "getFileSystem:(Ljava/net/URI;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::static_method(
            "getFileSystem",
            "(Ljava/net/URI;)Ljava/nio/file/FileSystem;",
        ),
    );
    methods.insert(
        "newFileSystem:(Ljava/net/URI;Ljava/util/Map;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::static_method(
            "newFileSystem",
            "(Ljava/net/URI;Ljava/util/Map;)Ljava/nio/file/FileSystem;",
        ),
    );
    methods.insert(
        "newFileSystem:(Ljava/net/URI;Ljava/util/Map;Ljava/lang/ClassLoader;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::static_method(
            "newFileSystem",
            "(Ljava/net/URI;Ljava/util/Map;Ljava/lang/ClassLoader;)Ljava/nio/file/FileSystem;",
        ),
    );
    // Java 13+ overloads
    methods.insert(
        "newFileSystem:(Ljava/nio/file/Path;Ljava/lang/ClassLoader;)Ljava/nio/file/FileSystem;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newFileSystem",
            "(Ljava/nio/file/Path;Ljava/lang/ClassLoader;)Ljava/nio/file/FileSystem;",
        ),
    );
    methods.insert(
        "newFileSystem:(Ljava/nio/file/Path;Ljava/util/Map;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::static_method(
            "newFileSystem",
            "(Ljava/nio/file/Path;Ljava/util/Map;)Ljava/nio/file/FileSystem;",
        ),
    );

    StubClass {
        name: "java/nio/file/FileSystems".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.spi.FileSystemProvider abstract class stub
pub fn create_file_system_provider_class() -> StubClass {
    let mut methods = HashMap::new();

    // Protected constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Static method
    methods.insert(
        "installedProviders:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::static_method("installedProviders", "()Ljava/util/List;"),
    );

    // Abstract methods
    methods.insert(
        "getScheme:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getScheme", "()Ljava/lang/String;"),
    );
    methods.insert(
        "newFileSystem:(Ljava/net/URI;Ljava/util/Map;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::instance_method(
            "newFileSystem",
            "(Ljava/net/URI;Ljava/util/Map;)Ljava/nio/file/FileSystem;",
        ),
    );
    methods.insert(
        "getFileSystem:(Ljava/net/URI;)Ljava/nio/file/FileSystem;".to_string(),
        StubMethodDescriptor::instance_method(
            "getFileSystem",
            "(Ljava/net/URI;)Ljava/nio/file/FileSystem;",
        ),
    );
    methods.insert(
        "getPath:(Ljava/net/URI;)Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("getPath", "(Ljava/net/URI;)Ljava/nio/file/Path;"),
    );
    methods.insert(
        "newByteChannel:(Ljava/nio/file/Path;Ljava/util/Set;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/channels/SeekableByteChannel;".to_string(),
        StubMethodDescriptor::instance_method(
            "newByteChannel",
            "(Ljava/nio/file/Path;Ljava/util/Set;[Ljava/nio/file/attribute/FileAttribute;)Ljava/nio/channels/SeekableByteChannel;",
        ),
    );
    methods.insert(
        "newDirectoryStream:(Ljava/nio/file/Path;Ljava/nio/file/DirectoryStream$Filter;)Ljava/nio/file/DirectoryStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "newDirectoryStream",
            "(Ljava/nio/file/Path;Ljava/nio/file/DirectoryStream$Filter;)Ljava/nio/file/DirectoryStream;",
        ),
    );
    methods.insert(
        "createDirectory:(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "createDirectory",
            "(Ljava/nio/file/Path;[Ljava/nio/file/attribute/FileAttribute;)V",
        ),
    );
    methods.insert(
        "delete:(Ljava/nio/file/Path;)V".to_string(),
        StubMethodDescriptor::instance_method("delete", "(Ljava/nio/file/Path;)V"),
    );
    methods.insert(
        "copy:(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "copy",
            "(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)V",
        ),
    );
    methods.insert(
        "move:(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "move",
            "(Ljava/nio/file/Path;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)V",
        ),
    );
    methods.insert(
        "isSameFile:(Ljava/nio/file/Path;Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::instance_method(
            "isSameFile",
            "(Ljava/nio/file/Path;Ljava/nio/file/Path;)Z",
        ),
    );
    methods.insert(
        "isHidden:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::instance_method("isHidden", "(Ljava/nio/file/Path;)Z"),
    );
    methods.insert(
        "getFileStore:(Ljava/nio/file/Path;)Ljava/nio/file/FileStore;".to_string(),
        StubMethodDescriptor::instance_method(
            "getFileStore",
            "(Ljava/nio/file/Path;)Ljava/nio/file/FileStore;",
        ),
    );
    methods.insert(
        "checkAccess:(Ljava/nio/file/Path;[Ljava/nio/file/AccessMode;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "checkAccess",
            "(Ljava/nio/file/Path;[Ljava/nio/file/AccessMode;)V",
        ),
    );
    methods.insert(
        "getFileAttributeView:(Ljava/nio/file/Path;Ljava/lang/Class;[Ljava/nio/file/LinkOption;)Ljava/nio/file/attribute/FileAttributeView;".to_string(),
        StubMethodDescriptor::instance_method(
            "getFileAttributeView",
            "(Ljava/nio/file/Path;Ljava/lang/Class;[Ljava/nio/file/LinkOption;)Ljava/nio/file/attribute/FileAttributeView;",
        ),
    );
    methods.insert(
        "readAttributes:(Ljava/nio/file/Path;Ljava/lang/Class;[Ljava/nio/file/LinkOption;)Ljava/nio/file/attribute/BasicFileAttributes;".to_string(),
        StubMethodDescriptor::instance_method(
            "readAttributes",
            "(Ljava/nio/file/Path;Ljava/lang/Class;[Ljava/nio/file/LinkOption;)Ljava/nio/file/attribute/BasicFileAttributes;",
        ),
    );
    methods.insert(
        "readAttributes:(Ljava/nio/file/Path;Ljava/lang/String;[Ljava/nio/file/LinkOption;)Ljava/util/Map;".to_string(),
        StubMethodDescriptor::instance_method(
            "readAttributes",
            "(Ljava/nio/file/Path;Ljava/lang/String;[Ljava/nio/file/LinkOption;)Ljava/util/Map;",
        ),
    );
    methods.insert(
        "setAttribute:(Ljava/nio/file/Path;Ljava/lang/String;Ljava/lang/Object;[Ljava/nio/file/LinkOption;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setAttribute",
            "(Ljava/nio/file/Path;Ljava/lang/String;Ljava/lang/Object;[Ljava/nio/file/LinkOption;)V",
        ),
    );

    StubClass {
        name: "java/nio/file/spi/FileSystemProvider".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.FileStore abstract class stub
pub fn create_file_store_class() -> StubClass {
    let mut methods = HashMap::new();

    // Protected constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Abstract methods
    methods.insert(
        "name:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("name", "()Ljava/lang/String;"),
    );
    methods.insert(
        "type:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("type", "()Ljava/lang/String;"),
    );
    methods.insert(
        "isReadOnly:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReadOnly", "()Z"),
    );
    methods.insert(
        "getTotalSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getTotalSpace", "()J"),
    );
    methods.insert(
        "getUsableSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getUsableSpace", "()J"),
    );
    methods.insert(
        "getUnallocatedSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getUnallocatedSpace", "()J"),
    );
    // Java 10+
    methods.insert(
        "getBlockSize:()J".to_string(),
        StubMethodDescriptor::instance_method("getBlockSize", "()J"),
    );
    methods.insert(
        "supportsFileAttributeView:(Ljava/lang/Class;)Z".to_string(),
        StubMethodDescriptor::instance_method("supportsFileAttributeView", "(Ljava/lang/Class;)Z"),
    );
    methods.insert(
        "supportsFileAttributeView:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("supportsFileAttributeView", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "getFileAttributeView:(Ljava/lang/Class;)Ljava/nio/file/attribute/FileAttributeView;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getFileAttributeView",
            "(Ljava/lang/Class;)Ljava/nio/file/attribute/FileAttributeView;",
        ),
    );
    methods.insert(
        "getAttribute:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAttribute",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "java/nio/file/FileStore".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ==================== java.nio.file Watch Service ====================

/// Create java.nio.file.WatchService interface stub
pub fn create_watch_service_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "poll:()Ljava/nio/file/WatchKey;".to_string(),
        StubMethodDescriptor::instance_method("poll", "()Ljava/nio/file/WatchKey;"),
    );
    methods.insert(
        "poll:(JLjava/util/concurrent/TimeUnit;)Ljava/nio/file/WatchKey;".to_string(),
        StubMethodDescriptor::instance_method(
            "poll",
            "(JLjava/util/concurrent/TimeUnit;)Ljava/nio/file/WatchKey;",
        ),
    );
    methods.insert(
        "take:()Ljava/nio/file/WatchKey;".to_string(),
        StubMethodDescriptor::instance_method("take", "()Ljava/nio/file/WatchKey;"),
    );

    StubClass {
        name: "java/nio/file/WatchService".to_string(),
        superclass: None,
        interfaces: vec!["java/io/Closeable".to_string()],
        methods,
    }
}

/// Create java.nio.file.WatchKey interface stub
pub fn create_watch_key_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "isValid:()Z".to_string(),
        StubMethodDescriptor::instance_method("isValid", "()Z"),
    );
    methods.insert(
        "pollEvents:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("pollEvents", "()Ljava/util/List;"),
    );
    methods.insert(
        "reset:()Z".to_string(),
        StubMethodDescriptor::instance_method("reset", "()Z"),
    );
    methods.insert(
        "cancel:()V".to_string(),
        StubMethodDescriptor::instance_method("cancel", "()V"),
    );
    methods.insert(
        "watchable:()Ljava/nio/file/Watchable;".to_string(),
        StubMethodDescriptor::instance_method("watchable", "()Ljava/nio/file/Watchable;"),
    );

    StubClass {
        name: "java/nio/file/WatchKey".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.WatchEvent interface stub
pub fn create_watch_event_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "kind:()Ljava/nio/file/WatchEvent$Kind;".to_string(),
        StubMethodDescriptor::instance_method("kind", "()Ljava/nio/file/WatchEvent$Kind;"),
    );
    methods.insert(
        "count:()I".to_string(),
        StubMethodDescriptor::instance_method("count", "()I"),
    );
    methods.insert(
        "context:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("context", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/nio/file/WatchEvent".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.WatchEvent$Kind interface stub
pub fn create_watch_event_kind_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "name:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("name", "()Ljava/lang/String;"),
    );
    methods.insert(
        "type:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("type", "()Ljava/lang/Class;"),
    );

    StubClass {
        name: "java/nio/file/WatchEvent$Kind".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.WatchEvent$Modifier interface stub
pub fn create_watch_event_modifier_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "name:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("name", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/nio/file/WatchEvent$Modifier".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.Watchable interface stub
pub fn create_watchable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "register:(Ljava/nio/file/WatchService;[Ljava/nio/file/WatchEvent$Kind;[Ljava/nio/file/WatchEvent$Modifier;)Ljava/nio/file/WatchKey;".to_string(),
        StubMethodDescriptor::instance_method(
            "register",
            "(Ljava/nio/file/WatchService;[Ljava/nio/file/WatchEvent$Kind;[Ljava/nio/file/WatchEvent$Modifier;)Ljava/nio/file/WatchKey;",
        ),
    );
    methods.insert(
        "register:(Ljava/nio/file/WatchService;[Ljava/nio/file/WatchEvent$Kind;)Ljava/nio/file/WatchKey;".to_string(),
        StubMethodDescriptor::instance_method(
            "register",
            "(Ljava/nio/file/WatchService;[Ljava/nio/file/WatchEvent$Kind;)Ljava/nio/file/WatchKey;",
        ),
    );

    StubClass {
        name: "java/nio/file/Watchable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.StandardWatchEventKinds utility class stub
pub fn create_standard_watch_event_kinds_class() -> StubClass {
    // This class only has static final fields, no methods
    // Fields are represented via static field access in the interpreter
    StubClass {
        name: "java/nio/file/StandardWatchEventKinds".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods: HashMap::new(),
    }
}

// ==================== java.nio.channels Supporting Interfaces ====================

/// Create java.nio.channels.Channel interface stub
pub fn create_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "isOpen:()Z".to_string(),
        StubMethodDescriptor::instance_method("isOpen", "()Z"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/nio/channels/Channel".to_string(),
        superclass: None,
        interfaces: vec!["java/io/Closeable".to_string()],
        methods,
    }
}

/// Create java.nio.channels.ReadableByteChannel interface stub
pub fn create_readable_byte_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "read:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("read", "(Ljava/nio/ByteBuffer;)I"),
    );

    StubClass {
        name: "java/nio/channels/ReadableByteChannel".to_string(),
        superclass: None,
        interfaces: vec!["java/nio/channels/Channel".to_string()],
        methods,
    }
}

/// Create java.nio.channels.WritableByteChannel interface stub
pub fn create_writable_byte_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "write:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/nio/ByteBuffer;)I"),
    );

    StubClass {
        name: "java/nio/channels/WritableByteChannel".to_string(),
        superclass: None,
        interfaces: vec!["java/nio/channels/Channel".to_string()],
        methods,
    }
}

/// Create java.nio.channels.ByteChannel interface stub
pub fn create_byte_channel_interface() -> StubClass {
    StubClass {
        name: "java/nio/channels/ByteChannel".to_string(),
        superclass: None,
        interfaces: vec![
            "java/nio/channels/ReadableByteChannel".to_string(),
            "java/nio/channels/WritableByteChannel".to_string(),
        ],
        methods: HashMap::new(),
    }
}

/// Create java.nio.channels.SeekableByteChannel interface stub
pub fn create_seekable_byte_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "read:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("read", "(Ljava/nio/ByteBuffer;)I"),
    );
    methods.insert(
        "write:(Ljava/nio/ByteBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/nio/ByteBuffer;)I"),
    );
    methods.insert(
        "position:()J".to_string(),
        StubMethodDescriptor::instance_method("position", "()J"),
    );
    methods.insert(
        "position:(J)Ljava/nio/channels/SeekableByteChannel;".to_string(),
        StubMethodDescriptor::instance_method(
            "position",
            "(J)Ljava/nio/channels/SeekableByteChannel;",
        ),
    );
    methods.insert(
        "size:()J".to_string(),
        StubMethodDescriptor::instance_method("size", "()J"),
    );
    methods.insert(
        "truncate:(J)Ljava/nio/channels/SeekableByteChannel;".to_string(),
        StubMethodDescriptor::instance_method(
            "truncate",
            "(J)Ljava/nio/channels/SeekableByteChannel;",
        ),
    );

    StubClass {
        name: "java/nio/channels/SeekableByteChannel".to_string(),
        superclass: None,
        interfaces: vec!["java/nio/channels/ByteChannel".to_string()],
        methods,
    }
}

/// Create java.nio.channels.AsynchronousChannel interface stub
pub fn create_asynchronous_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/nio/channels/AsynchronousChannel".to_string(),
        superclass: None,
        interfaces: vec!["java/nio/channels/Channel".to_string()],
        methods,
    }
}

/// Create java.nio.channels.AsynchronousByteChannel interface stub
pub fn create_asynchronous_byte_channel_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "read:(Ljava/nio/ByteBuffer;Ljava/lang/Object;Ljava/nio/channels/CompletionHandler;)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "read",
            "(Ljava/nio/ByteBuffer;Ljava/lang/Object;Ljava/nio/channels/CompletionHandler;)V",
        ),
    );
    methods.insert(
        "read:(Ljava/nio/ByteBuffer;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "read",
            "(Ljava/nio/ByteBuffer;)Ljava/util/concurrent/Future;",
        ),
    );
    methods.insert(
        "write:(Ljava/nio/ByteBuffer;Ljava/lang/Object;Ljava/nio/channels/CompletionHandler;)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "write",
            "(Ljava/nio/ByteBuffer;Ljava/lang/Object;Ljava/nio/channels/CompletionHandler;)V",
        ),
    );
    methods.insert(
        "write:(Ljava/nio/ByteBuffer;)Ljava/util/concurrent/Future;".to_string(),
        StubMethodDescriptor::instance_method(
            "write",
            "(Ljava/nio/ByteBuffer;)Ljava/util/concurrent/Future;",
        ),
    );

    StubClass {
        name: "java/nio/channels/AsynchronousByteChannel".to_string(),
        superclass: None,
        interfaces: vec!["java/nio/channels/AsynchronousChannel".to_string()],
        methods,
    }
}

/// Create java.nio.channels.CompletionHandler interface stub
pub fn create_completion_handler_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "completed:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "completed",
            "(Ljava/lang/Object;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "failed:(Ljava/lang/Throwable;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "failed",
            "(Ljava/lang/Throwable;Ljava/lang/Object;)V",
        ),
    );

    StubClass {
        name: "java/nio/channels/CompletionHandler".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== java.nio.file Supporting Interfaces ====================

/// Create java.nio.file.DirectoryStream interface stub
pub fn create_directory_stream_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );

    StubClass {
        name: "java/nio/file/DirectoryStream".to_string(),
        superclass: None,
        interfaces: vec![
            "java/io/Closeable".to_string(),
            "java/lang/Iterable".to_string(),
        ],
        methods,
    }
}

/// Create java.nio.file.DirectoryStream$Filter interface stub
pub fn create_directory_stream_filter_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;)Z"),
    );

    StubClass {
        name: "java/nio/file/DirectoryStream$Filter".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.PathMatcher interface stub
pub fn create_path_matcher_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "matches:(Ljava/nio/file/Path;)Z".to_string(),
        StubMethodDescriptor::instance_method("matches", "(Ljava/nio/file/Path;)Z"),
    );

    StubClass {
        name: "java/nio/file/PathMatcher".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.AccessMode enum stub
pub fn create_access_mode_enum() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "values:()[Ljava/nio/file/AccessMode;".to_string(),
        StubMethodDescriptor::static_method("values", "()[Ljava/nio/file/AccessMode;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/nio/file/AccessMode;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/String;)Ljava/nio/file/AccessMode;",
        ),
    );

    StubClass {
        name: "java/nio/file/AccessMode".to_string(),
        superclass: Some("java/lang/Enum".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.nio.file.LinkOption enum stub
pub fn create_link_option_enum() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "values:()[Ljava/nio/file/LinkOption;".to_string(),
        StubMethodDescriptor::static_method("values", "()[Ljava/nio/file/LinkOption;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/nio/file/LinkOption;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/String;)Ljava/nio/file/LinkOption;",
        ),
    );

    StubClass {
        name: "java/nio/file/LinkOption".to_string(),
        superclass: Some("java/lang/Enum".to_string()),
        interfaces: vec![
            "java/nio/file/OpenOption".to_string(),
            "java/nio/file/CopyOption".to_string(),
        ],
        methods,
    }
}

/// Create java.nio.file.CopyOption interface stub
pub fn create_copy_option_interface() -> StubClass {
    StubClass {
        name: "java/nio/file/CopyOption".to_string(),
        superclass: None,
        interfaces: vec![],
        methods: HashMap::new(),
    }
}

/// Create java.nio.file.OpenOption interface stub
pub fn create_open_option_interface() -> StubClass {
    StubClass {
        name: "java/nio/file/OpenOption".to_string(),
        superclass: None,
        interfaces: vec![],
        methods: HashMap::new(),
    }
}

// ==================== java.nio.channels.Channels Utility Class ====================

/// Create java.nio.channels.Channels utility class stub
pub fn create_channels_class() -> StubClass {
    let mut methods = HashMap::new();

    // InputStream/OutputStream from channels
    methods.insert(
        "newInputStream:(Ljava/nio/channels/ReadableByteChannel;)Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::static_method(
            "newInputStream",
            "(Ljava/nio/channels/ReadableByteChannel;)Ljava/io/InputStream;",
        ),
    );
    methods.insert(
        "newOutputStream:(Ljava/nio/channels/WritableByteChannel;)Ljava/io/OutputStream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newOutputStream",
            "(Ljava/nio/channels/WritableByteChannel;)Ljava/io/OutputStream;",
        ),
    );
    // Java 7+ asynchronous channel
    methods.insert(
        "newInputStream:(Ljava/nio/channels/AsynchronousByteChannel;)Ljava/io/InputStream;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newInputStream",
            "(Ljava/nio/channels/AsynchronousByteChannel;)Ljava/io/InputStream;",
        ),
    );

    // Channels from streams
    methods.insert(
        "newChannel:(Ljava/io/InputStream;)Ljava/nio/channels/ReadableByteChannel;".to_string(),
        StubMethodDescriptor::static_method(
            "newChannel",
            "(Ljava/io/InputStream;)Ljava/nio/channels/ReadableByteChannel;",
        ),
    );
    methods.insert(
        "newChannel:(Ljava/io/OutputStream;)Ljava/nio/channels/WritableByteChannel;".to_string(),
        StubMethodDescriptor::static_method(
            "newChannel",
            "(Ljava/io/OutputStream;)Ljava/nio/channels/WritableByteChannel;",
        ),
    );

    // Reader from channel
    methods.insert(
        "newReader:(Ljava/nio/channels/ReadableByteChannel;Ljava/nio/charset/CharsetDecoder;I)Ljava/io/Reader;".to_string(),
        StubMethodDescriptor::static_method(
            "newReader",
            "(Ljava/nio/channels/ReadableByteChannel;Ljava/nio/charset/CharsetDecoder;I)Ljava/io/Reader;",
        ),
    );
    methods.insert(
        "newReader:(Ljava/nio/channels/ReadableByteChannel;Ljava/lang/String;)Ljava/io/Reader;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newReader",
            "(Ljava/nio/channels/ReadableByteChannel;Ljava/lang/String;)Ljava/io/Reader;",
        ),
    );
    // Java 10+
    methods.insert(
        "newReader:(Ljava/nio/channels/ReadableByteChannel;Ljava/nio/charset/Charset;)Ljava/io/Reader;".to_string(),
        StubMethodDescriptor::static_method(
            "newReader",
            "(Ljava/nio/channels/ReadableByteChannel;Ljava/nio/charset/Charset;)Ljava/io/Reader;",
        ),
    );

    // Writer from channel
    methods.insert(
        "newWriter:(Ljava/nio/channels/WritableByteChannel;Ljava/nio/charset/CharsetEncoder;I)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::static_method(
            "newWriter",
            "(Ljava/nio/channels/WritableByteChannel;Ljava/nio/charset/CharsetEncoder;I)Ljava/io/Writer;",
        ),
    );
    methods.insert(
        "newWriter:(Ljava/nio/channels/WritableByteChannel;Ljava/lang/String;)Ljava/io/Writer;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "newWriter",
            "(Ljava/nio/channels/WritableByteChannel;Ljava/lang/String;)Ljava/io/Writer;",
        ),
    );
    // Java 10+
    methods.insert(
        "newWriter:(Ljava/nio/channels/WritableByteChannel;Ljava/nio/charset/Charset;)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::static_method(
            "newWriter",
            "(Ljava/nio/channels/WritableByteChannel;Ljava/nio/charset/Charset;)Ljava/io/Writer;",
        ),
    );

    StubClass {
        name: "java/nio/channels/Channels".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Register all java.nio function classes
pub fn register_java_nio_classes(classes: &mut HashMap<String, StubClass>) {
    // java.nio.file package
    classes.insert("java/nio/file/Path".to_string(), create_path_interface());
    classes.insert("java/nio/file/Paths".to_string(), create_paths_class());
    classes.insert("java/nio/file/Files".to_string(), create_files_class());
    classes.insert(
        "java/nio/file/StandardOpenOption".to_string(),
        create_standard_open_option_enum(),
    );

    // java.nio buffer classes
    classes.insert(
        "java/nio/ByteBuffer".to_string(),
        create_byte_buffer_class(),
    );
    classes.insert(
        "java/nio/CharBuffer".to_string(),
        create_char_buffer_class(),
    );
    classes.insert("java/nio/ByteOrder".to_string(), create_byte_order_class());

    // java.nio.channels package
    classes.insert(
        "java/nio/channels/FileChannel".to_string(),
        create_file_channel_class(),
    );

    // FileSystem classes
    classes.insert(
        "java/nio/file/FileSystem".to_string(),
        create_file_system_class(),
    );
    classes.insert(
        "java/nio/file/FileSystems".to_string(),
        create_file_systems_class(),
    );
    classes.insert(
        "java/nio/file/spi/FileSystemProvider".to_string(),
        create_file_system_provider_class(),
    );
    classes.insert(
        "java/nio/file/FileStore".to_string(),
        create_file_store_class(),
    );

    // Watch service interfaces
    classes.insert(
        "java/nio/file/WatchService".to_string(),
        create_watch_service_interface(),
    );
    classes.insert(
        "java/nio/file/WatchKey".to_string(),
        create_watch_key_interface(),
    );
    classes.insert(
        "java/nio/file/WatchEvent".to_string(),
        create_watch_event_interface(),
    );
    classes.insert(
        "java/nio/file/WatchEvent$Kind".to_string(),
        create_watch_event_kind_interface(),
    );
    classes.insert(
        "java/nio/file/WatchEvent$Modifier".to_string(),
        create_watch_event_modifier_interface(),
    );
    classes.insert(
        "java/nio/file/Watchable".to_string(),
        create_watchable_interface(),
    );
    classes.insert(
        "java/nio/file/StandardWatchEventKinds".to_string(),
        create_standard_watch_event_kinds_class(),
    );

    // Channel interfaces
    classes.insert(
        "java/nio/channels/Channel".to_string(),
        create_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/ReadableByteChannel".to_string(),
        create_readable_byte_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/WritableByteChannel".to_string(),
        create_writable_byte_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/ByteChannel".to_string(),
        create_byte_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/SeekableByteChannel".to_string(),
        create_seekable_byte_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/AsynchronousChannel".to_string(),
        create_asynchronous_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/AsynchronousByteChannel".to_string(),
        create_asynchronous_byte_channel_interface(),
    );
    classes.insert(
        "java/nio/channels/CompletionHandler".to_string(),
        create_completion_handler_interface(),
    );

    // Directory stream and filter
    classes.insert(
        "java/nio/file/DirectoryStream".to_string(),
        create_directory_stream_interface(),
    );
    classes.insert(
        "java/nio/file/DirectoryStream$Filter".to_string(),
        create_directory_stream_filter_interface(),
    );

    // PathMatcher
    classes.insert(
        "java/nio/file/PathMatcher".to_string(),
        create_path_matcher_interface(),
    );

    // Enums and options
    classes.insert(
        "java/nio/file/AccessMode".to_string(),
        create_access_mode_enum(),
    );
    classes.insert(
        "java/nio/file/LinkOption".to_string(),
        create_link_option_enum(),
    );
    classes.insert(
        "java/nio/file/CopyOption".to_string(),
        create_copy_option_interface(),
    );
    classes.insert(
        "java/nio/file/OpenOption".to_string(),
        create_open_option_interface(),
    );

    // Channels utility class
    classes.insert(
        "java/nio/channels/Channels".to_string(),
        create_channels_class(),
    );
}
