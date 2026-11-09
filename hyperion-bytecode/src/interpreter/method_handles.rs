// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Runtime representation of java.lang.invoke.MethodHandle
//!
//! MethodHandle is a typed, directly executable reference to a method, constructor,
//! or field. It's created by the bootstrap method and invoked via invokeExact.

use crate::interpreter::method_type::MethodTypeRef;
use std::collections::HashMap;

/// Unique identifier for a method handle in the store
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MethodHandleId(pub u32);

/// Kinds of method handles (from JVMS 5.4.3.5)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MethodHandleKind {
    // Field access
    GetField {
        class: String,
        field: String,
        descriptor: String, // Field type (e.g., "I", "Ljava/lang/String;")
    },
    GetStatic {
        class: String,
        field: String,
        descriptor: String,
    },
    PutField {
        class: String,
        field: String,
        descriptor: String,
    },
    PutStatic {
        class: String,
        field: String,
        descriptor: String,
    },
    // Method invocation
    InvokeVirtual {
        class: String,
        method: String,
        descriptor: String, // Method descriptor "(params)return"
    },
    InvokeStatic {
        class: String,
        method: String,
        descriptor: String,
    },
    InvokeSpecial {
        class: String,
        method: String,
        descriptor: String,
    },
    NewInvokeSpecial {
        class: String,
        method: String,
        descriptor: String, // Constructor descriptor "(...) V"
    },
    InvokeInterface {
        class: String,
        method: String,
        descriptor: String,
    },
}

/// Runtime representation of a MethodHandle
#[derive(Clone, Debug)]
pub struct RuntimeMethodHandle {
    /// The kind of handle (field access, method invocation, etc.)
    pub kind: MethodHandleKind,
    /// The type of this handle (parameter and return types)
    pub method_type: MethodTypeRef,
}

/// Storage and factory for method handles
pub struct MethodHandleStore {
    handles: HashMap<MethodHandleId, RuntimeMethodHandle>,
    next_id: u32,
}

impl MethodHandleStore {
    /// Create a new empty method handle store
    pub fn new() -> Self {
        MethodHandleStore {
            handles: HashMap::new(),
            next_id: 1, // Start from 1, 0 reserved for invalid
        }
    }

    /// Create a method handle from a handle kind and method type
    pub fn create_handle(
        &mut self,
        kind: MethodHandleKind,
        method_type: MethodTypeRef,
    ) -> MethodHandleId {
        let id = MethodHandleId(self.next_id);
        self.next_id += 1;

        self.handles
            .insert(id, RuntimeMethodHandle { kind, method_type });

        id
    }

    /// Get a method handle by ID
    pub fn get(&self, id: MethodHandleId) -> Option<&RuntimeMethodHandle> {
        self.handles.get(&id)
    }

    /// Get a mutable reference to a method handle
    pub fn get_mut(&mut self, id: MethodHandleId) -> Option<&mut RuntimeMethodHandle> {
        self.handles.get_mut(&id)
    }

    /// Get the number of stored handles
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

impl Default for MethodHandleStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_retrieve_handle() {
        let mut store = MethodHandleStore::new();
        let method_type = MethodTypeRef::from_descriptor("(I)I").unwrap();
        let kind = MethodHandleKind::InvokeStatic {
            class: "java/lang/Math".to_string(),
            method: "abs".to_string(),
            descriptor: "(I)I".to_string(),
        };

        let id = store.create_handle(kind.clone(), method_type.clone());
        let handle = store.get(id).unwrap();

        assert_eq!(handle.kind, kind);
        assert_eq!(handle.method_type, method_type);
    }

    #[test]
    fn test_handle_ids_are_unique() {
        let mut store = MethodHandleStore::new();
        let mt = MethodTypeRef::from_descriptor("()V").unwrap();
        let kind = MethodHandleKind::InvokeStatic {
            class: "Test".to_string(),
            method: "test".to_string(),
            descriptor: "()V".to_string(),
        };

        let id1 = store.create_handle(kind.clone(), mt.clone());
        let id2 = store.create_handle(kind.clone(), mt.clone());
        let id3 = store.create_handle(kind.clone(), mt.clone());

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_eq!(store.len(), 3);
    }
}
