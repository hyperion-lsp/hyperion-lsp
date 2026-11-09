// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.lang.invoke.MethodHandles.Lookup
//!
//! Lookup provides a factory for creating method handles for methods and fields
//! that are accessible to a particular class. It enforces access control rules.

use crate::interpreter::errors::{VMError, VMResult};
use crate::interpreter::executor::access_flags::ACC_PUBLIC;
use crate::interpreter::method_area::{same_runtime_package, MethodArea};
use crate::interpreter::method_handles::{MethodHandleKind, RuntimeMethodHandle};
use crate::interpreter::method_type::MethodTypeRef;

/// Access modes for method lookup
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessModes(u32);

impl AccessModes {
    /// PUBLIC: accessible to all
    pub const PUBLIC: u32 = 0x00000001;
    /// PRIVATE: accessible to the class itself
    pub const PRIVATE: u32 = 0x00000002;
    /// PROTECTED: accessible to subclasses
    pub const PROTECTED: u32 = 0x00000004;
    /// PACKAGE: accessible within the same package
    pub const PACKAGE: u32 = 0x00000008;
    /// MODULE: accessible within the module
    pub const MODULE: u32 = 0x00000010;
    /// UNCONDITIONAL: no access restrictions
    pub const UNCONDITIONAL: u32 = 0x00000020;

    /// Create new access modes
    pub fn new(modes: u32) -> Self {
        AccessModes(modes)
    }

    /// Check if a mode is allowed
    pub fn contains(&self, mode: u32) -> bool {
        (self.0 & mode) != 0
    }
}

/// MethodHandles.Lookup factory for method handle creation
#[derive(Clone, Debug)]
pub struct Lookup {
    /// The class that is performing the lookup (for access control)
    pub lookup_class: String,
    /// Allowed access modes
    pub allowed_modes: AccessModes,
}

impl Lookup {
    /// Create a new lookup with full access from within a class
    pub fn in_class(class_name: String) -> Self {
        Lookup {
            lookup_class: class_name,
            allowed_modes: AccessModes::new(
                AccessModes::PUBLIC
                    | AccessModes::PROTECTED
                    | AccessModes::PACKAGE
                    | AccessModes::PRIVATE,
            ),
        }
    }

    /// Create a public lookup with limited access
    pub fn public_lookup() -> Self {
        Lookup {
            lookup_class: String::from("java/lang/invoke/MethodHandles"),
            allowed_modes: AccessModes::new(AccessModes::PUBLIC | AccessModes::UNCONDITIONAL),
        }
    }

    /// Find a static method handle
    pub fn find_static(
        &self,
        class: &str,
        method_name: &str,
        method_type: &MethodTypeRef,
        method_area: &MethodArea,
    ) -> VMResult<RuntimeMethodHandle> {
        // Verify access
        if !self.can_access_class(class, method_area) {
            return Err(VMError::internal(format!(
                "Lookup from {} cannot access static method {}:{}",
                self.lookup_class,
                class,
                method_type.to_descriptor()
            )));
        }

        Ok(RuntimeMethodHandle {
            kind: MethodHandleKind::InvokeStatic {
                class: class.to_string(),
                method: method_name.to_string(),
                descriptor: method_type.to_descriptor(),
            },
            method_type: method_type.clone(),
        })
    }

    /// Find a virtual method handle
    pub fn find_virtual(
        &self,
        class: &str,
        method_name: &str,
        method_type: &MethodTypeRef,
        method_area: &MethodArea,
    ) -> VMResult<RuntimeMethodHandle> {
        // Verify access
        if !self.can_access_class(class, method_area) {
            return Err(VMError::internal(format!(
                "Lookup from {} cannot access virtual method {}:{}",
                self.lookup_class,
                class,
                method_type.to_descriptor()
            )));
        }

        Ok(RuntimeMethodHandle {
            kind: MethodHandleKind::InvokeVirtual {
                class: class.to_string(),
                method: method_name.to_string(),
                descriptor: method_type.to_descriptor(),
            },
            method_type: method_type.clone(),
        })
    }

    /// Find an interface method handle
    pub fn find_interface(
        &self,
        interface: &str,
        method_name: &str,
        method_type: &MethodTypeRef,
        method_area: &MethodArea,
    ) -> VMResult<RuntimeMethodHandle> {
        // Verify access
        if !self.can_access_class(interface, method_area) {
            return Err(VMError::internal(format!(
                "Lookup from {} cannot access interface method {}:{}",
                self.lookup_class,
                interface,
                method_type.to_descriptor()
            )));
        }

        Ok(RuntimeMethodHandle {
            kind: MethodHandleKind::InvokeInterface {
                class: interface.to_string(),
                method: method_name.to_string(),
                descriptor: method_type.to_descriptor(),
            },
            method_type: method_type.clone(),
        })
    }

    /// Find a special method handle (for constructors and private methods)
    pub fn find_special(
        &self,
        class: &str,
        method_name: &str,
        method_type: &MethodTypeRef,
        _method_area: &MethodArea,
    ) -> VMResult<RuntimeMethodHandle> {
        // Special access requires PRIVATE mode
        if !self.allowed_modes.contains(AccessModes::PRIVATE) {
            return Err(VMError::internal(format!(
                "Lookup from {} cannot access special method (requires PRIVATE access)",
                self.lookup_class
            )));
        }

        Ok(RuntimeMethodHandle {
            kind: MethodHandleKind::InvokeSpecial {
                class: class.to_string(),
                method: method_name.to_string(),
                descriptor: method_type.to_descriptor(),
            },
            method_type: method_type.clone(),
        })
    }

    /// Check whether the lookup class may name `class` (JVMS §5.4.4).
    ///
    /// A public class is accessible everywhere. A non-public class is accessible
    /// only from its own runtime package. A class that does not load is allowed,
    /// because bootstrap arguments may name classes that are loaded later. Member
    /// accessibility is not checked here.
    fn can_access_class(&self, class: &str, method_area: &MethodArea) -> bool {
        let Ok(loaded) = method_area.get_class(class) else {
            return true;
        };
        if loaded.modifiers & ACC_PUBLIC != 0 {
            return true;
        }
        same_runtime_package(&self.lookup_class, class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_creation() {
        let lookup = Lookup::in_class("Test".to_string());
        assert_eq!(lookup.lookup_class, "Test");
        assert!(lookup.allowed_modes.contains(AccessModes::PRIVATE));
    }

    #[test]
    fn test_public_lookup() {
        let lookup = Lookup::public_lookup();
        assert!(lookup.allowed_modes.contains(AccessModes::PUBLIC));
        assert!(!lookup.allowed_modes.contains(AccessModes::PRIVATE));
    }

    #[test]
    fn non_public_class_is_accessible_only_from_its_runtime_package() {
        use crate::interpreter::method_area::LoadedClass;

        let mut hidden = LoadedClass::new("com/example/internal/Hidden".to_string());
        hidden.modifiers = 0; // package-private
        let mut shared = LoadedClass::new("com/example/Shared".to_string());
        shared.modifiers = ACC_PUBLIC;
        let method_area = MethodArea::new();
        method_area
            .register_classes(vec![hidden, shared])
            .expect("test classes register");

        let same_package = Lookup::in_class("com/example/internal/Caller".to_string());
        let other_package = Lookup::in_class("com/example/Caller".to_string());

        assert!(same_package.can_access_class("com/example/internal/Hidden", &method_area));
        assert!(!other_package.can_access_class("com/example/internal/Hidden", &method_area));
        assert!(other_package.can_access_class("com/example/Shared", &method_area));
        // Bootstrap arguments may name classes that load later.
        assert!(other_package.can_access_class("com/example/Unloaded", &method_area));
    }
}
