// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Heap-backed native implementations for core `java.util` collections.

use super::{Heap, NativeRegistry, ObjectRef, VMError, VMResult, Value};

pub(super) fn register(registry: &mut NativeRegistry) {
    registry.register("java/util/ArrayList", "<init>", "()V", arraylist_init);
    registry.register(
        "java/util/ArrayList",
        "add",
        "(Ljava/lang/Object;)Z",
        arraylist_add,
    );
    registry.register(
        "java/util/ArrayList",
        "get",
        "(I)Ljava/lang/Object;",
        arraylist_get,
    );
    registry.register(
        "java/util/ArrayList",
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        arraylist_set,
    );
    registry.register(
        "java/util/ArrayList",
        "remove",
        "(I)Ljava/lang/Object;",
        arraylist_remove,
    );
    registry.register("java/util/ArrayList", "size", "()I", arraylist_size);
    // Groovy uses Object return type for generic methods.
    registry.register(
        "java/util/ArrayList",
        "size",
        "()Ljava/lang/Object;",
        arraylist_size_boxed,
    );
    registry.register("java/util/ArrayList", "clear", "()V", arraylist_clear);
    registry.register("java/util/ArrayList", "isEmpty", "()Z", arraylist_is_empty);

    registry.register("java/util/HashMap", "<init>", "()V", hashmap_init);
    registry.register(
        "java/util/HashMap",
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        hashmap_put,
    );
    registry.register(
        "java/util/HashMap",
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        hashmap_get,
    );
    registry.register("java/util/HashMap", "size", "()I", hashmap_size);
    registry.register(
        "java/util/HashMap",
        "containsKey",
        "(Ljava/lang/Object;)Z",
        hashmap_contains_key,
    );
    registry.register("java/util/HashMap", "isEmpty", "()Z", hashmap_is_empty);

    registry.register("java/util/HashSet", "<init>", "()V", hashset_init);
    registry.register(
        "java/util/HashSet",
        "add",
        "(Ljava/lang/Object;)Z",
        hashset_add,
    );
    registry.register(
        "java/util/HashSet",
        "contains",
        "(Ljava/lang/Object;)Z",
        hashset_contains,
    );
    registry.register("java/util/HashSet", "size", "()I", hashset_size);
    registry.register("java/util/HashSet", "isEmpty", "()Z", hashset_is_empty);

    registry.register("java/util/LinkedList", "<init>", "()V", linkedlist_init);
}

// ==== java.util.ArrayList Natives ====

/// java.util.ArrayList.<init>()V
fn arraylist_init(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_arraylist()?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// java.util.ArrayList.add(Ljava/lang/Object;)Z
fn arraylist_add(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "add requires 'this' and element".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let result = heap.arraylist_add(this_id, args[1].clone())?;
    Ok(Some(Value::Int(if result { 1 } else { 0 })))
}

/// java.util.ArrayList.get(I)Ljava/lang/Object;
fn arraylist_get(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "get requires 'this' and index".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let index = args[1].as_int()?;
    let value = heap.arraylist_get(this_id, index)?;
    Ok(Some(value))
}

/// java.util.ArrayList.set(ILjava/lang/Object;)Ljava/lang/Object;
fn arraylist_set(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 3 {
        return Err(VMError::NoSuchMethod(
            "set requires 'this', index, and element".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let index = args[1].as_int()?;
    let old = heap.arraylist_set(this_id, index, args[2].clone())?;
    Ok(Some(old))
}

/// java.util.ArrayList.remove(I)Ljava/lang/Object;
fn arraylist_remove(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "remove requires 'this' and index".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let index = args[1].as_int()?;
    let removed = heap.arraylist_remove(this_id, index)?;
    Ok(Some(removed))
}

/// java.util.ArrayList.size()I
fn arraylist_size(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("size requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.arraylist_size(this_id)?;
    Ok(Some(Value::Int(size)))
}

/// java.util.ArrayList.size()Ljava/lang/Object; - Returns boxed Integer for Groovy
fn arraylist_size_boxed(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("size requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.arraylist_size(this_id)?;
    // Return boxed Integer for Groovy compatibility
    let boxed_id = heap.allocate_integer(size)?;
    Ok(Some(Value::Reference(ObjectRef::Object(boxed_id))))
}

/// java.util.ArrayList.clear()V
fn arraylist_clear(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("clear requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    heap.arraylist_clear(this_id)?;
    Ok(None)
}

/// java.util.ArrayList.isEmpty()Z
fn arraylist_is_empty(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("isEmpty requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.arraylist_size(this_id)?;
    Ok(Some(Value::Int(if size == 0 { 1 } else { 0 })))
}

// ==== java.util.HashMap Natives ====

/// java.util.HashMap.<init>()V
fn hashmap_init(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_hashmap()?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// java.util.HashMap.put(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;
fn hashmap_put(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 3 {
        return Err(VMError::NoSuchMethod(
            "put requires 'this', key, and value".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let old = heap.hashmap_put(this_id, args[1].clone(), args[2].clone())?;
    Ok(Some(old))
}

/// java.util.HashMap.get(Ljava/lang/Object;)Ljava/lang/Object;
fn hashmap_get(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "get requires 'this' and key".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let value = heap.hashmap_get(this_id, &args[1])?;
    Ok(Some(value))
}

/// java.util.HashMap.size()I
fn hashmap_size(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("size requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.hashmap_size(this_id)?;
    Ok(Some(Value::Int(size)))
}

/// java.util.HashMap.containsKey(Ljava/lang/Object;)Z
fn hashmap_contains_key(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "containsKey requires 'this' and key".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let contains = heap.hashmap_contains_key(this_id, &args[1])?;
    Ok(Some(Value::Int(if contains { 1 } else { 0 })))
}

/// java.util.HashMap.isEmpty()Z
fn hashmap_is_empty(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("isEmpty requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.hashmap_size(this_id)?;
    Ok(Some(Value::Int(if size == 0 { 1 } else { 0 })))
}

// ==== java.util.HashSet Natives ====

/// java.util.HashSet.<init>()V
fn hashset_init(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_hashset()?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}

/// java.util.HashSet.add(Ljava/lang/Object;)Z
fn hashset_add(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "add requires 'this' and element".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let added = heap.hashset_add(this_id, args[1].clone())?;
    Ok(Some(Value::Int(if added { 1 } else { 0 })))
}

/// java.util.HashSet.contains(Ljava/lang/Object;)Z
fn hashset_contains(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.len() < 2 {
        return Err(VMError::NoSuchMethod(
            "contains requires 'this' and element".to_string(),
        ));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let contains = heap.hashset_contains(this_id, &args[1])?;
    Ok(Some(Value::Int(if contains { 1 } else { 0 })))
}

/// java.util.HashSet.size()I
fn hashset_size(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("size requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.hashset_size(this_id)?;
    Ok(Some(Value::Int(size)))
}

/// java.util.HashSet.isEmpty()Z
fn hashset_is_empty(heap: &mut Heap, args: &[Value]) -> VMResult<Option<Value>> {
    if args.is_empty() {
        return Err(VMError::NoSuchMethod("isEmpty requires 'this'".to_string()));
    }
    let this_id = match &args[0] {
        Value::Reference(ObjectRef::Object(id)) => *id,
        _ => return Err(VMError::NullPointerException),
    };
    let size = heap.hashset_size(this_id)?;
    Ok(Some(Value::Int(if size == 0 { 1 } else { 0 })))
}

// ==== java.util.LinkedList Natives ====

/// java.util.LinkedList.<init>()V
fn linkedlist_init(heap: &mut Heap, _args: &[Value]) -> VMResult<Option<Value>> {
    let id = heap.allocate_linkedlist()?;
    Ok(Some(Value::Reference(ObjectRef::Object(id))))
}
