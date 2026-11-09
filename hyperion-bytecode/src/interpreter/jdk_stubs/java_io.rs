// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.io JDK stub classes
//!
//! Comprehensive I/O class stubs: File, streams, readers, writers, etc.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

// ============================================================================
// Interfaces
// ============================================================================

/// Create java.io.Closeable interface stub
pub fn create_closeable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/Closeable".to_string(),
        superclass: None,
        interfaces: vec!["java/lang/AutoCloseable".to_string()],
        methods,
    }
}

/// Create java.io.Flushable interface stub
pub fn create_flushable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );

    StubClass {
        name: "java/io/Flushable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.Serializable interface stub
pub fn create_serializable_interface() -> StubClass {
    // Marker interface - no methods
    StubClass {
        name: "java/io/Serializable".to_string(),
        superclass: None,
        interfaces: vec![],
        methods: HashMap::new(),
    }
}

/// Create java.io.FileFilter interface stub
pub fn create_file_filter_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/io/File;)Z".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/io/File;)Z"),
    );

    StubClass {
        name: "java/io/FileFilter".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FilenameFilter interface stub
pub fn create_filename_filter_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/io/File;Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/io/File;Ljava/lang/String;)Z"),
    );

    StubClass {
        name: "java/io/FilenameFilter".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ============================================================================
// FileDescriptor
// ============================================================================

/// Create java.io.FileDescriptor class stub
pub fn create_file_descriptor_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Instance methods
    methods.insert(
        "valid:()Z".to_string(),
        StubMethodDescriptor::instance_method("valid", "()Z"),
    );
    methods.insert(
        "sync:()V".to_string(),
        StubMethodDescriptor::instance_method("sync", "()V"),
    );

    StubClass {
        name: "java/io/FileDescriptor".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ============================================================================
// File
// ============================================================================

/// Create java.io.File stub class
pub fn create_file_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/net/URI;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/net/URI;)V"),
    );

    // Path info methods
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getParent:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getParent", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getParentFile:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("getParentFile", "()Ljava/io/File;"),
    );
    methods.insert(
        "getPath:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getPath", "()Ljava/lang/String;"),
    );
    methods.insert(
        "isAbsolute:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAbsolute", "()Z"),
    );
    methods.insert(
        "getAbsolutePath:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getAbsolutePath", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getAbsoluteFile:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("getAbsoluteFile", "()Ljava/io/File;"),
    );
    methods.insert(
        "getCanonicalPath:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getCanonicalPath", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getCanonicalFile:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("getCanonicalFile", "()Ljava/io/File;"),
    );
    methods.insert(
        "toURI:()Ljava/net/URI;".to_string(),
        StubMethodDescriptor::instance_method("toURI", "()Ljava/net/URI;"),
    );
    methods.insert(
        "toPath:()Ljava/nio/file/Path;".to_string(),
        StubMethodDescriptor::instance_method("toPath", "()Ljava/nio/file/Path;"),
    );

    // Existence checks
    methods.insert(
        "exists:()Z".to_string(),
        StubMethodDescriptor::instance_method("exists", "()Z"),
    );
    methods.insert(
        "isDirectory:()Z".to_string(),
        StubMethodDescriptor::instance_method("isDirectory", "()Z"),
    );
    methods.insert(
        "isFile:()Z".to_string(),
        StubMethodDescriptor::instance_method("isFile", "()Z"),
    );
    methods.insert(
        "isHidden:()Z".to_string(),
        StubMethodDescriptor::instance_method("isHidden", "()Z"),
    );

    // Permissions
    methods.insert(
        "canRead:()Z".to_string(),
        StubMethodDescriptor::instance_method("canRead", "()Z"),
    );
    methods.insert(
        "canWrite:()Z".to_string(),
        StubMethodDescriptor::instance_method("canWrite", "()Z"),
    );
    methods.insert(
        "canExecute:()Z".to_string(),
        StubMethodDescriptor::instance_method("canExecute", "()Z"),
    );
    methods.insert(
        "setReadable:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("setReadable", "(Z)Z"),
    );
    methods.insert(
        "setReadable:(ZZ)Z".to_string(),
        StubMethodDescriptor::instance_method("setReadable", "(ZZ)Z"),
    );
    methods.insert(
        "setWritable:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("setWritable", "(Z)Z"),
    );
    methods.insert(
        "setWritable:(ZZ)Z".to_string(),
        StubMethodDescriptor::instance_method("setWritable", "(ZZ)Z"),
    );
    methods.insert(
        "setExecutable:(Z)Z".to_string(),
        StubMethodDescriptor::instance_method("setExecutable", "(Z)Z"),
    );
    methods.insert(
        "setExecutable:(ZZ)Z".to_string(),
        StubMethodDescriptor::instance_method("setExecutable", "(ZZ)Z"),
    );
    methods.insert(
        "setReadOnly:()Z".to_string(),
        StubMethodDescriptor::instance_method("setReadOnly", "()Z"),
    );

    // File info
    methods.insert(
        "length:()J".to_string(),
        StubMethodDescriptor::instance_method("length", "()J"),
    );
    methods.insert(
        "lastModified:()J".to_string(),
        StubMethodDescriptor::instance_method("lastModified", "()J"),
    );
    methods.insert(
        "setLastModified:(J)Z".to_string(),
        StubMethodDescriptor::instance_method("setLastModified", "(J)Z"),
    );

    // Directory operations
    methods.insert(
        "list:()[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("list", "()[Ljava/lang/String;"),
    );
    methods.insert(
        "list:(Ljava/io/FilenameFilter;)[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "list",
            "(Ljava/io/FilenameFilter;)[Ljava/lang/String;",
        ),
    );
    methods.insert(
        "listFiles:()[Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("listFiles", "()[Ljava/io/File;"),
    );
    methods.insert(
        "listFiles:(Ljava/io/FileFilter;)[Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("listFiles", "(Ljava/io/FileFilter;)[Ljava/io/File;"),
    );
    methods.insert(
        "listFiles:(Ljava/io/FilenameFilter;)[Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method(
            "listFiles",
            "(Ljava/io/FilenameFilter;)[Ljava/io/File;",
        ),
    );
    methods.insert(
        "mkdir:()Z".to_string(),
        StubMethodDescriptor::instance_method("mkdir", "()Z"),
    );
    methods.insert(
        "mkdirs:()Z".to_string(),
        StubMethodDescriptor::instance_method("mkdirs", "()Z"),
    );

    // File operations
    methods.insert(
        "createNewFile:()Z".to_string(),
        StubMethodDescriptor::instance_method("createNewFile", "()Z"),
    );
    methods.insert(
        "delete:()Z".to_string(),
        StubMethodDescriptor::instance_method("delete", "()Z"),
    );
    methods.insert(
        "deleteOnExit:()V".to_string(),
        StubMethodDescriptor::instance_method("deleteOnExit", "()V"),
    );
    methods.insert(
        "renameTo:(Ljava/io/File;)Z".to_string(),
        StubMethodDescriptor::instance_method("renameTo", "(Ljava/io/File;)Z"),
    );

    // Static methods
    methods.insert(
        "createTempFile:(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;".to_string(),
        StubMethodDescriptor::static_method(
            "createTempFile",
            "(Ljava/lang/String;Ljava/lang/String;)Ljava/io/File;",
        ),
    );
    methods.insert(
        "createTempFile:(Ljava/lang/String;Ljava/lang/String;Ljava/io/File;)Ljava/io/File;"
            .to_string(),
        StubMethodDescriptor::static_method(
            "createTempFile",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/io/File;)Ljava/io/File;",
        ),
    );
    methods.insert(
        "listRoots:()[Ljava/io/File;".to_string(),
        StubMethodDescriptor::static_method("listRoots", "()[Ljava/io/File;"),
    );

    // Comparable/Object methods
    methods.insert(
        "compareTo:(Ljava/io/File;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/io/File;)I"),
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

    // Space methods
    methods.insert(
        "getTotalSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getTotalSpace", "()J"),
    );
    methods.insert(
        "getFreeSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getFreeSpace", "()J"),
    );
    methods.insert(
        "getUsableSpace:()J".to_string(),
        StubMethodDescriptor::instance_method("getUsableSpace", "()J"),
    );

    StubClass {
        name: "java/io/File".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Serializable".to_string(),
            "java/lang/Comparable".to_string(),
        ],
        methods,
    }
}

// ============================================================================
// InputStream hierarchy
// ============================================================================

/// Create java.io.InputStream stub class (abstract)
pub fn create_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Abstract method
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );

    // Default implementations
    methods.insert(
        "read:([B)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([B)I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );

    // Java 9+ methods
    methods.insert(
        "readAllBytes:()[B".to_string(),
        StubMethodDescriptor::instance_method("readAllBytes", "()[B"),
    );
    // Java 11+ methods
    methods.insert(
        "readNBytes:(I)[B".to_string(),
        StubMethodDescriptor::instance_method("readNBytes", "(I)[B"),
    );
    methods.insert(
        "readNBytes:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("readNBytes", "([BII)I"),
    );

    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    // Java 12+ method
    methods.insert(
        "skipNBytes:(J)V".to_string(),
        StubMethodDescriptor::instance_method("skipNBytes", "(J)V"),
    );

    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );

    // Java 9+ method
    methods.insert(
        "transferTo:(Ljava/io/OutputStream;)J".to_string(),
        StubMethodDescriptor::instance_method("transferTo", "(Ljava/io/OutputStream;)J"),
    );

    // Java 11+ static method
    methods.insert(
        "nullInputStream:()Ljava/io/InputStream;".to_string(),
        StubMethodDescriptor::static_method("nullInputStream", "()Ljava/io/InputStream;"),
    );

    StubClass {
        name: "java/io/InputStream".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Closeable".to_string()],
        methods,
    }
}

/// Create java.io.FilterInputStream stub class
pub fn create_filter_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );

    // Overridden methods
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([B)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([B)I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );

    StubClass {
        name: "java/io/FilterInputStream".to_string(),
        superclass: Some("java/io/InputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FileInputStream stub class
pub fn create_file_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/FileDescriptor;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/FileDescriptor;)V"),
    );

    // Overridden methods
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([B)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([B)I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // FileInputStream-specific methods
    methods.insert(
        "getFD:()Ljava/io/FileDescriptor;".to_string(),
        StubMethodDescriptor::instance_method("getFD", "()Ljava/io/FileDescriptor;"),
    );
    methods.insert(
        "getChannel:()Ljava/nio/channels/FileChannel;".to_string(),
        StubMethodDescriptor::instance_method("getChannel", "()Ljava/nio/channels/FileChannel;"),
    );

    StubClass {
        name: "java/io/FileInputStream".to_string(),
        superclass: Some("java/io/InputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.BufferedInputStream stub class
pub fn create_buffered_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/InputStream;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;I)V"),
    );

    // Overridden methods
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/BufferedInputStream".to_string(),
        superclass: Some("java/io/FilterInputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.DataInputStream stub class
pub fn create_data_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );

    // DataInput methods
    methods.insert(
        "read:([B)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([B)I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "readFully:([B)V".to_string(),
        StubMethodDescriptor::instance_method("readFully", "([B)V"),
    );
    methods.insert(
        "readFully:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("readFully", "([BII)V"),
    );
    methods.insert(
        "skipBytes:(I)I".to_string(),
        StubMethodDescriptor::instance_method("skipBytes", "(I)I"),
    );
    methods.insert(
        "readBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("readBoolean", "()Z"),
    );
    methods.insert(
        "readByte:()B".to_string(),
        StubMethodDescriptor::instance_method("readByte", "()B"),
    );
    methods.insert(
        "readUnsignedByte:()I".to_string(),
        StubMethodDescriptor::instance_method("readUnsignedByte", "()I"),
    );
    methods.insert(
        "readShort:()S".to_string(),
        StubMethodDescriptor::instance_method("readShort", "()S"),
    );
    methods.insert(
        "readUnsignedShort:()I".to_string(),
        StubMethodDescriptor::instance_method("readUnsignedShort", "()I"),
    );
    methods.insert(
        "readChar:()C".to_string(),
        StubMethodDescriptor::instance_method("readChar", "()C"),
    );
    methods.insert(
        "readInt:()I".to_string(),
        StubMethodDescriptor::instance_method("readInt", "()I"),
    );
    methods.insert(
        "readLong:()J".to_string(),
        StubMethodDescriptor::instance_method("readLong", "()J"),
    );
    methods.insert(
        "readFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("readFloat", "()F"),
    );
    methods.insert(
        "readDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("readDouble", "()D"),
    );
    methods.insert(
        "readLine:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readLine", "()Ljava/lang/String;"),
    );
    methods.insert(
        "readUTF:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readUTF", "()Ljava/lang/String;"),
    );

    // Static method
    methods.insert(
        "readUTF:(Ljava/io/DataInput;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::static_method("readUTF", "(Ljava/io/DataInput;)Ljava/lang/String;"),
    );

    StubClass {
        name: "java/io/DataInputStream".to_string(),
        superclass: Some("java/io/FilterInputStream".to_string()),
        interfaces: vec!["java/io/DataInput".to_string()],
        methods,
    }
}

/// Create java.io.ByteArrayInputStream stub class
pub fn create_byte_array_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:([B)V".to_string(),
        StubMethodDescriptor::constructor("([B)V"),
    );
    methods.insert(
        "<init>:([BII)V".to_string(),
        StubMethodDescriptor::constructor("([BII)V"),
    );

    // Overridden methods
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "readAllBytes:()[B".to_string(),
        StubMethodDescriptor::instance_method("readAllBytes", "()[B"),
    );
    methods.insert(
        "readNBytes:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("readNBytes", "([BII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/ByteArrayInputStream".to_string(),
        superclass: Some("java/io/InputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.ObjectInputStream stub class
pub fn create_object_input_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );

    // Core methods
    methods.insert(
        "readObject:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("readObject", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "readUnshared:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("readUnshared", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "defaultReadObject:()V".to_string(),
        StubMethodDescriptor::instance_method("defaultReadObject", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // DataInput methods
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "readBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("readBoolean", "()Z"),
    );
    methods.insert(
        "readByte:()B".to_string(),
        StubMethodDescriptor::instance_method("readByte", "()B"),
    );
    methods.insert(
        "readChar:()C".to_string(),
        StubMethodDescriptor::instance_method("readChar", "()C"),
    );
    methods.insert(
        "readDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("readDouble", "()D"),
    );
    methods.insert(
        "readFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("readFloat", "()F"),
    );
    methods.insert(
        "readInt:()I".to_string(),
        StubMethodDescriptor::instance_method("readInt", "()I"),
    );
    methods.insert(
        "readLong:()J".to_string(),
        StubMethodDescriptor::instance_method("readLong", "()J"),
    );
    methods.insert(
        "readShort:()S".to_string(),
        StubMethodDescriptor::instance_method("readShort", "()S"),
    );
    methods.insert(
        "readUTF:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readUTF", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/io/ObjectInputStream".to_string(),
        superclass: Some("java/io/InputStream".to_string()),
        interfaces: vec!["java/io/ObjectInput".to_string()],
        methods,
    }
}

// ============================================================================
// OutputStream hierarchy
// ============================================================================

/// Create java.io.OutputStream stub class (abstract)
pub fn create_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // Abstract method
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );

    // Default implementations
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // Java 11+ static method
    methods.insert(
        "nullOutputStream:()Ljava/io/OutputStream;".to_string(),
        StubMethodDescriptor::static_method("nullOutputStream", "()Ljava/io/OutputStream;"),
    );

    StubClass {
        name: "java/io/OutputStream".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/io/Closeable".to_string(),
            "java/io/Flushable".to_string(),
        ],
        methods,
    }
}

/// Create java.io.FilterOutputStream stub class
pub fn create_filter_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );

    // Overridden methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/FilterOutputStream".to_string(),
        superclass: Some("java/io/OutputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FileOutputStream stub class
pub fn create_file_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/FileDescriptor;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/FileDescriptor;)V"),
    );

    // Overridden methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // FileOutputStream-specific methods
    methods.insert(
        "getFD:()Ljava/io/FileDescriptor;".to_string(),
        StubMethodDescriptor::instance_method("getFD", "()Ljava/io/FileDescriptor;"),
    );
    methods.insert(
        "getChannel:()Ljava/nio/channels/FileChannel;".to_string(),
        StubMethodDescriptor::instance_method("getChannel", "()Ljava/nio/channels/FileChannel;"),
    );

    StubClass {
        name: "java/io/FileOutputStream".to_string(),
        superclass: Some("java/io/OutputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.BufferedOutputStream stub class
pub fn create_buffered_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;I)V"),
    );

    // Overridden methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );

    StubClass {
        name: "java/io/BufferedOutputStream".to_string(),
        superclass: Some("java/io/FilterOutputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.DataOutputStream stub class
pub fn create_data_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );

    // DataOutput methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "writeBoolean:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("writeBoolean", "(Z)V"),
    );
    methods.insert(
        "writeByte:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeByte", "(I)V"),
    );
    methods.insert(
        "writeShort:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeShort", "(I)V"),
    );
    methods.insert(
        "writeChar:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeChar", "(I)V"),
    );
    methods.insert(
        "writeInt:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeInt", "(I)V"),
    );
    methods.insert(
        "writeLong:(J)V".to_string(),
        StubMethodDescriptor::instance_method("writeLong", "(J)V"),
    );
    methods.insert(
        "writeFloat:(F)V".to_string(),
        StubMethodDescriptor::instance_method("writeFloat", "(F)V"),
    );
    methods.insert(
        "writeDouble:(D)V".to_string(),
        StubMethodDescriptor::instance_method("writeDouble", "(D)V"),
    );
    methods.insert(
        "writeBytes:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeBytes", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "writeChars:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeChars", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "writeUTF:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeUTF", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );

    StubClass {
        name: "java/io/DataOutputStream".to_string(),
        superclass: Some("java/io/FilterOutputStream".to_string()),
        interfaces: vec!["java/io/DataOutput".to_string()],
        methods,
    }
}

/// Create java.io.ByteArrayOutputStream stub class
pub fn create_byte_array_output_stream_class() -> StubClass {
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

    // Overridden methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "writeBytes:([B)V".to_string(),
        StubMethodDescriptor::instance_method("writeBytes", "([B)V"),
    );
    methods.insert(
        "writeTo:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::instance_method("writeTo", "(Ljava/io/OutputStream;)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "toByteArray:()[B".to_string(),
        StubMethodDescriptor::instance_method("toByteArray", "()[B"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toString:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/ByteArrayOutputStream".to_string(),
        superclass: Some("java/io/OutputStream".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.ObjectOutputStream stub class
pub fn create_object_output_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );

    // Core methods
    methods.insert(
        "writeObject:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("writeObject", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "writeUnshared:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("writeUnshared", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "defaultWriteObject:()V".to_string(),
        StubMethodDescriptor::instance_method("defaultWriteObject", "()V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );

    // DataOutput methods
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "writeBoolean:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("writeBoolean", "(Z)V"),
    );
    methods.insert(
        "writeByte:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeByte", "(I)V"),
    );
    methods.insert(
        "writeChar:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeChar", "(I)V"),
    );
    methods.insert(
        "writeDouble:(D)V".to_string(),
        StubMethodDescriptor::instance_method("writeDouble", "(D)V"),
    );
    methods.insert(
        "writeFloat:(F)V".to_string(),
        StubMethodDescriptor::instance_method("writeFloat", "(F)V"),
    );
    methods.insert(
        "writeInt:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeInt", "(I)V"),
    );
    methods.insert(
        "writeLong:(J)V".to_string(),
        StubMethodDescriptor::instance_method("writeLong", "(J)V"),
    );
    methods.insert(
        "writeShort:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeShort", "(I)V"),
    );
    methods.insert(
        "writeUTF:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeUTF", "(Ljava/lang/String;)V"),
    );

    StubClass {
        name: "java/io/ObjectOutputStream".to_string(),
        superclass: Some("java/io/OutputStream".to_string()),
        interfaces: vec!["java/io/ObjectOutput".to_string()],
        methods,
    }
}

// ============================================================================
// PrintStream
// ============================================================================

/// Create java.io.PrintStream stub class
pub fn create_print_stream_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;ZLjava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;ZLjava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/lang/String;)V"),
    );

    // OutputStream overrides
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "checkError:()Z".to_string(),
        StubMethodDescriptor::instance_method("checkError", "()Z"),
    );
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );

    // print methods
    methods.insert(
        "print:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Z)V"),
    );
    methods.insert(
        "print:(C)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(C)V"),
    );
    methods.insert(
        "print:(I)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(I)V"),
    );
    methods.insert(
        "print:(J)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(J)V"),
    );
    methods.insert(
        "print:(F)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(F)V"),
    );
    methods.insert(
        "print:(D)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(D)V"),
    );
    methods.insert(
        "print:([C)V".to_string(),
        StubMethodDescriptor::instance_method("print", "([C)V"),
    );
    methods.insert(
        "print:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "print:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Ljava/lang/Object;)V"),
    );

    // println methods
    methods.insert(
        "println:()V".to_string(),
        StubMethodDescriptor::instance_method("println", "()V"),
    );
    methods.insert(
        "println:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Z)V"),
    );
    methods.insert(
        "println:(C)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(C)V"),
    );
    methods.insert(
        "println:(I)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(I)V"),
    );
    methods.insert(
        "println:(J)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(J)V"),
    );
    methods.insert(
        "println:(F)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(F)V"),
    );
    methods.insert(
        "println:(D)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(D)V"),
    );
    methods.insert(
        "println:([C)V".to_string(),
        StubMethodDescriptor::instance_method("println", "([C)V"),
    );
    methods.insert(
        "println:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "println:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Ljava/lang/Object;)V"),
    );

    // printf/format methods
    methods.insert(
        "printf:(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "printf",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
        ),
    );
    methods.insert(
        "printf:(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "printf",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
        ),
    );
    methods.insert(
        "format:(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
        ),
    );
    methods.insert(
        "format:(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "format",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
        ),
    );

    // Appendable methods
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/io/PrintStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/io/PrintStream;",
        ),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;II)Ljava/io/PrintStream;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/PrintStream;",
        ),
    );
    methods.insert(
        "append:(C)Ljava/io/PrintStream;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/io/PrintStream;"),
    );

    StubClass {
        name: "java/io/PrintStream".to_string(),
        superclass: Some("java/io/FilterOutputStream".to_string()),
        interfaces: vec![
            "java/lang/Appendable".to_string(),
            "java/io/Closeable".to_string(),
        ],
        methods,
    }
}

// ============================================================================
// Reader hierarchy
// ============================================================================

/// Create java.io.Reader stub class (abstract)
pub fn create_reader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;)V"),
    );

    // Abstract methods
    methods.insert(
        "read:([CII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([CII)I"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // Default implementations
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([C)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([C)I"),
    );
    methods.insert(
        "read:(Ljava/nio/CharBuffer;)I".to_string(),
        StubMethodDescriptor::instance_method("read", "(Ljava/nio/CharBuffer;)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "ready:()Z".to_string(),
        StubMethodDescriptor::instance_method("ready", "()Z"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );

    // Java 10+ method
    methods.insert(
        "transferTo:(Ljava/io/Writer;)J".to_string(),
        StubMethodDescriptor::instance_method("transferTo", "(Ljava/io/Writer;)J"),
    );

    // Java 11+ static method
    methods.insert(
        "nullReader:()Ljava/io/Reader;".to_string(),
        StubMethodDescriptor::static_method("nullReader", "()Ljava/io/Reader;"),
    );

    StubClass {
        name: "java/io/Reader".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/lang/Readable".to_string(),
            "java/io/Closeable".to_string(),
        ],
        methods,
    }
}

/// Create java.io.InputStreamReader stub class
pub fn create_input_stream_reader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/InputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/InputStream;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/InputStream;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/InputStream;Ljava/nio/charset/Charset;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/InputStream;Ljava/nio/charset/CharsetDecoder;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/io/InputStream;Ljava/nio/charset/CharsetDecoder;)V",
        ),
    );

    // Reader overrides
    methods.insert(
        "getEncoding:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getEncoding", "()Ljava/lang/String;"),
    );
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([CII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([CII)I"),
    );
    methods.insert(
        "ready:()Z".to_string(),
        StubMethodDescriptor::instance_method("ready", "()Z"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/InputStreamReader".to_string(),
        superclass: Some("java/io/Reader".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FileReader stub class
pub fn create_file_reader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/FileDescriptor;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/FileDescriptor;)V"),
    );
    // Java 11+ constructors
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/nio/charset/Charset;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/nio/charset/Charset;)V"),
    );

    StubClass {
        name: "java/io/FileReader".to_string(),
        superclass: Some("java/io/InputStreamReader".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.BufferedReader stub class
pub fn create_buffered_reader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/Reader;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Reader;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/Reader;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Reader;I)V"),
    );

    // Reader overrides
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([CII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([CII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "ready:()Z".to_string(),
        StubMethodDescriptor::instance_method("ready", "()Z"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // BufferedReader-specific methods
    methods.insert(
        "readLine:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readLine", "()Ljava/lang/String;"),
    );

    // Java 8+ method
    methods.insert(
        "lines:()Ljava/util/stream/Stream;".to_string(),
        StubMethodDescriptor::instance_method("lines", "()Ljava/util/stream/Stream;"),
    );

    StubClass {
        name: "java/io/BufferedReader".to_string(),
        superclass: Some("java/io/Reader".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.StringReader stub class
pub fn create_string_reader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Reader overrides
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([CII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([CII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "ready:()Z".to_string(),
        StubMethodDescriptor::instance_method("ready", "()Z"),
    );
    methods.insert(
        "markSupported:()Z".to_string(),
        StubMethodDescriptor::instance_method("markSupported", "()Z"),
    );
    methods.insert(
        "mark:(I)V".to_string(),
        StubMethodDescriptor::instance_method("mark", "(I)V"),
    );
    methods.insert(
        "reset:()V".to_string(),
        StubMethodDescriptor::instance_method("reset", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/StringReader".to_string(),
        superclass: Some("java/io/Reader".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ============================================================================
// Writer hierarchy
// ============================================================================

/// Create java.io.Writer stub class (abstract)
pub fn create_writer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;)V"),
    );

    // Abstract methods
    methods.insert(
        "write:([CII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([CII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // Default implementations
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([C)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([C)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;II)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;II)V"),
    );

    // Appendable methods
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/io/Writer;",
        ),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;II)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/Writer;",
        ),
    );
    methods.insert(
        "append:(C)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/io/Writer;"),
    );

    // Java 11+ static method
    methods.insert(
        "nullWriter:()Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::static_method("nullWriter", "()Ljava/io/Writer;"),
    );

    StubClass {
        name: "java/io/Writer".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![
            "java/lang/Appendable".to_string(),
            "java/io/Closeable".to_string(),
            "java/io/Flushable".to_string(),
        ],
        methods,
    }
}

/// Create java.io.OutputStreamWriter stub class
pub fn create_output_stream_writer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;Ljava/nio/charset/Charset;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;Ljava/nio/charset/CharsetEncoder;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/io/OutputStream;Ljava/nio/charset/CharsetEncoder;)V",
        ),
    );

    // Writer overrides
    methods.insert(
        "getEncoding:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getEncoding", "()Ljava/lang/String;"),
    );
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([CII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([CII)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;II)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;II)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/OutputStreamWriter".to_string(),
        superclass: Some("java/io/Writer".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FileWriter stub class
pub fn create_file_writer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/FileDescriptor;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/FileDescriptor;)V"),
    );
    // Java 11+ constructors
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/nio/charset/Charset;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/nio/charset/Charset;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/nio/charset/Charset;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/nio/charset/Charset;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/nio/charset/Charset;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/nio/charset/Charset;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/nio/charset/Charset;Z)V"),
    );

    StubClass {
        name: "java/io/FileWriter".to_string(),
        superclass: Some("java/io/OutputStreamWriter".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.BufferedWriter stub class
pub fn create_buffered_writer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/Writer;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Writer;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/Writer;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Writer;I)V"),
    );

    // Writer overrides
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([CII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([CII)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;II)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;II)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // BufferedWriter-specific method
    methods.insert(
        "newLine:()V".to_string(),
        StubMethodDescriptor::instance_method("newLine", "()V"),
    );

    StubClass {
        name: "java/io/BufferedWriter".to_string(),
        superclass: Some("java/io/Writer".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.StringWriter stub class
pub fn create_string_writer_class() -> StubClass {
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

    // Writer overrides
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([CII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([CII)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;II)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;II)V"),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/io/StringWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/io/StringWriter;",
        ),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;II)Ljava/io/StringWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/StringWriter;",
        ),
    );
    methods.insert(
        "append:(C)Ljava/io/StringWriter;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/io/StringWriter;"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    // StringWriter-specific methods
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getBuffer:()Ljava/lang/StringBuffer;".to_string(),
        StubMethodDescriptor::instance_method("getBuffer", "()Ljava/lang/StringBuffer;"),
    );

    StubClass {
        name: "java/io/StringWriter".to_string(),
        superclass: Some("java/io/Writer".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.PrintWriter stub class
pub fn create_print_writer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/io/Writer;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Writer;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/Writer;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/Writer;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/OutputStream;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/OutputStream;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/lang/String;)V"),
    );

    // Writer overrides
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );
    methods.insert(
        "checkError:()Z".to_string(),
        StubMethodDescriptor::instance_method("checkError", "()Z"),
    );
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([CII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([CII)V"),
    );
    methods.insert(
        "write:([C)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([C)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;II)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;II)V"),
    );
    methods.insert(
        "write:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(Ljava/lang/String;)V"),
    );

    // print methods
    methods.insert(
        "print:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Z)V"),
    );
    methods.insert(
        "print:(C)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(C)V"),
    );
    methods.insert(
        "print:(I)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(I)V"),
    );
    methods.insert(
        "print:(J)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(J)V"),
    );
    methods.insert(
        "print:(F)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(F)V"),
    );
    methods.insert(
        "print:(D)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(D)V"),
    );
    methods.insert(
        "print:([C)V".to_string(),
        StubMethodDescriptor::instance_method("print", "([C)V"),
    );
    methods.insert(
        "print:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "print:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Ljava/lang/Object;)V"),
    );

    // println methods
    methods.insert(
        "println:()V".to_string(),
        StubMethodDescriptor::instance_method("println", "()V"),
    );
    methods.insert(
        "println:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Z)V"),
    );
    methods.insert(
        "println:(C)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(C)V"),
    );
    methods.insert(
        "println:(I)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(I)V"),
    );
    methods.insert(
        "println:(J)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(J)V"),
    );
    methods.insert(
        "println:(F)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(F)V"),
    );
    methods.insert(
        "println:(D)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(D)V"),
    );
    methods.insert(
        "println:([C)V".to_string(),
        StubMethodDescriptor::instance_method("println", "([C)V"),
    );
    methods.insert(
        "println:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "println:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Ljava/lang/Object;)V"),
    );

    // printf/format methods
    methods.insert(
        "printf:(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "printf",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;",
        ),
    );
    methods.insert(
        "printf:(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "printf",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;",
        ),
    );
    methods.insert(
        "format:(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;",
        ),
    );
    methods.insert(
        "format:(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "format",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintWriter;",
        ),
    );

    // Appendable methods
    methods.insert(
        "append:(Ljava/lang/CharSequence;)Ljava/io/PrintWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;)Ljava/io/PrintWriter;",
        ),
    );
    methods.insert(
        "append:(Ljava/lang/CharSequence;II)Ljava/io/PrintWriter;".to_string(),
        StubMethodDescriptor::instance_method(
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/PrintWriter;",
        ),
    );
    methods.insert(
        "append:(C)Ljava/io/PrintWriter;".to_string(),
        StubMethodDescriptor::instance_method("append", "(C)Ljava/io/PrintWriter;"),
    );

    StubClass {
        name: "java/io/PrintWriter".to_string(),
        superclass: Some("java/io/Writer".to_string()),
        interfaces: vec![],
        methods,
    }
}

// ============================================================================
// DataInput/DataOutput interfaces
// ============================================================================

/// Create java.io.DataInput interface stub
pub fn create_data_input_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "readFully:([B)V".to_string(),
        StubMethodDescriptor::instance_method("readFully", "([B)V"),
    );
    methods.insert(
        "readFully:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("readFully", "([BII)V"),
    );
    methods.insert(
        "skipBytes:(I)I".to_string(),
        StubMethodDescriptor::instance_method("skipBytes", "(I)I"),
    );
    methods.insert(
        "readBoolean:()Z".to_string(),
        StubMethodDescriptor::instance_method("readBoolean", "()Z"),
    );
    methods.insert(
        "readByte:()B".to_string(),
        StubMethodDescriptor::instance_method("readByte", "()B"),
    );
    methods.insert(
        "readUnsignedByte:()I".to_string(),
        StubMethodDescriptor::instance_method("readUnsignedByte", "()I"),
    );
    methods.insert(
        "readShort:()S".to_string(),
        StubMethodDescriptor::instance_method("readShort", "()S"),
    );
    methods.insert(
        "readUnsignedShort:()I".to_string(),
        StubMethodDescriptor::instance_method("readUnsignedShort", "()I"),
    );
    methods.insert(
        "readChar:()C".to_string(),
        StubMethodDescriptor::instance_method("readChar", "()C"),
    );
    methods.insert(
        "readInt:()I".to_string(),
        StubMethodDescriptor::instance_method("readInt", "()I"),
    );
    methods.insert(
        "readLong:()J".to_string(),
        StubMethodDescriptor::instance_method("readLong", "()J"),
    );
    methods.insert(
        "readFloat:()F".to_string(),
        StubMethodDescriptor::instance_method("readFloat", "()F"),
    );
    methods.insert(
        "readDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("readDouble", "()D"),
    );
    methods.insert(
        "readLine:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readLine", "()Ljava/lang/String;"),
    );
    methods.insert(
        "readUTF:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("readUTF", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/io/DataInput".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.DataOutput interface stub
pub fn create_data_output_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "writeBoolean:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("writeBoolean", "(Z)V"),
    );
    methods.insert(
        "writeByte:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeByte", "(I)V"),
    );
    methods.insert(
        "writeShort:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeShort", "(I)V"),
    );
    methods.insert(
        "writeChar:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeChar", "(I)V"),
    );
    methods.insert(
        "writeInt:(I)V".to_string(),
        StubMethodDescriptor::instance_method("writeInt", "(I)V"),
    );
    methods.insert(
        "writeLong:(J)V".to_string(),
        StubMethodDescriptor::instance_method("writeLong", "(J)V"),
    );
    methods.insert(
        "writeFloat:(F)V".to_string(),
        StubMethodDescriptor::instance_method("writeFloat", "(F)V"),
    );
    methods.insert(
        "writeDouble:(D)V".to_string(),
        StubMethodDescriptor::instance_method("writeDouble", "(D)V"),
    );
    methods.insert(
        "writeBytes:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeBytes", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "writeChars:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeChars", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "writeUTF:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("writeUTF", "(Ljava/lang/String;)V"),
    );

    StubClass {
        name: "java/io/DataOutput".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.ObjectInput interface stub
pub fn create_object_input_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "readObject:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("readObject", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "read:()I".to_string(),
        StubMethodDescriptor::instance_method("read", "()I"),
    );
    methods.insert(
        "read:([B)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([B)I"),
    );
    methods.insert(
        "read:([BII)I".to_string(),
        StubMethodDescriptor::instance_method("read", "([BII)I"),
    );
    methods.insert(
        "skip:(J)J".to_string(),
        StubMethodDescriptor::instance_method("skip", "(J)J"),
    );
    methods.insert(
        "available:()I".to_string(),
        StubMethodDescriptor::instance_method("available", "()I"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/ObjectInput".to_string(),
        superclass: None,
        interfaces: vec![
            "java/io/DataInput".to_string(),
            "java/lang/AutoCloseable".to_string(),
        ],
        methods,
    }
}

/// Create java.io.ObjectOutput interface stub
pub fn create_object_output_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "writeObject:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("writeObject", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "write:(I)V".to_string(),
        StubMethodDescriptor::instance_method("write", "(I)V"),
    );
    methods.insert(
        "write:([B)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([B)V"),
    );
    methods.insert(
        "write:([BII)V".to_string(),
        StubMethodDescriptor::instance_method("write", "([BII)V"),
    );
    methods.insert(
        "flush:()V".to_string(),
        StubMethodDescriptor::instance_method("flush", "()V"),
    );
    methods.insert(
        "close:()V".to_string(),
        StubMethodDescriptor::instance_method("close", "()V"),
    );

    StubClass {
        name: "java/io/ObjectOutput".to_string(),
        superclass: None,
        interfaces: vec![
            "java/io/DataOutput".to_string(),
            "java/lang/AutoCloseable".to_string(),
        ],
        methods,
    }
}

// ============================================================================
// Exceptions
// ============================================================================

/// Create java.io.IOException class stub
pub fn create_io_exception_class() -> StubClass {
    let mut methods = HashMap::new();

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
        name: "java/io/IOException".to_string(),
        superclass: Some("java/lang/Exception".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.FileNotFoundException class stub
pub fn create_file_not_found_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    StubClass {
        name: "java/io/FileNotFoundException".to_string(),
        superclass: Some("java/io/IOException".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.EOFException class stub
pub fn create_eof_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    StubClass {
        name: "java/io/EOFException".to_string(),
        superclass: Some("java/io/IOException".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create java.io.UncheckedIOException class stub
pub fn create_unchecked_io_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/io/IOException;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/io/IOException;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/IOException;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/IOException;)V"),
    );
    methods.insert(
        "getCause:()Ljava/io/IOException;".to_string(),
        StubMethodDescriptor::instance_method("getCause", "()Ljava/io/IOException;"),
    );

    StubClass {
        name: "java/io/UncheckedIOException".to_string(),
        superclass: Some("java/lang/RuntimeException".to_string()),
        interfaces: vec![],
        methods,
    }
}
