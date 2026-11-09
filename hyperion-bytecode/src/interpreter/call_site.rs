// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Call Site Resolution for invokedynamic
//!
//! This module implements the invokedynamic call site mechanism.
//! A CallSite represents a resolved dynamic method call that can be cached and reused.

use super::errors::VMError;
use std::fmt;
use std::sync::Arc;

/// Handler function type for native method handles.
type NativeHandler = dyn Fn(&[crate::interpreter::Value]) -> Result<crate::interpreter::Value, VMError>
    + Send
    + Sync;

/// Call site mutability type
/// Per JVM spec §5.4.3.6 and java.lang.invoke package:
/// - ConstantCallSite: Target never changes (safe to cache permanently)
/// - MutableCallSite: Target can change via setTarget() (unsafe to cache)
/// - VolatileCallSite: Like MutableCallSite but with volatile semantics
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallSiteKind {
    /// Immutable call site - safe to cache
    Constant,
    /// Mutable call site - must not cache
    Mutable,
    /// Volatile mutable call site - must not cache
    Volatile,
}

/// Represents a resolved call site from invokedynamic instruction
/// Can be cached and reused across multiple invocations (only if Constant)
#[derive(Clone)]
pub struct CallSite {
    /// The bound method handle target
    pub target: MethodHandle,
    /// Name from the invokedynamic instruction
    pub name: String,
    /// Method type descriptor (e.g., "(I)V")
    pub method_type: String,
    /// Mutability kind - determines if caching is safe
    /// Per JVM spec §5.4.3.6, only ConstantCallSite can be cached
    pub kind: CallSiteKind,
}

/// Represents a method handle (like a typed function pointer)
/// Different kinds support different invocation patterns
#[derive(Clone)]
pub enum MethodHandle {
    /// Direct invocation of a static method
    InvokeStatic {
        class_name: String,
        method_name: String,
        descriptor: String,
    },
    /// Virtual method invocation (dispatches via vtable)
    InvokeVirtual {
        class_name: String,
        method_name: String,
        descriptor: String,
    },
    /// Interface method invocation (dispatches via itable)
    InvokeInterface {
        class_name: String,
        method_name: String,
        descriptor: String,
    },
    /// Constructor invocation (invokespecial <init>)
    NewInvokeSpecial {
        class_name: String,
        descriptor: String,
    },
    /// Instance field getter
    GetField {
        class_name: String,
        field_name: String,
        descriptor: String,
    },
    /// Instance field setter
    PutField {
        class_name: String,
        field_name: String,
        descriptor: String,
    },
    /// Static field getter
    GetStatic {
        class_name: String,
        field_name: String,
        descriptor: String,
    },
    /// Static field setter
    PutStatic {
        class_name: String,
        field_name: String,
        descriptor: String,
    },
    /// Native/stub method (from Gradle stubs, etc.)
    /// Handler takes stack values and returns result
    Native { handler: Arc<NativeHandler> },
}

impl MethodHandle {
    /// Create a static invoke handle
    pub fn invoke_static(class: &str, method: &str, descriptor: &str) -> Self {
        MethodHandle::InvokeStatic {
            class_name: class.to_string(),
            method_name: method.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a virtual invoke handle
    pub fn invoke_virtual(class: &str, method: &str, descriptor: &str) -> Self {
        MethodHandle::InvokeVirtual {
            class_name: class.to_string(),
            method_name: method.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create an interface invoke handle
    pub fn invoke_interface(class: &str, method: &str, descriptor: &str) -> Self {
        MethodHandle::InvokeInterface {
            class_name: class.to_string(),
            method_name: method.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a constructor invoke handle
    pub fn new_invoke_special(class: &str, descriptor: &str) -> Self {
        MethodHandle::NewInvokeSpecial {
            class_name: class.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a field getter handle
    pub fn get_field(class: &str, field: &str, descriptor: &str) -> Self {
        MethodHandle::GetField {
            class_name: class.to_string(),
            field_name: field.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a field setter handle
    pub fn put_field(class: &str, field: &str, descriptor: &str) -> Self {
        MethodHandle::PutField {
            class_name: class.to_string(),
            field_name: field.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a static field getter handle
    pub fn get_static(class: &str, field: &str, descriptor: &str) -> Self {
        MethodHandle::GetStatic {
            class_name: class.to_string(),
            field_name: field.to_string(),
            descriptor: descriptor.to_string(),
        }
    }

    /// Create a static field setter handle
    pub fn put_static(class: &str, field: &str, descriptor: &str) -> Self {
        MethodHandle::PutStatic {
            class_name: class.to_string(),
            field_name: field.to_string(),
            descriptor: descriptor.to_string(),
        }
    }
}

impl fmt::Debug for MethodHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            } => {
                write!(
                    f,
                    "InvokeStatic({}.{}{})",
                    class_name, method_name, descriptor
                )
            }
            MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            } => {
                write!(
                    f,
                    "InvokeVirtual({}.{}{})",
                    class_name, method_name, descriptor
                )
            }
            MethodHandle::InvokeInterface {
                class_name,
                method_name,
                descriptor,
            } => {
                write!(
                    f,
                    "InvokeInterface({}.{}{})",
                    class_name, method_name, descriptor
                )
            }
            MethodHandle::NewInvokeSpecial {
                class_name,
                descriptor,
            } => {
                write!(f, "NewInvokeSpecial({}<init>{})", class_name, descriptor)
            }
            MethodHandle::GetField {
                class_name,
                field_name,
                descriptor,
            } => {
                write!(f, "GetField({}.{}: {})", class_name, field_name, descriptor)
            }
            MethodHandle::PutField {
                class_name,
                field_name,
                descriptor,
            } => {
                write!(f, "PutField({}.{}: {})", class_name, field_name, descriptor)
            }
            MethodHandle::GetStatic {
                class_name,
                field_name,
                descriptor,
            } => {
                write!(
                    f,
                    "GetStatic({}.{}: {})",
                    class_name, field_name, descriptor
                )
            }
            MethodHandle::PutStatic {
                class_name,
                field_name,
                descriptor,
            } => {
                write!(
                    f,
                    "PutStatic({}.{}: {})",
                    class_name, field_name, descriptor
                )
            }
            MethodHandle::Native { .. } => {
                write!(f, "Native(<handler>)")
            }
        }
    }
}

impl CallSite {
    /// Create a new call site with a static method handle
    /// Defaults to Constant call site (immutable, safe to cache)
    pub fn static_method(
        name: &str,
        class: &str,
        method: &str,
        descriptor: &str,
        method_type: &str,
    ) -> Self {
        CallSite {
            target: MethodHandle::invoke_static(class, method, descriptor),
            name: name.to_string(),
            method_type: method_type.to_string(),
            kind: CallSiteKind::Constant,
        }
    }

    /// Create a new call site with a virtual method handle
    /// Defaults to Constant call site (immutable, safe to cache)
    pub fn virtual_method(
        name: &str,
        class: &str,
        method: &str,
        descriptor: &str,
        method_type: &str,
    ) -> Self {
        CallSite {
            target: MethodHandle::invoke_virtual(class, method, descriptor),
            name: name.to_string(),
            method_type: method_type.to_string(),
            kind: CallSiteKind::Constant,
        }
    }

    /// Check if this call site can be safely cached
    /// Per JVM spec §5.4.3.6, only ConstantCallSite is immutable
    pub fn is_cacheable(&self) -> bool {
        matches!(self.kind, CallSiteKind::Constant)
    }

    /// Validate that the CallSite has a valid, non-empty target
    /// Per JVM spec §6.5.invokedynamic: bootstrap must return non-null CallSite
    /// with compatible MethodHandle target
    pub fn validate(&self) -> Result<(), VMError> {
        // Validate that name and method_type are non-empty
        if self.name.is_empty() {
            return Err(VMError::BootstrapMethodError(
                "CallSite name cannot be empty".to_string(),
            ));
        }
        if self.method_type.is_empty() {
            return Err(VMError::BootstrapMethodError(
                "CallSite method_type cannot be empty".to_string(),
            ));
        }

        // Validate that the target MethodHandle has valid references
        match &self.target {
            MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            }
            | MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            }
            | MethodHandle::InvokeInterface {
                class_name,
                method_name,
                descriptor,
            } => {
                if class_name.is_empty() || method_name.is_empty() || descriptor.is_empty() {
                    return Err(VMError::BootstrapMethodError(
                        "MethodHandle has empty class, method, or descriptor".to_string(),
                    ));
                }
            }
            MethodHandle::NewInvokeSpecial {
                class_name,
                descriptor,
            } => {
                if class_name.is_empty() || descriptor.is_empty() {
                    return Err(VMError::BootstrapMethodError(
                        "NewInvokeSpecial has empty class or descriptor".to_string(),
                    ));
                }
            }
            MethodHandle::GetField {
                class_name,
                field_name,
                descriptor,
            }
            | MethodHandle::PutField {
                class_name,
                field_name,
                descriptor,
            }
            | MethodHandle::GetStatic {
                class_name,
                field_name,
                descriptor,
            }
            | MethodHandle::PutStatic {
                class_name,
                field_name,
                descriptor,
            } => {
                if class_name.is_empty() || field_name.is_empty() || descriptor.is_empty() {
                    return Err(VMError::BootstrapMethodError(
                        "Field MethodHandle has empty class, field, or descriptor".to_string(),
                    ));
                }
            }
            MethodHandle::Native { .. } => {
                // Native handlers are always valid (Rust function pointer)
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_handle_invoke_static() {
        let handle =
            MethodHandle::invoke_static("java/lang/String", "valueOf", "(I)Ljava/lang/String;");
        match handle {
            MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            } => {
                assert_eq!(class_name, "java/lang/String");
                assert_eq!(method_name, "valueOf");
                assert_eq!(descriptor, "(I)Ljava/lang/String;");
            }
            _ => panic!("Expected InvokeStatic variant"),
        }
    }

    #[test]
    fn test_method_handle_invoke_virtual() {
        let handle =
            MethodHandle::invoke_virtual("java/lang/Object", "toString", "()Ljava/lang/String;");
        match handle {
            MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            } => {
                assert_eq!(class_name, "java/lang/Object");
                assert_eq!(method_name, "toString");
                assert_eq!(descriptor, "()Ljava/lang/String;");
            }
            _ => panic!("Expected InvokeVirtual variant"),
        }
    }

    #[test]
    fn test_call_site_static_method() {
        let call_site = CallSite::static_method(
            "concat",
            "java/lang/String",
            "concat",
            "(Ljava/lang/String;)Ljava/lang/String;",
            "(Ljava/lang/String;)Ljava/lang/String;",
        );

        assert_eq!(&*call_site.name, "concat");
        assert_eq!(
            call_site.method_type,
            "(Ljava/lang/String;)Ljava/lang/String;"
        );

        match &call_site.target {
            MethodHandle::InvokeStatic {
                class_name,
                method_name,
                descriptor,
            } => {
                assert_eq!(class_name, "java/lang/String");
                assert_eq!(method_name, "concat");
                assert_eq!(descriptor, "(Ljava/lang/String;)Ljava/lang/String;");
            }
            _ => panic!("Expected InvokeStatic variant"),
        }
    }

    #[test]
    fn test_call_site_virtual_method() {
        let call_site = CallSite::virtual_method(
            "toString",
            "java/lang/Object",
            "toString",
            "()Ljava/lang/String;",
            "()Ljava/lang/String;",
        );

        assert_eq!(&*call_site.name, "toString");
        assert_eq!(call_site.method_type, "()Ljava/lang/String;");

        match &call_site.target {
            MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            } => {
                assert_eq!(class_name, "java/lang/Object");
                assert_eq!(method_name, "toString");
                assert_eq!(descriptor, "()Ljava/lang/String;");
            }
            _ => panic!("Expected InvokeVirtual variant"),
        }
    }

    #[test]
    fn test_method_handle_debug_format() {
        let static_handle = MethodHandle::invoke_static("java/lang/Math", "max", "(II)I");
        let debug_str = format!("{:?}", static_handle);
        assert!(debug_str.contains("InvokeStatic"));
        assert!(debug_str.contains("java/lang/Math"));
        assert!(debug_str.contains("max"));

        let virtual_handle =
            MethodHandle::invoke_virtual("java/util/ArrayList", "add", "(Ljava/lang/Object;)Z");
        let debug_str = format!("{:?}", virtual_handle);
        assert!(debug_str.contains("InvokeVirtual"));
        assert!(debug_str.contains("java/util/ArrayList"));
        assert!(debug_str.contains("add"));
    }

    // ============================================================
    // Regression tests: CallSite mutability and caching
    // ============================================================
    // Per JVM spec §5.4.3.6 and java.lang.invoke package:
    // - ConstantCallSite: Immutable, safe to cache
    // - MutableCallSite: Can change via setTarget(), unsafe to cache
    // - VolatileCallSite: Like MutableCallSite but with volatile semantics, unsafe to cache
    // These tests verify that only Constant call sites are cacheable.

    #[test]
    fn test_constant_call_site_is_cacheable() {
        let call_site =
            CallSite::static_method("test", "com/example/Test", "testMethod", "()V", "()V");

        assert_eq!(call_site.kind, CallSiteKind::Constant);
        assert!(
            call_site.is_cacheable(),
            "ConstantCallSite should be cacheable"
        );
    }

    #[test]
    fn test_mutable_call_site_not_cacheable() {
        let mut call_site =
            CallSite::static_method("test", "com/example/Test", "testMethod", "()V", "()V");

        // Simulate bootstrap method returning MutableCallSite
        call_site.kind = CallSiteKind::Mutable;

        assert_eq!(call_site.kind, CallSiteKind::Mutable);
        assert!(
            !call_site.is_cacheable(),
            "MutableCallSite must not be cacheable"
        );
    }

    #[test]
    fn test_volatile_call_site_not_cacheable() {
        let mut call_site =
            CallSite::static_method("test", "com/example/Test", "testMethod", "()V", "()V");

        // Simulate bootstrap method returning VolatileCallSite
        call_site.kind = CallSiteKind::Volatile;

        assert_eq!(call_site.kind, CallSiteKind::Volatile);
        assert!(
            !call_site.is_cacheable(),
            "VolatileCallSite must not be cacheable"
        );
    }

    #[test]
    fn test_call_site_kind_equality() {
        assert_eq!(CallSiteKind::Constant, CallSiteKind::Constant);
        assert_eq!(CallSiteKind::Mutable, CallSiteKind::Mutable);
        assert_eq!(CallSiteKind::Volatile, CallSiteKind::Volatile);

        assert_ne!(CallSiteKind::Constant, CallSiteKind::Mutable);
        assert_ne!(CallSiteKind::Constant, CallSiteKind::Volatile);
        assert_ne!(CallSiteKind::Mutable, CallSiteKind::Volatile);
    }

    #[test]
    fn test_default_call_sites_are_constant() {
        // Verify that the helper methods default to Constant (safe default)
        let static_cs = CallSite::static_method("test", "Test", "test", "()V", "()V");
        assert_eq!(
            static_cs.kind,
            CallSiteKind::Constant,
            "static_method should default to Constant"
        );

        let virtual_cs = CallSite::virtual_method("test", "Test", "test", "()V", "()V");
        assert_eq!(
            virtual_cs.kind,
            CallSiteKind::Constant,
            "virtual_method should default to Constant"
        );
    }

    #[test]
    fn test_callsite_validate_valid() {
        let cs = CallSite::static_method("apply", "com/example/Test", "myMethod", "()V", "()V");
        assert!(
            cs.validate().is_ok(),
            "Valid CallSite should pass validation"
        );
    }

    #[test]
    fn test_callsite_validate_empty_name() {
        let cs = CallSite {
            target: MethodHandle::invoke_static("com/example/Test", "test", "()V"),
            name: String::new(), // Empty name!
            method_type: "()V".to_string(),
            kind: CallSiteKind::Constant,
        };
        assert!(
            cs.validate().is_err(),
            "CallSite with empty name should fail validation"
        );
    }

    #[test]
    fn test_callsite_validate_empty_class() {
        let cs = CallSite {
            target: MethodHandle::invoke_static("", "test", "()V"), // Empty class!
            name: "test".to_string(),
            method_type: "()V".to_string(),
            kind: CallSiteKind::Constant,
        };
        assert!(
            cs.validate().is_err(),
            "CallSite with empty class should fail validation"
        );
    }

    #[test]
    fn test_callsite_validate_empty_method() {
        let cs = CallSite {
            target: MethodHandle::invoke_virtual("com/example/Test", "", "()V"), // Empty method!
            name: "test".to_string(),
            method_type: "()V".to_string(),
            kind: CallSiteKind::Constant,
        };
        assert!(
            cs.validate().is_err(),
            "CallSite with empty method should fail validation"
        );
    }
}
