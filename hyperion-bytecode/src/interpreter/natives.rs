// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Native method implementations for JDK classes
//!
//! This module provides Rust implementations of Java native methods
//! that are required for bytecode execution.

mod collections;
pub(crate) mod file;
mod math;
mod numbers;
mod strings;
mod thread;

use crate::interpreter::{Heap, ObjectRef, VMError, VMResult, Value};
use std::collections::HashMap;

pub(crate) use strings::{
    java_format, java_string_char_at, java_string_compare, java_string_hash, java_string_index_of,
    java_string_last_index_of, java_string_replace_range, java_string_substring, java_string_trim,
    java_string_utf16_len,
};

/// Native method implementation
/// Takes heap access for string operations and other heap-dependent functionality
pub type NativeMethod = fn(&mut Heap, &[Value]) -> VMResult<Option<Value>>;

/// Registry of native method implementations
pub struct NativeRegistry {
    methods: HashMap<String, NativeMethod>,
}

impl NativeRegistry {
    /// Create a new native method registry
    pub fn new() -> Self {
        let mut registry = Self {
            methods: HashMap::new(),
        };

        registry.register_jdk_natives();
        super::kotlin_intrinsics::register_kotlin_intrinsics(&mut registry);
        super::coroutine_natives::register_coroutine_natives(&mut registry);
        registry
    }

    /// Register a native method
    pub fn register(
        &mut self,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        method: NativeMethod,
    ) {
        let key = format!("{}.{}:{}", class_name, method_name, descriptor);
        self.methods.insert(key, method);
    }

    /// Look up and invoke a native method
    pub fn invoke(
        &self,
        heap: &mut Heap,
        class_name: &str,
        method_name: &str,
        descriptor: &str,
        args: &[Value],
    ) -> VMResult<Option<Value>> {
        let key = format!("{}.{}:{}", class_name, method_name, descriptor);
        match self.methods.get(&key) {
            Some(method) => method(heap, args),
            None => Err(VMError::UnsatisfiedLinkError(format!(
                "Native method not found: {}.{}{}",
                class_name, method_name, descriptor
            ))),
        }
    }

    /// Check if a method is native
    pub fn is_native(&self, class_name: &str, method_name: &str, descriptor: &str) -> bool {
        let key = format!("{}.{}:{}", class_name, method_name, descriptor);
        self.methods.contains_key(&key)
    }

    /// Register all JDK native methods
    fn register_jdk_natives(&mut self) {
        // java.lang.Object
        self.register("java/lang/Object", "hashCode", "()I", object_hash_code);
        self.register(
            "java/lang/Object",
            "equals",
            "(Ljava/lang/Object;)Z",
            object_equals,
        );
        self.register(
            "java/lang/Object",
            "getClass",
            "()Ljava/lang/Class;",
            object_get_class,
        );

        // java.lang.System
        self.register(
            "java/lang/System",
            "currentTimeMillis",
            "()J",
            system_current_time_millis,
        );
        self.register(
            "java/lang/System",
            "arraycopy",
            "(Ljava/lang/Object;ILjava/lang/Object;II)V",
            system_arraycopy,
        );

        // java.lang.Class
        self.register(
            "java/lang/Class",
            "forName",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            class_for_name,
        );
        self.register(
            "java/lang/Class",
            "getName",
            "()Ljava/lang/String;",
            class_get_name,
        );
        self.register(
            "java/lang/Class",
            "getSimpleName",
            "()Ljava/lang/String;",
            class_get_simple_name,
        );

        // java.io.PrintStream (System.out)
        self.register(
            "java/io/PrintStream",
            "println",
            "(Ljava/lang/String;)V",
            printstream_println,
        );
        self.register(
            "java/io/PrintStream",
            "println",
            "(I)V",
            printstream_println_int,
        );
        self.register(
            "java/io/PrintStream",
            "println",
            "(Z)V",
            printstream_println_boolean,
        );
        self.register(
            "java/io/PrintStream",
            "println",
            "()V",
            printstream_println_newline,
        );

        collections::register(self);

        file::register(self);

        thread::register(self);

        math::register(self);
        numbers::register(self);
        strings::register(self);

        // java.util.Optional factories
        self.register(
            "java/util/Optional",
            "empty",
            "()Ljava/util/Optional;",
            optional_empty,
        );
        self.register(
            "java/util/Optional",
            "of",
            "(Ljava/lang/Object;)Ljava/util/Optional;",
            optional_of,
        );
        self.register(
            "java/util/Optional",
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/Optional;",
            optional_of,
        );

        // java.util.Arrays.asList(T...) -> a (heap-backed) List view.
        self.register(
            "java/util/Arrays",
            "asList",
            "([Ljava/lang/Object;)Ljava/util/List;",
            arrays_as_list,
        );

        // java.util.Arrays.sort / toString for primitive int arrays.
        self.register("java/util/Arrays", "sort", "([I)V", arrays_sort_int);
        self.register(
            "java/util/Arrays",
            "toString",
            "([I)Ljava/lang/String;",
            arrays_to_string_int,
        );
        self.register(
            "java/util/Arrays",
            "toString",
            "([Ljava/lang/Object;)Ljava/lang/String;",
            arrays_to_string_obj,
        );

        // java.util.stream.Stream static factories. The metadata stubs in
        // jdk_stubs/java_stream.rs register these descriptors for class-loading
        // and instanceof checks, but invokestatic dispatch goes through this
        // registry — without entries here, `Stream.of`/`empty`/`ofNullable`
        // throw NoSuchMethod even though the instance ops (filter/map/collect,
        // see executor/handlers/streams.rs::dispatch_stream_method) work fine
        // once a stream object exists.
        self.register(
            "java/util/stream/Stream",
            "empty",
            "()Ljava/util/stream/Stream;",
            stream_empty,
        );
        self.register(
            "java/util/stream/Stream",
            "of",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_single,
        );
        self.register(
            "java/util/stream/Stream",
            "of",
            "([Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_varargs,
        );
        self.register(
            "java/util/stream/Stream",
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_nullable,
        );

        // java.util.stream.IntStream static factories. Registered because the
        // generic stream instance ops (filter/map/sum/count/forEach/...) in
        // dispatch_stream_method are keyed only on method name, not on the
        // "Int"/"Long"/"Double" stream flavor — any `allocate_stream` result
        // dispatches through the same code regardless of element type, so
        // these factories are safe to wire up without further instance-op work.
        self.register(
            "java/util/stream/IntStream",
            "of",
            "(I)Ljava/util/stream/IntStream;",
            int_stream_of_single,
        );
        self.register(
            "java/util/stream/IntStream",
            "of",
            "([I)Ljava/util/stream/IntStream;",
            int_stream_of_array,
        );
        self.register(
            "java/util/stream/IntStream",
            "range",
            "(II)Ljava/util/stream/IntStream;",
            int_stream_range,
        );
        self.register(
            "java/util/stream/IntStream",
            "rangeClosed",
            "(II)Ljava/util/stream/IntStream;",
            int_stream_range_closed,
        );
    }
}

fn arg_i32(args: &[Value]) -> i32 {
    args.first().and_then(|v| v.as_int().ok()).unwrap_or(0)
}

/// Arrays.sort(int[]) — sort in place.
fn arrays_sort_int(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if let Some(Value::Reference(ObjectRef::Array(id))) = args.first() {
        let len = heap.array_length(*id)?;
        let mut vals: Vec<i32> = (0..len)
            .map(|i| heap.array_load(*id, i).and_then(|v| v.as_int()))
            .collect::<VMResult<Vec<_>>>()?;
        vals.sort_unstable();
        for (i, v) in vals.into_iter().enumerate() {
            heap.array_store(*id, i as i32, Value::Int(v))?;
        }
    }
    Ok(None)
}

/// Arrays.toString(int[]) -> "[a, b, c]".
fn arrays_to_string_int(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let s = match args.first() {
        Some(Value::Reference(ObjectRef::Array(id))) => {
            let len = heap.array_length(*id)?;
            let parts: Vec<String> = (0..len)
                .map(|i| heap.array_load(*id, i).map(|v| heap.render_value(&v)))
                .collect::<VMResult<Vec<_>>>()?;
            format!("[{}]", parts.join(", "))
        }
        Some(Value::Reference(ObjectRef::Null)) | None => "null".to_string(),
        _ => "[]".to_string(),
    };
    Ok(Some(Value::Reference(heap.intern_string(&s)?)))
}

/// Arrays.toString(Object[]) -> "[a, b, c]".
fn arrays_to_string_obj(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    arrays_to_string_int(heap, args)
}

/// `Arrays.asList(T...)` — the varargs arrive as a single `Object[]`; copy them
/// into a heap-backed ArrayList.
fn arrays_as_list(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let array = match args.first() {
        Some(Value::Reference(ObjectRef::Array(array))) => *array,
        _ => return Err(VMError::NullPointerException),
    };
    let list = heap.allocate_array_view(array)?;
    Ok(Some(Value::Reference(ObjectRef::Object(list))))
}

/// Allocate a fresh `java.util.Optional` whose field 0 holds `value`.
fn alloc_optional(heap: &mut Heap, value: Value) -> VMResult<Value> {
    let id = heap.allocate_object(crate::interpreter::heap::ClassRef {
        name: "java/util/Optional".to_string(),
        instance_field_count: 1,
        static_field_count: 0,
    })?;
    heap.set_field(id, 0, value)?;
    Ok(Value::Reference(ObjectRef::Object(id)))
}

/// `Optional.empty()` -> an Optional holding null.
fn optional_empty(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    Ok(Some(alloc_optional(
        heap,
        Value::Reference(ObjectRef::Null),
    )?))
}

/// `Optional.of(x)` / `Optional.ofNullable(x)` -> an Optional holding x.
fn optional_of(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let v = args
        .first()
        .cloned()
        .unwrap_or(Value::Reference(ObjectRef::Null));
    Ok(Some(alloc_optional(heap, v)?))
}

// ==== java.util.stream.Stream / IntStream static factories ====
//
// Streams are modeled eagerly as a heap collection (see
// `Heap::allocate_stream`); every factory here just builds the initial
// element snapshot and hands it to `allocate_stream`. Instance ops
// (filter/map/collect/...) are implemented in
// `executor/handlers/streams.rs::dispatch_stream_method`, dispatched purely
// by method name, so they work unchanged for any of these.

/// `Stream.empty()` -> a stream with no elements.
fn stream_empty(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_stream(Vec::new())?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `Stream.of(T t)` — the single-value overload javac emits instead of the
/// varargs form when the call site has exactly one argument.
fn stream_of_single(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let v = args
        .first()
        .cloned()
        .unwrap_or(Value::Reference(ObjectRef::Null));
    let id = heap.allocate_stream(vec![v])?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `Stream.of(T... values)` — the varargs arrive as a single `Object[]`;
/// unpack into the stream's element snapshot (mirrors `arrays_as_list`).
fn stream_of_varargs(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let mut items = Vec::new();
    if let Some(Value::Reference(ObjectRef::Array(aid))) = args.first() {
        let len = heap.array_length(*aid)?;
        for i in 0..len {
            items.push(heap.array_load(*aid, i)?);
        }
    }
    let id = heap.allocate_stream(items)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `Stream.ofNullable(x)` -> an empty stream if `x` is null, else a
/// single-element stream holding `x`.
fn stream_of_nullable(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let v = args
        .first()
        .cloned()
        .unwrap_or(Value::Reference(ObjectRef::Null));
    let items = match v {
        Value::Reference(ObjectRef::Null) => Vec::new(),
        other => vec![other],
    };
    let id = heap.allocate_stream(items)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `IntStream.of(int)` — single-value overload.
fn int_stream_of_single(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let v = Value::Int(arg_i32(args));
    let id = heap.allocate_stream(vec![v])?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `IntStream.of(int... values)` — the varargs arrive as a single `int[]`.
fn int_stream_of_array(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let mut items = Vec::new();
    if let Some(Value::Reference(ObjectRef::Array(aid))) = args.first() {
        let len = heap.array_length(*aid)?;
        for i in 0..len {
            items.push(heap.array_load(*aid, i)?);
        }
    }
    let id = heap.allocate_stream(items)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `IntStream.range(start, end)` — exclusive of `end`.
fn int_stream_range(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let start = arg_i32(args);
    let end = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(start);
    let items: Vec<Value> = (start..end).map(Value::Int).collect();
    let id = heap.allocate_stream(items)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// `IntStream.rangeClosed(start, end)` — inclusive of `end`.
fn int_stream_range_closed(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let start = arg_i32(args);
    let end = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(start);
    let items: Vec<Value> = (start..=end).map(Value::Int).collect();
    let id = heap.allocate_stream(items)?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

// ==== JDK Native Method Implementations ====

/// java.lang.Object.hashCode()I
/// `Object.equals`: identity. A class that overrides `equals` is dispatched to its own method
/// before this one is reached.
fn object_equals(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let [this, other] = args else {
        return Err(VMError::NoSuchMethod(
            "equals requires 'this' and one argument".to_string(),
        ));
    };
    match (this, other) {
        (Value::Reference(ObjectRef::Null), _) => Err(VMError::NullPointerException),
        (Value::Reference(this), Value::Reference(other)) => {
            Ok(Some(Value::Int(i32::from(this == other))))
        }
        _ => Err(VMError::TypeMismatch {
            expected: "object reference".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

fn object_hash_code(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "hashCode requires 'this'".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) | Value::Reference(ObjectRef::Array(id)) => {
            // Use the object ID as the hash code
            Ok(Some(Value::Int(id.0 as i32)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "object reference".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.Object.getClass()Ljava/lang/Class;
fn object_get_class(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "getClass requires 'this'".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) | Value::Reference(ObjectRef::Array(id)) => {
            // Get the class name of the object
            let class_name = heap.get_object_class_name(*id)?;
            // Intern and return a Class object for this class
            let class_obj = heap.intern_class(&class_name)?;
            Ok(Some(Value::Reference(class_obj)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "object reference".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.System.currentTimeMillis()J
fn system_current_time_millis(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| VMError::InternalError(format!("Failed to get system time: {}", e)))?
        .as_millis() as i64;

    Ok(Some(Value::Long(millis)))
}

/// java.lang.System.arraycopy(Ljava/lang/Object;ILjava/lang/Object;II)V
fn system_arraycopy(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    let src = match args.first() {
        Some(Value::Reference(ObjectRef::Array(id))) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let src_pos = args.get(1).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let dest = match args.get(2) {
        Some(Value::Reference(ObjectRef::Array(id))) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let dest_pos = args.get(3).and_then(|v| v.as_int().ok()).unwrap_or(0);
    let length = args.get(4).and_then(|v| v.as_int().ok()).unwrap_or(0);
    // Snapshot the source range first so an overlapping src==dest copy is correct.
    let buf: Vec<Value> = (0..length)
        .map(|i| heap.array_load(src, src_pos + i))
        .collect::<VMResult<Vec<_>>>()?;
    for (i, v) in buf.into_iter().enumerate() {
        heap.array_store(dest, dest_pos + i as i32, v)?;
    }
    Ok(None)
}

/// java.lang.Class.forName(Ljava/lang/String;)Ljava/lang/Class;
fn class_for_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "forName requires class name argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            // Get the class name from the String object
            let class_name = heap.get_string(*id)?;
            // Convert dots to slashes for internal format
            let internal_name = class_name.replace('.', "/");
            // Intern and return a Class object
            let class_obj = heap.intern_class(&internal_name)?;
            Ok(Some(Value::Reference(class_obj)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "String".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.Class.getName()Ljava/lang/String;
fn class_get_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("getName requires 'this'".to_string()));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            // Get the represented class name from the Class object
            let class_name = heap.get_class_name(*id)?;
            // Intern and return as a String
            let string_ref = heap.intern_string(&class_name)?;
            Ok(Some(Value::Reference(string_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "Class".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.lang.Class.getSimpleName()Ljava/lang/String;
fn class_get_simple_name(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod(
            "getSimpleName requires 'this'".to_string(),
        ));
    }

    match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => {
            // Get the represented class name from the Class object
            let class_name = heap.get_class_name(*id)?;
            // Simple name = the part after the last package ('.'/'/') *and*
            // enclosing-class ('$') separator (JLS: `Outer$Inner` -> `Inner`).
            let simple_name = class_name
                .rsplit(['.', '/', '$'])
                .next()
                .unwrap_or(&class_name);
            // Intern and return as a String
            let string_ref = heap.intern_string(simple_name)?;
            Ok(Some(Value::Reference(string_ref)))
        }
        Value::Reference(ObjectRef::Null) => Err(VMError::NullPointerException),
        _ => Err(VMError::TypeMismatch {
            expected: "Class".to_string(),
            got: "primitive".to_string(),
        }),
    }
}

/// java.io.PrintStream.println(Ljava/lang/String;)V
fn printstream_println(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "println requires 'this' and string argument".to_string(),
        ));
    }

    // args[0] is 'this' (PrintStream), args[1] is the string to print
    match &args[1] {
        Value::Reference(ObjectRef::Object(id)) => {
            // Extract actual string from heap
            match heap.get_string(*id) {
                Ok(s) => println!("{}", s),
                Err(_) => println!("[String@{}]", id.0),
            }
        }
        Value::Reference(ObjectRef::Null) => {
            println!("null");
        }
        _ => {
            return Err(VMError::TypeMismatch {
                expected: "String".to_string(),
                got: format!("{:?}", args[1]),
            });
        }
    }

    Ok(None) // void return
}

/// java.io.PrintStream.println(I)V
fn printstream_println_int(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "println requires 'this' and int argument".to_string(),
        ));
    }

    let value = args[1].as_int()?;
    println!("{}", value);

    Ok(None) // void return
}

/// java.io.PrintStream.println(Z)V
fn printstream_println_boolean(_heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "println requires 'this' and boolean argument".to_string(),
        ));
    }

    // Boolean is stored as int (0 = false, non-zero = true)
    let value = args[1].as_int()?;
    println!("{}", if value != 0 { "true" } else { "false" });

    Ok(None) // void return
}

/// java.io.PrintStream.println()V
fn printstream_println_newline(_heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    println!();
    Ok(None) // void return
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_heap() -> Heap {
        Heap::new(1024 * 1024)
    }

    #[test]
    fn test_native_registry_creation() {
        let registry = NativeRegistry::new();
        assert!(registry.is_native("java/lang/Object", "hashCode", "()I"));
        assert!(registry.is_native("java/lang/System", "currentTimeMillis", "()J"));
    }

    #[test]
    fn test_object_hash_code() {
        let mut heap = test_heap();
        let result = object_hash_code(
            &mut heap,
            &[Value::Reference(ObjectRef::Object(
                crate::interpreter::HeapId(42),
            ))],
        );
        assert!(result.is_ok());
        match result.unwrap() {
            Some(Value::Int(h)) => assert_eq!(h, 42),
            _ => panic!("Expected int hash code"),
        }
    }

    #[test]
    fn test_system_current_time_millis() {
        let mut heap = test_heap();
        let result = system_current_time_millis(&mut heap, &[]);
        assert!(result.is_ok());
        match result.unwrap() {
            Some(Value::Long(millis)) => assert!(millis > 0),
            _ => panic!("Expected long milliseconds"),
        }
    }

    #[test]
    fn test_object_hash_code_null_reference() {
        let mut heap = test_heap();
        let result = object_hash_code(&mut heap, &[Value::Reference(ObjectRef::Null)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_native_lookup() {
        let registry = NativeRegistry::new();

        // Test successful lookup
        assert!(registry.is_native("java/lang/String", "intern", "()Ljava/lang/String;"));

        // Test failed lookup
        assert!(!registry.is_native("java/lang/String", "unknown", "()V"));
    }

    // ==== java.util.stream.Stream / IntStream static factories ====

    fn stream_items(heap: &Heap, v: Option<Value>) -> Vec<Value> {
        match v {
            Some(Value::Reference(ObjectRef::Object(id))) => heap.collection_snapshot(id).unwrap(),
            other => panic!("expected a stream object, got {other:?}"),
        }
    }

    #[test]
    fn test_stream_factories_registered() {
        let registry = NativeRegistry::new();
        assert!(registry.is_native(
            "java/util/stream/Stream",
            "empty",
            "()Ljava/util/stream/Stream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/Stream",
            "of",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/Stream",
            "of",
            "([Ljava/lang/Object;)Ljava/util/stream/Stream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/Stream",
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/IntStream",
            "of",
            "(I)Ljava/util/stream/IntStream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/IntStream",
            "of",
            "([I)Ljava/util/stream/IntStream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/IntStream",
            "range",
            "(II)Ljava/util/stream/IntStream;"
        ));
        assert!(registry.is_native(
            "java/util/stream/IntStream",
            "rangeClosed",
            "(II)Ljava/util/stream/IntStream;"
        ));
    }

    /// `Stream.empty()` — the returned stream must snapshot to zero elements
    /// (edge case explicitly called out: an empty stream is a valid, distinct
    /// object, not null and not an error).
    #[test]
    fn test_stream_empty() {
        let mut heap = test_heap();
        let result = stream_empty(&mut heap, &[]).unwrap();
        let items = stream_items(&heap, result);
        assert!(items.is_empty());
    }

    #[test]
    fn test_stream_of_single() {
        let mut heap = test_heap();
        let s = heap.intern_string("solo").unwrap();
        let result = stream_of_single(&mut heap, &[Value::Reference(s)]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0], Value::Reference(s));
    }

    #[test]
    fn test_stream_of_varargs() {
        let mut heap = test_heap();
        let a = heap.intern_string("a").unwrap();
        let b = heap.intern_string("b").unwrap();
        let arr = heap
            .allocate_array(
                crate::interpreter::heap::ArrayElementType::Reference(
                    "java/lang/Object".to_string(),
                ),
                2,
            )
            .unwrap();
        heap.array_store(arr, 0, Value::Reference(a)).unwrap();
        heap.array_store(arr, 1, Value::Reference(b)).unwrap();
        let result =
            stream_of_varargs(&mut heap, &[Value::Reference(ObjectRef::Array(arr))]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(items, vec![Value::Reference(a), Value::Reference(b)]);
    }

    /// `Stream.ofNullable(null)` must yield an *empty* stream, not a
    /// single-element stream holding null (edge case explicitly called out).
    #[test]
    fn test_stream_of_nullable_null() {
        let mut heap = test_heap();
        let result = stream_of_nullable(&mut heap, &[Value::Reference(ObjectRef::Null)]).unwrap();
        let items = stream_items(&heap, result);
        assert!(items.is_empty());
    }

    #[test]
    fn test_stream_of_nullable_present() {
        let mut heap = test_heap();
        let s = heap.intern_string("x").unwrap();
        let result = stream_of_nullable(&mut heap, &[Value::Reference(s)]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(items, vec![Value::Reference(s)]);
    }

    #[test]
    fn test_int_stream_of_single() {
        let mut heap = test_heap();
        let result = int_stream_of_single(&mut heap, &[Value::Int(7)]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(items, vec![Value::Int(7)]);
    }

    #[test]
    fn test_int_stream_range() {
        let mut heap = test_heap();
        let result = int_stream_range(&mut heap, &[Value::Int(2), Value::Int(5)]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(items, vec![Value::Int(2), Value::Int(3), Value::Int(4)]);
    }

    #[test]
    fn test_int_stream_range_closed() {
        let mut heap = test_heap();
        let result = int_stream_range_closed(&mut heap, &[Value::Int(2), Value::Int(5)]).unwrap();
        let items = stream_items(&heap, result);
        assert_eq!(
            items,
            vec![Value::Int(2), Value::Int(3), Value::Int(4), Value::Int(5)]
        );
    }

    #[test]
    fn test_int_stream_range_empty_when_start_ge_end() {
        let mut heap = test_heap();
        let result = int_stream_range(&mut heap, &[Value::Int(5), Value::Int(5)]).unwrap();
        let items = stream_items(&heap, result);
        assert!(items.is_empty());
    }
}
