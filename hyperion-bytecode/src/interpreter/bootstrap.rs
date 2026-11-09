// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! Bootstrap Method Handling for invokedynamic
//!
//! This module implements bootstrap method resolution for invokedynamic instructions.
//! Bootstrap methods are statically invoked at link time to produce CallSite objects.
//!
//! JVMS §6.5.invokedynamic specifies:
//! 1. Resolve the bootstrap method from constant pool
//! 2. Collect bootstrap arguments from constant pool
//! 3. Invoke: bootstrap(Lookup, String name, MethodType type, Object... args)
//! 4. Bootstrap returns a CallSite object
//! 5. Cache CallSite for future invocations of same invokedynamic
//!
//! NOTE: We store resolved bootstrap data directly from cafebabe parsing,
//! avoiding redundant CP index resolution at execution time.

use super::call_site::{CallSite, CallSiteKind, MethodHandle};
use super::errors::{VMError, VMResult};
use super::method_type::MethodTypeRef;

// ============================================================================
// Resolved Bootstrap Types (populated at class load time from cafebabe)
// ============================================================================

/// Method handle kind (matches JVMS reference_kind values 1-9)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodHandleKind {
    GetField,
    GetStatic,
    PutField,
    PutStatic,
    InvokeVirtual,
    InvokeStatic,
    InvokeSpecial,
    NewInvokeSpecial,
    InvokeInterface,
}

/// A resolved method handle (class, method, descriptor already extracted)
#[derive(Debug, Clone)]
pub struct ResolvedMethodHandle {
    pub kind: MethodHandleKind,
    pub class_name: String,
    pub member_name: String,
    pub descriptor: String,
}

/// Resolved bootstrap argument (already resolved from constant pool)
#[derive(Debug, Clone)]
pub enum BootstrapArg {
    String(String),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Class(String),
    MethodHandle(ResolvedMethodHandle),
    MethodType(String), // descriptor
}

/// Resolved bootstrap method (populated at class load time)
#[derive(Debug, Clone)]
pub struct ResolvedBootstrapMethod {
    /// The bootstrap method handle (already resolved)
    pub method: ResolvedMethodHandle,
    /// Static arguments (already resolved)
    pub arguments: Vec<BootstrapArg>,
}

// ============================================================================
// Bootstrap Context
// ============================================================================

/// Bootstrap context for invokedynamic call site resolution
/// Note: Caching is handled at the executor level (by CP index), not here.
pub struct BootstrapContext;

impl BootstrapContext {
    /// Create a new bootstrap context
    pub fn new() -> Self {
        BootstrapContext
    }

    /// Execute a bootstrap method to get a CallSite
    ///
    /// Takes already-resolved bootstrap data (from class loading) and produces a CallSite.
    /// No CP resolution needed — cafebabe already did that work.
    pub fn invoke_bootstrap(
        &self,
        bootstrap: &ResolvedBootstrapMethod,
        call_site_name: &str,
        call_site_type: &MethodTypeRef,
    ) -> VMResult<CallSite> {
        let class = &bootstrap.method.class_name;
        let method = &bootstrap.method.member_name;

        // Pattern-match on known bootstrap methods
        match class.as_str() {
            _ if class.contains("LambdaMetafactory") => {
                self.lambda_metafactory(call_site_name, call_site_type, &bootstrap.arguments)
            }
            _ if class.contains("StringConcatFactory") => {
                self.string_concat_factory(call_site_name, call_site_type, &bootstrap.arguments)
            }
            _ if class.contains("groovy") || class.contains("Groovy") => {
                self.groovy_bootstrap(method, call_site_name, call_site_type, &bootstrap.arguments)
            }
            _ => Err(VMError::BootstrapMethodError(format!(
                "Unknown bootstrap method: {}.{}{}",
                class, method, bootstrap.method.descriptor
            ))),
        }
    }

    /// LambdaMetafactory bootstrap
    /// Implements java.lang.invoke.LambdaMetafactory.metafactory()
    /// Creates a CallSite for lambda expression invocation
    ///
    /// Static args from the bootstrap method:
    /// - Arg 0: SAM method type (MethodType)
    /// - Arg 1: Implementation method (MethodHandle) - the actual method to invoke
    /// - Arg 2: Instantiated method type (MethodType)
    fn lambda_metafactory(
        &self,
        method_name: &str,
        method_type: &MethodTypeRef,
        static_args: &[BootstrapArg],
    ) -> VMResult<CallSite> {
        // Extract implementation MethodHandle from static args
        // Arg 1 must be a MethodHandle per JVM spec - this contains the ACTUAL
        // implementation method (e.g., "lambda$main$0"), NOT the SAM method ("apply")
        let impl_handle = if static_args.len() > 1 {
            match &static_args[1] {
                BootstrapArg::MethodHandle(mh) => mh,
                _ => {
                    return Err(VMError::internal(
                        "LambdaMetafactory arg 1 must be MethodHandle",
                    ))
                }
            }
        } else {
            return Err(VMError::internal(
                "LambdaMetafactory requires implementation MethodHandle",
            ));
        };

        // The SAM method type (for the CallSite's declared type)
        let sam_descriptor = Self::method_type_to_descriptor(method_type);

        // Create CallSite targeting the IMPLEMENTATION method, not the SAM method
        // Example: For `Function<String, Integer> f = s -> s.length();`
        // - method_name = "apply" (SAM method - used for CallSite name)
        // - impl_handle.member_name = "lambda$main$0" (actual implementation)
        // - impl_handle.descriptor = "(Ljava/lang/String;)Ljava/lang/Integer;"
        let target = match impl_handle.kind {
            MethodHandleKind::InvokeStatic => MethodHandle::invoke_static(
                &impl_handle.class_name,
                &impl_handle.member_name,
                &impl_handle.descriptor,
            ),
            MethodHandleKind::InvokeVirtual => MethodHandle::invoke_virtual(
                &impl_handle.class_name,
                &impl_handle.member_name,
                &impl_handle.descriptor,
            ),
            MethodHandleKind::InvokeSpecial => MethodHandle::invoke_virtual(
                &impl_handle.class_name,
                &impl_handle.member_name,
                &impl_handle.descriptor,
            ),
            MethodHandleKind::InvokeInterface => MethodHandle::invoke_interface(
                &impl_handle.class_name,
                &impl_handle.member_name,
                &impl_handle.descriptor,
            ),
            MethodHandleKind::NewInvokeSpecial => {
                MethodHandle::new_invoke_special(&impl_handle.class_name, &impl_handle.descriptor)
            }
            _ => {
                return Err(VMError::internal(format!(
                    "Unsupported MethodHandle kind for lambda: {:?}",
                    impl_handle.kind
                )))
            }
        };

        Ok(CallSite {
            target,
            name: method_name.to_string(), // SAM method name for interface
            method_type: sam_descriptor,   // SAM type for interface compatibility
            kind: CallSiteKind::Constant,
        })
    }

    /// StringConcatFactory bootstrap
    /// Implements java.lang.invoke.StringConcatFactory.makeConcatWithConstants()
    /// Creates a CallSite for efficient string concatenation (Java 9+)
    ///
    /// Static args from the bootstrap method:
    /// - Arg 0: Recipe string (the concatenation pattern)
    /// - Args 1+: Constant values to be concatenated
    fn string_concat_factory(
        &self,
        method_name: &str,
        method_type: &MethodTypeRef,
        _static_args: &[BootstrapArg],
    ) -> VMResult<CallSite> {
        let descriptor = Self::method_type_to_descriptor(method_type);

        Ok(CallSite {
            target: MethodHandle::invoke_static(
                "java.lang.invoke.StringConcatFactory",
                method_name,
                &descriptor,
            ),
            name: method_name.to_string(),
            method_type: descriptor,
            kind: CallSiteKind::Constant,
        })
    }

    /// Groovy bootstrap methods (selectMethod, getProperty, setProperty)
    /// Implements Groovy's dynamic method dispatch and property access
    ///
    /// CRITICAL: Groovy bootstrap methods return MutableCallSite, not ConstantCallSite.
    /// Per org.codehaus.groovy.vmplugin.v8.IndyInterface implementation, Groovy's
    /// MetaClass can change at runtime (via ExpandoMetaClass, categories, mixins),
    /// so call sites must be mutable and MUST NOT be cached.
    fn groovy_bootstrap(
        &self,
        bootstrap_method_name: &str,
        call_site_name: &str,
        method_type: &MethodTypeRef,
        _static_args: &[BootstrapArg],
    ) -> VMResult<CallSite> {
        match bootstrap_method_name {
            "selectMethod" => {
                let descriptor = Self::method_type_to_descriptor(method_type);
                Ok(CallSite {
                    target: MethodHandle::invoke_virtual(
                        "org.codehaus.groovy.runtime.InvokerHelper",
                        call_site_name,
                        &descriptor,
                    ),
                    name: call_site_name.to_string(),
                    method_type: descriptor,
                    // BUG FIX: Groovy uses MutableCallSite for dynamic dispatch
                    // because MetaClass can change via ExpandoMetaClass/categories
                    kind: CallSiteKind::Mutable,
                })
            }
            "getProperty" => {
                let getter_name = if !call_site_name.is_empty() {
                    format!(
                        "get{}{}",
                        call_site_name[0..1].to_uppercase(),
                        &call_site_name[1..]
                    )
                } else {
                    "get".to_string()
                };

                Ok(CallSite {
                    target: MethodHandle::invoke_virtual(
                        "java.lang.Object",
                        &getter_name,
                        "()Ljava/lang/Object;",
                    ),
                    name: call_site_name.to_string(),
                    method_type: "()Ljava/lang/Object;".to_string(),
                    // BUG FIX: Groovy property access is dynamic (MetaClass mutable)
                    kind: CallSiteKind::Mutable,
                })
            }
            "setProperty" => {
                let setter_name = if !call_site_name.is_empty() {
                    format!(
                        "set{}{}",
                        call_site_name[0..1].to_uppercase(),
                        &call_site_name[1..]
                    )
                } else {
                    "set".to_string()
                };

                Ok(CallSite {
                    target: MethodHandle::invoke_virtual(
                        "java.lang.Object",
                        &setter_name,
                        "(Ljava/lang/Object;)V",
                    ),
                    name: call_site_name.to_string(),
                    method_type: "(Ljava/lang/Object;)V".to_string(),
                    // BUG FIX: Groovy property setters are dynamic (MetaClass mutable)
                    kind: CallSiteKind::Mutable,
                })
            }
            // Groovy's IndyInterface.bootstrap - the generic bootstrap method
            // This delegates to selectMethod for all method calls including closure.call()
            "bootstrap" => {
                let call_site_descriptor = Self::method_type_to_descriptor(method_type);
                let Some(receiver) = method_type.parameter_types.first() else {
                    return Err(VMError::BootstrapMethodError(
                        "Groovy invokedynamic call site is missing its receiver parameter"
                            .to_string(),
                    ));
                };
                if !receiver.is_reference() {
                    return Err(VMError::BootstrapMethodError(
                        "Groovy invokedynamic receiver must be a reference type".to_string(),
                    ));
                }
                let virtual_method_type = MethodTypeRef {
                    return_type: method_type.return_type.clone(),
                    parameter_types: method_type.parameter_types[1..].to_vec(),
                };
                let virtual_descriptor = Self::method_type_to_descriptor(&virtual_method_type);

                // For closure.call(), redirect to doCall. The call-site type
                // includes the receiver; the virtual member descriptor does not.
                let actual_method = if call_site_name == "call" {
                    "doCall"
                } else {
                    call_site_name
                };

                Ok(CallSite {
                    target: MethodHandle::invoke_virtual(
                        "org.codehaus.groovy.runtime.InvokerHelper",
                        actual_method,
                        &virtual_descriptor,
                    ),
                    name: call_site_name.to_string(),
                    method_type: call_site_descriptor,
                    kind: CallSiteKind::Mutable,
                })
            }
            other => Err(VMError::BootstrapMethodError(format!(
                "Unknown Groovy bootstrap method: {}",
                other
            ))),
        }
    }

    /// Convert MethodTypeRef to JVM method descriptor string
    fn method_type_to_descriptor(method_type: &MethodTypeRef) -> String {
        let params = method_type
            .parameter_types
            .iter()
            .map(|t| format!("{}", t))
            .collect::<String>();
        format!("({}){}", params, method_type.return_type)
    }
}

impl Default for BootstrapContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_method_type(ret: &str, params: &[&str]) -> MethodTypeRef {
        use super::super::method_type::TypeDescriptor;

        let parse_type = |s: &str| -> TypeDescriptor {
            match s {
                "V" => TypeDescriptor::Void,
                "I" => TypeDescriptor::Int,
                "J" => TypeDescriptor::Long,
                "F" => TypeDescriptor::Float,
                "D" => TypeDescriptor::Double,
                "Z" => TypeDescriptor::Boolean,
                "B" => TypeDescriptor::Byte,
                "C" => TypeDescriptor::Char,
                "S" => TypeDescriptor::Short,
                s if s.starts_with("L") && s.ends_with(";") => {
                    TypeDescriptor::Reference(s[1..s.len() - 1].to_string())
                }
                s => TypeDescriptor::Reference(s.to_string()),
            }
        };

        MethodTypeRef {
            return_type: parse_type(ret),
            parameter_types: params.iter().map(|s| parse_type(s)).collect(),
        }
    }

    #[test]
    fn test_bootstrap_context_creation() {
        let _ctx = BootstrapContext::new();
    }

    #[test]
    fn test_lambda_metafactory() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/Object;", &[]);

        // lambda_metafactory requires a MethodHandle as the implementation
        let impl_handle = ResolvedMethodHandle {
            kind: MethodHandleKind::InvokeVirtual,
            class_name: "java/lang/Object".to_string(),
            member_name: "apply".to_string(),
            descriptor: "()Ljava/lang/Object;".to_string(),
        };

        let static_args = vec![
            BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
            BootstrapArg::MethodHandle(impl_handle),
            BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
        ];

        let result = ctx.lambda_metafactory("apply", &method_type, &static_args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_lambda_metafactory_with_impl_method() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/Object;", &[]);

        // Simulate javac-generated lambda: Function<String, Integer> f = s -> s.length();
        // SAM method is "apply" but implementation is "lambda$main$0"
        let impl_handle = ResolvedMethodHandle {
            kind: MethodHandleKind::InvokeVirtual,
            class_name: "com/example/MyClass".to_string(),
            member_name: "lambda$main$0".to_string(), // Implementation method, NOT "apply"
            descriptor: "(Ljava/lang/String;)Ljava/lang/Integer;".to_string(),
        };

        let static_args = vec![
            BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
            BootstrapArg::MethodHandle(impl_handle),
            BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
        ];

        // SAM method name is "apply", but CallSite should target the implementation
        let result = ctx.lambda_metafactory("apply", &method_type, &static_args);
        assert!(result.is_ok());
        let call_site = result.unwrap();

        // CRITICAL: Verify we use the IMPLEMENTATION method, not the SAM method
        match &call_site.target {
            MethodHandle::InvokeVirtual {
                class_name,
                method_name,
                descriptor,
            } => {
                assert_eq!(class_name, "com/example/MyClass");
                // Must be the impl method name, NOT "apply"
                assert_eq!(
                    method_name, "lambda$main$0",
                    "Should use implementation method name, not SAM method 'apply'"
                );
                // Must be the impl descriptor, NOT the SAM type
                assert_eq!(
                    descriptor, "(Ljava/lang/String;)Ljava/lang/Integer;",
                    "Should use implementation descriptor, not SAM type"
                );
            }
            _ => panic!("Expected InvokeVirtual"),
        }

        // CallSite name should still be the SAM method name (for interface)
        assert_eq!(&*call_site.name, "apply");
    }

    #[test]
    fn test_string_concat_factory() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/String;", &["Ljava/lang/String;"]);
        let result = ctx.string_concat_factory("makeConcatWithConstants", &method_type, &[]);
        assert!(result.is_ok());
        let call_site = result.unwrap();
        assert_eq!(&*call_site.name, "makeConcatWithConstants");
    }

    #[test]
    fn test_groovy_select_method() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("V", &[]);
        let result = ctx.groovy_bootstrap("selectMethod", "foo", &method_type, &[]);
        assert!(result.is_ok());
        let call_site = result.unwrap();
        assert_eq!(&*call_site.name, "foo");
        // CRITICAL: Groovy call sites must be Mutable (not cacheable)
        assert_eq!(
            call_site.kind,
            CallSiteKind::Mutable,
            "Groovy selectMethod must return MutableCallSite"
        );
        assert!(
            !call_site.is_cacheable(),
            "Groovy call sites must not be cached"
        );
    }

    #[test]
    fn generic_groovy_bootstrap_separates_call_site_receiver_from_virtual_descriptor() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type(
            "Ljava/lang/Object;",
            &["Ljava/lang/Object;", "Ljava/lang/Object;"],
        );
        let call_site = ctx
            .groovy_bootstrap("bootstrap", "call", &method_type, &[])
            .expect("receiver-aware Groovy call site");

        assert_eq!(
            call_site.method_type,
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;"
        );
        match call_site.target {
            MethodHandle::InvokeVirtual {
                method_name,
                descriptor,
                ..
            } => {
                assert_eq!(method_name, "doCall");
                assert_eq!(descriptor, "(Ljava/lang/Object;)Ljava/lang/Object;");
            }
            _ => panic!("expected virtual Groovy target"),
        }

        let missing_receiver = make_method_type("Ljava/lang/Object;", &[]);
        let error = match ctx.groovy_bootstrap("bootstrap", "call", &missing_receiver, &[]) {
            Ok(_) => panic!("receiver-free Groovy call site must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("missing its receiver parameter"));
    }

    #[test]
    fn test_groovy_get_property() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/Object;", &[]);
        let result = ctx.groovy_bootstrap("getProperty", "name", &method_type, &[]);
        assert!(result.is_ok());
        let call_site = result.unwrap();
        assert_eq!(&*call_site.name, "name");
        // Should generate getter method
        match &call_site.target {
            MethodHandle::InvokeVirtual { method_name, .. } => {
                assert_eq!(method_name, "getName");
            }
            _ => panic!("Expected InvokeVirtual"),
        }
        // CRITICAL: Groovy property access must be Mutable
        assert_eq!(call_site.kind, CallSiteKind::Mutable);
        assert!(!call_site.is_cacheable());
    }

    #[test]
    fn test_groovy_set_property() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("V", &["Ljava/lang/Object;"]);
        let result = ctx.groovy_bootstrap("setProperty", "value", &method_type, &[]);
        assert!(result.is_ok());
        let call_site = result.unwrap();
        assert_eq!(&*call_site.name, "value");
        // Should generate setter method
        match &call_site.target {
            MethodHandle::InvokeVirtual { method_name, .. } => {
                assert_eq!(method_name, "setValue");
            }
            _ => panic!("Expected InvokeVirtual"),
        }
        // CRITICAL: Groovy property setters must be Mutable
        assert_eq!(call_site.kind, CallSiteKind::Mutable);
        assert!(!call_site.is_cacheable());
    }

    #[test]
    fn test_unknown_bootstrap_method() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("V", &[]);
        let bootstrap = ResolvedBootstrapMethod {
            method: ResolvedMethodHandle {
                kind: MethodHandleKind::InvokeStatic,
                class_name: "com/unknown/Factory".to_string(),
                member_name: "create".to_string(),
                descriptor: "()V".to_string(),
            },
            arguments: vec![],
        };
        let result = ctx.invoke_bootstrap(&bootstrap, "test", &method_type);
        assert!(result.is_err());
    }

    #[test]
    fn test_invoke_bootstrap_lambda() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/Object;", &[]);

        // LambdaMetafactory requires proper static args:
        // - Arg 0: SAM method type
        // - Arg 1: Implementation method handle
        // - Arg 2: Instantiated method type
        let impl_handle = ResolvedMethodHandle {
            kind: MethodHandleKind::InvokeVirtual,
            class_name: "java/lang/Object".to_string(),
            member_name: "apply".to_string(),
            descriptor: "()Ljava/lang/Object;".to_string(),
        };

        let bootstrap = ResolvedBootstrapMethod {
            method: ResolvedMethodHandle {
                kind: MethodHandleKind::InvokeStatic,
                class_name: "java/lang/invoke/LambdaMetafactory".to_string(),
                member_name: "metafactory".to_string(),
                descriptor: "()V".to_string(),
            },
            arguments: vec![
                BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
                BootstrapArg::MethodHandle(impl_handle),
                BootstrapArg::MethodType("()Ljava/lang/Object;".to_string()),
            ],
        };

        let result = ctx.invoke_bootstrap(&bootstrap, "apply", &method_type);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invoke_bootstrap_string_concat() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/String;", &["I", "Ljava/lang/String;"]);

        let bootstrap = ResolvedBootstrapMethod {
            method: ResolvedMethodHandle {
                kind: MethodHandleKind::InvokeStatic,
                class_name: "java/lang/invoke/StringConcatFactory".to_string(),
                member_name: "makeConcatWithConstants".to_string(),
                descriptor: "()V".to_string(),
            },
            arguments: vec![BootstrapArg::String("\\1\\2".to_string())],
        };

        let result = ctx.invoke_bootstrap(&bootstrap, "concat", &method_type);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invoke_bootstrap_groovy() {
        let ctx = BootstrapContext::new();
        let method_type = make_method_type("Ljava/lang/Object;", &[]);

        let bootstrap = ResolvedBootstrapMethod {
            method: ResolvedMethodHandle {
                kind: MethodHandleKind::InvokeStatic,
                class_name: "org/codehaus/groovy/vmplugin/v8/IndyInterface".to_string(),
                member_name: "selectMethod".to_string(),
                descriptor: "()V".to_string(),
            },
            arguments: vec![],
        };

        let result = ctx.invoke_bootstrap(&bootstrap, "myMethod", &method_type);
        assert!(result.is_ok());
        let call_site = result.unwrap();
        // CRITICAL: Groovy bootstrap must return MutableCallSite
        assert_eq!(call_site.kind, CallSiteKind::Mutable);
        assert!(!call_site.is_cacheable());
    }

    #[test]
    fn test_method_type_to_descriptor() {
        let method_type = make_method_type("I", &["Ljava/lang/String;", "I"]);
        let descriptor = BootstrapContext::method_type_to_descriptor(&method_type);
        assert_eq!(descriptor, "(Ljava/lang/String;I)I");
    }

    #[test]
    fn test_method_type_to_descriptor_void() {
        let method_type = make_method_type("V", &[]);
        let descriptor = BootstrapContext::method_type_to_descriptor(&method_type);
        assert_eq!(descriptor, "()V");
    }
}
