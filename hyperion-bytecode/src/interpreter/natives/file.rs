// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.io.File. The path is kept in field 0 of the object, and the file-system queries
//! read it back (see `executor/handlers/native_virtual.rs`).

use super::{Heap, NativeRegistry, ObjectRef, VMError, VMResult, Value};

/// Field offset of the path string in a `java/io/File` object.
pub(crate) const PATH_FIELD: usize = 0;

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register(
        "java/io/File",
        "<init>",
        "(Ljava/lang/String;)V",
        file_init_with_path,
    );
}

/// java.io.File.<init>(String): remembers the path the queries answer for.
fn file_init_with_path(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let (Some(Value::Reference(ObjectRef::Object(file))), Some(path)) = (args.first(), args.get(1))
    else {
        return Err(VMError::NoSuchMethod(
            "File.<init>(String) requires 'this' and path".to_string(),
        ));
    };
    if matches!(path, Value::Reference(ObjectRef::Null)) {
        return Err(VMError::NullPointerException);
    }
    heap.set_field(*file, PATH_FIELD, path.clone())?;
    Ok(None)
}
