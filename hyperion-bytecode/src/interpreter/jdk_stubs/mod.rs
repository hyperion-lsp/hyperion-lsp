// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JDK Class Stub Implementations
//!
//! This module provides Rust implementations of essential JDK classes
//! that are required for bytecode execution without a full JVM runtime.
//!
//! # Architecture
//!
//! JDK stubs intercept class resolution and provide native implementations
//! for common JDK classes, enabling Groovy/Gradle bytecode execution.
//!
//! ```text
//! Class Resolution Request
//!         ↓
//! Check if JDK stub exists
//!         ↓
//! If yes: Return stub class → Execute via natives
//! If no: Continue normal resolution
//! ```
//!
//! # Supported Packages
//!
//! - `java.lang` - Object, String, StringBuilder, primitive wrappers
//! - `java.util` - Collections (ArrayList, HashMap, etc.)
//! - `java.io` - File, streams (basic support)
//! - `groovy.lang` - IntRange, GString, Closure support

pub mod groovy_lang;
pub mod java_concurrent;
pub mod java_function;
pub mod java_io;
pub mod java_lang;
pub mod java_math;
pub mod java_nio;
pub mod java_regex;
pub mod java_stream;
pub mod java_util;

use super::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

/// Represents a stubbed JDK class with its methods
#[derive(Debug, Clone)]
pub struct StubClass {
    /// Fully qualified class name (e.g., "java/lang/String")
    pub name: String,
    /// Superclass name
    pub superclass: Option<String>,
    /// Implemented interfaces
    pub interfaces: Vec<String>,
    /// Methods available on this class
    pub methods: HashMap<String, StubMethodDescriptor>,
}

/// Descriptor for a stub method
#[derive(Debug, Clone)]
pub struct StubMethodDescriptor {
    /// Method name
    pub name: String,
    /// JVM descriptor (e.g., "(I)Ljava/lang/String;")
    pub descriptor: String,
    /// Whether this is a static method
    pub is_static: bool,
    /// Whether this is a constructor
    pub is_constructor: bool,
}

impl StubMethodDescriptor {
    pub fn new(name: &str, descriptor: &str, is_static: bool) -> Self {
        Self {
            name: name.to_string(),
            descriptor: descriptor.to_string(),
            is_static,
            is_constructor: name == "<init>",
        }
    }

    pub fn constructor(descriptor: &str) -> Self {
        Self {
            name: "<init>".to_string(),
            descriptor: descriptor.to_string(),
            is_static: false,
            is_constructor: true,
        }
    }

    pub fn static_method(name: &str, descriptor: &str) -> Self {
        Self::new(name, descriptor, true)
    }

    pub fn instance_method(name: &str, descriptor: &str) -> Self {
        Self::new(name, descriptor, false)
    }
}

/// The stub classes, and which of them are interfaces. Kept in one shared allocation so that a
/// registry clone copies one pointer.
#[derive(Clone, Default)]
struct StubCatalog {
    classes: HashMap<String, StubClass>,
    interfaces: HashSet<String>,
}

/// Registry of all JDK stub classes
#[derive(Clone)]
pub struct JdkStubRegistry {
    catalog: Arc<StubCatalog>,
}

impl JdkStubRegistry {
    /// Create a new JDK stub registry with all supported classes
    pub fn new() -> Self {
        static BUILTIN_CLASSES: OnceLock<Arc<StubCatalog>> = OnceLock::new();

        let catalog = BUILTIN_CLASSES.get_or_init(|| {
            let mut registry = Self {
                catalog: Arc::new(StubCatalog::default()),
            };

            // Register all JDK stub classes once for the process. Registry
            // clones normally share this immutable map; the public extension
            // API below preserves custom registries through copy-on-write.
            registry.register_java_lang_classes();
            registry.register_java_util_classes();
            registry.register_java_util_function_classes();
            registry.register_java_util_concurrent_classes();
            registry.register_java_util_stream_classes();
            registry.register_java_util_regex_classes();
            registry.register_java_io_classes();
            registry.register_java_nio_classes();
            registry.register_java_math_classes();
            registry.register_awt_classes();
            registry.register_linkage_only_native_classes();
            registry.register_generated_runtime_classes();
            registry.register_groovy_lang_classes();
            registry.register_gradle_classes();

            registry.catalog
        });

        Self {
            catalog: Arc::clone(catalog),
        }
    }

    /// Authoritative declarations for private compiler/runtime protocols.
    ///
    /// The Groovy lowering emits ordinary `invokestatic` references to these
    /// classes. They therefore need real symbolic metadata even though the VM
    /// executes their bodies through native handlers.
    fn register_generated_runtime_classes(&mut self) {
        use crate::groovy_runtime::{
            BINARY_NUMBER_DESCRIPTOR, COMPARE_NUMBER_DESCRIPTOR, NUMBERS_CLASS, TRUTH_CLASS,
            TRUTH_DESCRIPTOR, TRUTH_METHOD, UNARY_NUMBER_DESCRIPTOR,
        };

        let mut number_methods = HashMap::new();
        for name in ["plus", "minus", "multiply", "divide", "remainder"] {
            number_methods.insert(
                format!("{name}:{BINARY_NUMBER_DESCRIPTOR}"),
                StubMethodDescriptor::static_method(name, BINARY_NUMBER_DESCRIPTOR),
            );
        }
        for (name, descriptor) in [
            ("compare", COMPARE_NUMBER_DESCRIPTOR),
            ("negate", UNARY_NUMBER_DESCRIPTOR),
        ] {
            number_methods.insert(
                format!("{name}:{descriptor}"),
                StubMethodDescriptor::static_method(name, descriptor),
            );
        }
        self.register_class(StubClass {
            name: NUMBERS_CLASS.to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: number_methods,
        });

        self.register_class(StubClass {
            name: TRUTH_CLASS.to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::from([(
                format!("{TRUTH_METHOD}:{TRUTH_DESCRIPTOR}"),
                StubMethodDescriptor::static_method(TRUTH_METHOD, TRUTH_DESCRIPTOR),
            )]),
        });
    }

    fn register_linkage_only_native_classes(&mut self) {
        self.register_structural_linkage_types();

        let mut entry_methods = HashMap::new();
        for method in ["getKey", "getValue"] {
            entry_methods.insert(
                format!("{method}:()Ljava/lang/Object;"),
                StubMethodDescriptor::instance_method(method, "()Ljava/lang/Object;"),
            );
        }
        entry_methods.insert(
            "setValue:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
            StubMethodDescriptor::instance_method(
                "setValue",
                "(Ljava/lang/Object;)Ljava/lang/Object;",
            ),
        );
        self.register_class(StubClass {
            name: "java/util/Map$Entry".to_string(),
            superclass: None,
            interfaces: vec![],
            methods: entry_methods,
        });

        let mut optional_double_methods = HashMap::new();
        optional_double_methods.insert(
            "isPresent:()Z".to_string(),
            StubMethodDescriptor::instance_method("isPresent", "()Z"),
        );
        optional_double_methods.insert(
            "getAsDouble:()D".to_string(),
            StubMethodDescriptor::instance_method("getAsDouble", "()D"),
        );
        optional_double_methods.insert(
            "orElse:(D)D".to_string(),
            StubMethodDescriptor::instance_method("orElse", "(D)D"),
        );
        self.register_class(StubClass {
            name: "java/util/OptionalDouble".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: optional_double_methods,
        });

        for (name, accessor, primitive) in [
            ("OptionalInt", "getAsInt", "I"),
            ("OptionalLong", "getAsLong", "J"),
        ] {
            let mut methods = HashMap::new();
            methods.insert(
                "isPresent:()Z".to_string(),
                StubMethodDescriptor::instance_method("isPresent", "()Z"),
            );
            methods.insert(
                format!("{accessor}:(){primitive}"),
                StubMethodDescriptor::instance_method(accessor, &format!("(){primitive}")),
            );
            methods.insert(
                format!("orElse:({primitive}){primitive}"),
                StubMethodDescriptor::instance_method(
                    "orElse",
                    &format!("({primitive}){primitive}"),
                ),
            );
            self.register_class(StubClass {
                name: format!("java/util/{name}"),
                superclass: Some("java/lang/Object".to_string()),
                interfaces: vec![],
                methods,
            });
        }
    }

    /// Structural JDK nodes required to link the native surface below.
    ///
    /// These classes do not need executable native methods themselves, but a
    /// child cannot be safely published with a missing or misclassified parent.
    /// Keep the real hierarchy here so subtype checks, inherited interface
    /// routes, and field/vtable prefixing remain correct.
    fn register_structural_linkage_types(&mut self) {
        for (name, interfaces) in [
            ("java/lang/Appendable", vec![]),
            ("java/lang/Readable", vec![]),
            ("java/lang/reflect/AnnotatedElement", vec![]),
            (
                "java/lang/reflect/GenericDeclaration",
                vec!["java/lang/reflect/AnnotatedElement"],
            ),
            ("java/lang/reflect/Member", vec![]),
            ("java/lang/reflect/Type", vec![]),
            ("java/util/RandomAccess", vec![]),
            ("java/util/SortedMap", vec!["java/util/Map"]),
            ("java/util/NavigableMap", vec!["java/util/SortedMap"]),
            ("java/util/SortedSet", vec!["java/util/Set"]),
            ("java/util/NavigableSet", vec!["java/util/SortedSet"]),
            ("java/util/concurrent/ConcurrentMap", vec!["java/util/Map"]),
            ("java/util/concurrent/CompletionStage", vec![]),
            (
                "java/nio/channels/InterruptibleChannel",
                vec!["java/nio/channels/Channel"],
            ),
            (
                "java/nio/channels/GatheringByteChannel",
                vec!["java/nio/channels/WritableByteChannel"],
            ),
            (
                "java/nio/channels/ScatteringByteChannel",
                vec!["java/nio/channels/ReadableByteChannel"],
            ),
        ] {
            self.register_class(StubClass {
                name: name.to_string(),
                superclass: None,
                interfaces: interfaces.into_iter().map(str::to_string).collect(),
                methods: HashMap::new(),
            });
        }

        for (name, superclass, interfaces) in [
            (
                "java/util/AbstractCollection",
                "java/lang/Object",
                vec!["java/util/Collection"],
            ),
            (
                "java/util/AbstractList",
                "java/util/AbstractCollection",
                vec!["java/util/List"],
            ),
            (
                "java/util/AbstractSequentialList",
                "java/util/AbstractList",
                vec![],
            ),
            (
                "java/util/AbstractMap",
                "java/lang/Object",
                vec!["java/util/Map"],
            ),
            (
                "java/util/AbstractSet",
                "java/util/AbstractCollection",
                vec!["java/util/Set"],
            ),
            (
                "java/lang/AbstractStringBuilder",
                "java/lang/Object",
                vec!["java/lang/Appendable", "java/lang/CharSequence"],
            ),
            (
                "java/lang/reflect/AccessibleObject",
                "java/lang/Object",
                vec!["java/lang/reflect/AnnotatedElement"],
            ),
            ("java/nio/Buffer", "java/lang/Object", vec![]),
            (
                "java/nio/channels/spi/AbstractInterruptibleChannel",
                "java/lang/Object",
                vec![
                    "java/nio/channels/Channel",
                    "java/nio/channels/InterruptibleChannel",
                ],
            ),
            (
                "java/security/SecureClassLoader",
                "java/lang/ClassLoader",
                vec![],
            ),
            (
                "java/net/URLClassLoader",
                "java/security/SecureClassLoader",
                vec![],
            ),
        ] {
            self.register_class(StubClass {
                name: name.to_string(),
                superclass: Some(superclass.to_string()),
                interfaces: interfaces.into_iter().map(str::to_string).collect(),
                methods: HashMap::new(),
            });
        }
    }

    fn register_awt_classes(&mut self) {
        let mut color_methods = HashMap::new();
        for descriptor in ["()V", "(I)V", "(III)V", "(IIII)V"] {
            color_methods.insert(
                format!("<init>:{descriptor}"),
                StubMethodDescriptor::constructor(descriptor),
            );
        }
        color_methods.insert(
            "getRGB:()I".to_string(),
            StubMethodDescriptor::instance_method("getRGB", "()I"),
        );
        self.register_class(StubClass {
            name: "java/awt/Color".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: color_methods,
        });

        let mut image_methods = HashMap::new();
        image_methods.insert(
            "<init>:(III)V".to_string(),
            StubMethodDescriptor::constructor("(III)V"),
        );
        for method in ["getWidth", "getHeight"] {
            image_methods.insert(
                format!("{method}:()I"),
                StubMethodDescriptor::instance_method(method, "()I"),
            );
        }
        image_methods.insert(
            "getRGB:(II)I".to_string(),
            StubMethodDescriptor::instance_method("getRGB", "(II)I"),
        );
        image_methods.insert(
            "setRGB:(III)V".to_string(),
            StubMethodDescriptor::instance_method("setRGB", "(III)V"),
        );
        image_methods.insert(
            "createGraphics:()Ljava/awt/Graphics2D;".to_string(),
            StubMethodDescriptor::instance_method("createGraphics", "()Ljava/awt/Graphics2D;"),
        );
        image_methods.insert(
            "getGraphics:()Ljava/awt/Graphics;".to_string(),
            StubMethodDescriptor::instance_method("getGraphics", "()Ljava/awt/Graphics;"),
        );
        self.register_class(StubClass {
            name: "java/awt/image/BufferedImage".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: image_methods,
        });

        // The Graphics2D an image hands out. The painting methods are the ones the
        // native dispatch implements, so a call to any other method fails loudly.
        let mut graphics_methods = HashMap::new();
        for (method, descriptor) in [
            ("setColor", "(Ljava/awt/Color;)V"),
            ("setPaint", "(Ljava/awt/Paint;)V"),
            ("setBackground", "(Ljava/awt/Color;)V"),
            ("fillRect", "(IIII)V"),
            ("drawRect", "(IIII)V"),
            ("drawLine", "(IIII)V"),
            ("translate", "(II)V"),
            ("setFont", "(Ljava/awt/Font;)V"),
            ("setStroke", "(Ljava/awt/Stroke;)V"),
            ("fill", "(Ljava/awt/Shape;)V"),
            ("draw", "(Ljava/awt/Shape;)V"),
            ("dispose", "()V"),
        ] {
            graphics_methods.insert(
                format!("{method}:{descriptor}"),
                StubMethodDescriptor::instance_method(method, descriptor),
            );
        }
        self.register_class(StubClass {
            name: "java/awt/Graphics2D".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: graphics_methods,
        });
    }

    /// Register Gradle DSL stub classes
    fn register_gradle_classes(&mut self) {
        // gradle/Project - main project API
        self.register_class(StubClass {
            name: "gradle/Project".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/DependencyHandler - for dependencies { } block
        self.register_class(StubClass {
            name: "gradle/DependencyHandler".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/RepositoryHandler - for repositories { } block
        self.register_class(StubClass {
            name: "gradle/RepositoryHandler".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/PluginDependenciesSpec - for plugins { } block
        self.register_class(StubClass {
            name: "gradle/PluginDependenciesSpec".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/ScriptHandler - for buildscript { } block
        self.register_class(StubClass {
            name: "gradle/ScriptHandler".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/MavenArtifactRepository
        self.register_class(StubClass {
            name: "gradle/MavenArtifactRepository".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/DependencyConstraintHandler
        self.register_class(StubClass {
            name: "gradle/DependencyConstraintHandler".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // gradle/DependencyConstraint
        self.register_class(StubClass {
            name: "gradle/DependencyConstraint".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // Minimal org.gradle API surface for executing compiled plugins offline.
        //
        // These are interface-only type stubs: method bodies are handled by
        // GradleNativeBridge and native intercepts. Preserving their interface
        // identity is required for linking precompiled plugin implementations.
        for (name, declarations) in [
            (
                "org/gradle/api/Project",
                vec![
                    ("getRootDir", "()Ljava/io/File;"),
                    ("getPlugins", "()Lorg/gradle/api/plugins/PluginContainer;"),
                    (
                        "getDependencies",
                        "()Lorg/gradle/api/artifacts/dsl/DependencyHandler;",
                    ),
                    ("subprojects", "(Lorg/gradle/api/Action;)V"),
                ],
            ),
            (
                "org/gradle/api/Plugin",
                vec![("apply", "(Ljava/lang/Object;)V")],
            ),
            (
                "org/gradle/api/Action",
                vec![("execute", "(Ljava/lang/Object;)V")],
            ),
            (
                "org/gradle/api/plugins/PluginContainer",
                vec![("apply", "(Ljava/lang/String;)V")],
            ),
            (
                "org/gradle/api/artifacts/dsl/DependencyHandler",
                vec![(
                    "add",
                    "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
                )],
            ),
        ] {
            let methods = declarations
                .into_iter()
                .map(|(method, descriptor)| {
                    (
                        format!("{method}:{descriptor}"),
                        StubMethodDescriptor::instance_method(method, descriptor),
                    )
                })
                .collect();
            self.register_class(StubClass {
                name: name.to_string(),
                superclass: None,
                interfaces: vec![],
                methods,
            });
        }
    }

    /// Check if a class is a JDK stub
    pub fn is_stub_class(&self, class_name: &str) -> bool {
        self.catalog.classes.contains_key(class_name)
    }

    /// Get a stub class definition
    pub fn get_class(&self, class_name: &str) -> Option<&StubClass> {
        self.catalog.classes.get(class_name)
    }

    /// Register a stub class
    pub fn register_class(&mut self, class: StubClass) {
        Arc::make_mut(&mut self.catalog)
            .classes
            .insert(class.name.clone(), class);
    }

    pub fn register_interface(&mut self, class: StubClass) {
        Arc::make_mut(&mut self.catalog)
            .interfaces
            .insert(class.name.clone());
        self.register_class(class);
    }

    pub fn is_interface(&self, class_name: &str) -> bool {
        self.catalog.interfaces.contains(class_name)
    }

    fn register_java_lang_classes(&mut self) {
        // java.lang.Object
        self.register_class(java_lang::create_object_class());

        // java.lang.Enum (structural base for javac-generated enum classes)
        self.register_class(java_lang::create_enum_class());

        // java.lang.Record (structural base for javac-generated records)
        self.register_class(java_lang::create_record_class());

        // java.lang.String
        self.register_class(java_lang::create_string_class());

        // java.lang.StringBuilder
        self.register_class(java_lang::create_string_builder_class());

        // java.lang.StringBuffer
        self.register_class(java_lang::create_string_buffer_class());

        // java.lang.Number
        self.register_class(java_lang::create_number_class());

        // Primitive wrappers
        self.register_class(java_lang::create_integer_class());
        self.register_class(java_lang::create_long_class());
        self.register_class(java_lang::create_double_class());
        self.register_class(java_lang::create_float_class());
        self.register_class(java_lang::create_boolean_class());
        self.register_class(java_lang::create_character_class());
        self.register_class(java_lang::create_byte_class());
        self.register_class(java_lang::create_short_class());

        // java.lang.Class
        self.register_class(java_lang::create_class_class());
        self.register_class(StubClass {
            name: "java/lang/invoke/MethodHandle".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        // java.lang.System
        self.register_class(java_lang::create_system_class());

        // java.lang.Math
        self.register_class(java_lang::create_math_class());

        // java.lang.Throwable and exceptions
        self.register_class(java_lang::create_throwable_class());
        self.register_class(java_lang::create_exception_class());
        self.register_class(java_lang::create_runtime_exception_class());
        self.register_core_exception_hierarchy();
        self.register_interface(java_lang::create_runnable_interface());
        self.register_class(StubClass {
            name: "java/lang/Cloneable".to_string(),
            superclass: None,
            interfaces: vec![],
            methods: HashMap::new(),
        });
        let mut illegal_argument_methods = HashMap::new();
        for descriptor in ["()V", "(Ljava/lang/String;)V"] {
            illegal_argument_methods.insert(
                format!("<init>:{descriptor}"),
                StubMethodDescriptor::constructor(descriptor),
            );
        }
        self.register_class(StubClass {
            name: "java/lang/IllegalArgumentException".to_string(),
            superclass: Some("java/lang/RuntimeException".to_string()),
            interfaces: vec![],
            methods: illegal_argument_methods,
        });

        // java.lang.Comparable
        self.register_interface(java_lang::create_comparable_interface());

        // java.lang.AutoCloseable
        self.register_interface(java_lang::create_auto_closeable_interface());

        // java.lang.CharSequence
        self.register_interface(java_lang::create_charsequence_interface());

        // java.lang.reflect package
        self.register_class(java_lang::create_reflect_method_class());
        self.register_class(java_lang::create_reflect_field_class());
        self.register_class(java_lang::create_reflect_constructor_class());
        self.register_class(java_lang::create_reflect_array_class());
        self.register_class(java_lang::create_reflect_modifier_class());

        // java.lang.Thread
        self.register_class(java_lang::create_thread_class());

        // java.lang process execution classes
        self.register_class(java_lang::create_runtime_class());
        self.register_class(java_lang::create_process_class());
        self.register_class(java_lang::create_process_builder_class());
        self.register_class(java_lang::create_process_builder_redirect_class());
        self.register_interface(java_lang::create_process_handle_interface());
        self.register_interface(java_lang::create_process_handle_info_interface());

        // java.lang.ClassLoader
        self.register_class(java_lang::create_classloader_class());

        // java.lang threading classes
        self.register_class(java_lang::create_threadlocal_class());
        self.register_class(java_lang::create_inheritable_threadlocal_class());
    }

    fn register_core_exception_hierarchy(&mut self) {
        for (name, superclass) in [
            (
                "java/lang/ArithmeticException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/ArrayStoreException",
                "java/lang/RuntimeException",
            ),
            ("java/lang/ClassCastException", "java/lang/RuntimeException"),
            (
                "java/lang/IllegalMonitorStateException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/IllegalStateException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/IndexOutOfBoundsException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/ArrayIndexOutOfBoundsException",
                "java/lang/IndexOutOfBoundsException",
            ),
            (
                "java/lang/StringIndexOutOfBoundsException",
                "java/lang/IndexOutOfBoundsException",
            ),
            (
                "java/lang/NegativeArraySizeException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/NullPointerException",
                "java/lang/RuntimeException",
            ),
            (
                "java/lang/NumberFormatException",
                "java/lang/IllegalArgumentException",
            ),
            ("java/lang/SecurityException", "java/lang/RuntimeException"),
            (
                "java/lang/UnsupportedOperationException",
                "java/lang/RuntimeException",
            ),
            ("java/lang/Error", "java/lang/Throwable"),
            ("java/lang/LinkageError", "java/lang/Error"),
            ("java/lang/VirtualMachineError", "java/lang/Error"),
            ("java/lang/AssertionError", "java/lang/Error"),
            // These two are VirtualMachineError subclasses in the JDK, rather
            // than direct Error subclasses.
            ("java/lang/InternalError", "java/lang/VirtualMachineError"),
            ("java/lang/UnknownError", "java/lang/VirtualMachineError"),
            (
                "java/lang/IncompatibleClassChangeError",
                "java/lang/LinkageError",
            ),
            ("java/lang/ClassFormatError", "java/lang/LinkageError"),
            ("java/lang/NoClassDefFoundError", "java/lang/LinkageError"),
            ("java/lang/BootstrapMethodError", "java/lang/LinkageError"),
            ("java/lang/VerifyError", "java/lang/LinkageError"),
            ("java/lang/UnsatisfiedLinkError", "java/lang/LinkageError"),
            (
                "java/lang/AbstractMethodError",
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                "java/lang/IllegalAccessError",
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                "java/lang/InstantiationError",
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                "java/lang/NoSuchFieldError",
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                "java/lang/NoSuchMethodError",
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                "java/lang/ExceptionInInitializerError",
                "java/lang/LinkageError",
            ),
            (
                "java/lang/OutOfMemoryError",
                "java/lang/VirtualMachineError",
            ),
            (
                "java/lang/StackOverflowError",
                "java/lang/VirtualMachineError",
            ),
            ("java/lang/ClassNotFoundException", "java/lang/Exception"),
            ("java/lang/InterruptedException", "java/lang/Exception"),
            ("java/lang/NoSuchFieldException", "java/lang/Exception"),
            ("java/lang/NoSuchMethodException", "java/lang/Exception"),
        ] {
            let mut methods = HashMap::new();
            for descriptor in ["()V", "(Ljava/lang/String;)V"] {
                methods.insert(
                    format!("<init>:{descriptor}"),
                    StubMethodDescriptor::constructor(descriptor),
                );
            }
            if name == "java/lang/ExceptionInInitializerError" {
                methods.insert(
                    "<init>:(Ljava/lang/Throwable;)V".to_string(),
                    StubMethodDescriptor::constructor("(Ljava/lang/Throwable;)V"),
                );
                methods.insert(
                    "getException:()Ljava/lang/Throwable;".to_string(),
                    StubMethodDescriptor::instance_method(
                        "getException",
                        "()Ljava/lang/Throwable;",
                    ),
                );
            }
            self.register_class(StubClass {
                name: name.to_string(),
                superclass: Some(superclass.to_string()),
                interfaces: vec![],
                methods,
            });
        }
    }

    fn register_java_util_classes(&mut self) {
        // java.util.ArrayList
        self.register_class(java_util::create_array_list_class());

        // java.util.LinkedList
        self.register_class(java_util::create_linked_list_class());

        // java.util.HashMap
        self.register_class(java_util::create_hash_map_class());

        // java.util.LinkedHashMap
        self.register_class(java_util::create_linked_hash_map_class());

        // java.util.HashSet
        self.register_class(java_util::create_hash_set_class());

        // java.util.TreeMap
        self.register_class(java_util::create_tree_map_class());

        // java.util.TreeSet
        self.register_class(java_util::create_tree_set_class());

        // Interfaces
        self.register_interface(java_util::create_list_interface());
        self.register_interface(java_util::create_map_interface());
        self.register_interface(java_util::create_set_interface());
        self.register_interface(java_util::create_collection_interface());
        self.register_interface(java_util::create_iterable_interface());
        self.register_interface(java_util::create_iterator_interface());

        // Utility classes
        self.register_class(java_util::create_arrays_class());
        self.register_class(java_util::create_collections_class());
        self.register_class(java_util::create_objects_class());
        self.register_class(java_util::create_optional_class());

        // Queue/Deque interfaces and implementations
        self.register_interface(java_util::create_queue_interface());
        self.register_interface(java_util::create_deque_interface());
        self.register_class(java_util::create_abstract_queue_class());
        self.register_class(java_util::create_vector_class());
        self.register_class(java_util::create_stack_class());
        self.register_class(java_util::create_priority_queue_class());
        self.register_class(java_util::create_array_deque_class());

        // Legacy classes
        self.register_class(java_util::create_dictionary_class());
        self.register_interface(java_util::create_enumeration_interface());
        self.register_class(java_util::create_hashtable_class());
        self.register_class(java_util::create_properties_class());

        // I/O and utility classes
        self.register_class(java_util::create_scanner_class());
        self.register_class(java_util::create_random_class());
        self.register_class(java_util::create_uuid_class());

        // PrimitiveIterator interfaces
        self.register_interface(java_util::create_primitive_iterator_interface());
        self.register_interface(java_util::create_primitive_iterator_of_int_interface());
        self.register_interface(java_util::create_primitive_iterator_of_long_interface());
        self.register_interface(java_util::create_primitive_iterator_of_double_interface());
    }

    fn register_java_util_regex_classes(&mut self) {
        self.register_interface(java_regex::create_match_result_interface());
        self.register_class(java_regex::create_pattern_class());
        self.register_class(java_regex::create_matcher_class());
        self.register_class(java_regex::create_pattern_syntax_exception_class());
    }

    fn register_java_util_function_classes(&mut self) {
        // Core functional interfaces
        self.register_interface(java_function::create_function_interface());
        self.register_interface(java_function::create_bifunction_interface());
        self.register_interface(java_function::create_consumer_interface());
        self.register_interface(java_function::create_biconsumer_interface());
        self.register_interface(java_function::create_supplier_interface());
        self.register_interface(java_function::create_predicate_interface());
        self.register_interface(java_function::create_bipredicate_interface());
        self.register_interface(java_function::create_unary_operator_interface());
        self.register_interface(java_function::create_binary_operator_interface());

        // Int primitive specializations
        self.register_interface(java_function::create_int_function_interface());
        self.register_interface(java_function::create_int_consumer_interface());
        self.register_interface(java_function::create_int_supplier_interface());
        self.register_interface(java_function::create_int_predicate_interface());
        self.register_interface(java_function::create_int_unary_operator_interface());
        self.register_interface(java_function::create_int_binary_operator_interface());
        self.register_interface(java_function::create_to_int_function_interface());

        // Long primitive specializations
        self.register_interface(java_function::create_long_function_interface());
        self.register_interface(java_function::create_long_consumer_interface());
        self.register_interface(java_function::create_long_supplier_interface());
        self.register_interface(java_function::create_long_predicate_interface());
        self.register_interface(java_function::create_long_unary_operator_interface());
        self.register_interface(java_function::create_long_binary_operator_interface());
        self.register_interface(java_function::create_to_long_function_interface());

        // Double primitive specializations
        self.register_interface(java_function::create_double_function_interface());
        self.register_interface(java_function::create_double_consumer_interface());
        self.register_interface(java_function::create_double_supplier_interface());
        self.register_interface(java_function::create_double_predicate_interface());
        self.register_interface(java_function::create_double_unary_operator_interface());
        self.register_interface(java_function::create_double_binary_operator_interface());
        self.register_interface(java_function::create_to_double_function_interface());

        // Binary Object-Primitive Consumers
        self.register_interface(java_function::create_obj_int_consumer_interface());
        self.register_interface(java_function::create_obj_long_consumer_interface());
        self.register_interface(java_function::create_obj_double_consumer_interface());

        // Binary ToX Functions
        self.register_interface(java_function::create_to_int_bifunction_interface());
        self.register_interface(java_function::create_to_long_bifunction_interface());
        self.register_interface(java_function::create_to_double_bifunction_interface());

        // Cross-Primitive Conversion Functions
        self.register_interface(java_function::create_int_to_long_function_interface());
        self.register_interface(java_function::create_int_to_double_function_interface());
        self.register_interface(java_function::create_long_to_int_function_interface());
        self.register_interface(java_function::create_long_to_double_function_interface());
        self.register_interface(java_function::create_double_to_int_function_interface());
        self.register_interface(java_function::create_double_to_long_function_interface());
    }

    fn register_java_io_classes(&mut self) {
        // Interfaces
        self.register_interface(java_io::create_closeable_interface());
        self.register_interface(java_io::create_flushable_interface());
        self.register_interface(java_io::create_serializable_interface());
        self.register_interface(java_io::create_file_filter_interface());
        self.register_interface(java_io::create_filename_filter_interface());
        self.register_interface(java_io::create_data_input_interface());
        self.register_interface(java_io::create_data_output_interface());
        self.register_interface(java_io::create_object_input_interface());
        self.register_interface(java_io::create_object_output_interface());

        // FileDescriptor
        self.register_class(java_io::create_file_descriptor_class());

        // File
        self.register_class(java_io::create_file_class());

        // InputStream hierarchy
        self.register_class(java_io::create_input_stream_class());
        self.register_class(java_io::create_filter_input_stream_class());
        self.register_class(java_io::create_file_input_stream_class());
        self.register_class(java_io::create_buffered_input_stream_class());
        self.register_class(java_io::create_data_input_stream_class());
        self.register_class(java_io::create_byte_array_input_stream_class());
        self.register_class(java_io::create_object_input_stream_class());

        // OutputStream hierarchy
        self.register_class(java_io::create_output_stream_class());
        self.register_class(java_io::create_filter_output_stream_class());
        self.register_class(java_io::create_file_output_stream_class());
        self.register_class(java_io::create_buffered_output_stream_class());
        self.register_class(java_io::create_data_output_stream_class());
        self.register_class(java_io::create_byte_array_output_stream_class());
        self.register_class(java_io::create_object_output_stream_class());
        self.register_class(java_io::create_print_stream_class());

        // Reader hierarchy
        self.register_class(java_io::create_reader_class());
        self.register_class(java_io::create_input_stream_reader_class());
        self.register_class(java_io::create_file_reader_class());
        self.register_class(java_io::create_buffered_reader_class());
        self.register_class(java_io::create_string_reader_class());

        // Writer hierarchy
        self.register_class(java_io::create_writer_class());
        self.register_class(java_io::create_output_stream_writer_class());
        self.register_class(java_io::create_file_writer_class());
        self.register_class(java_io::create_buffered_writer_class());
        self.register_class(java_io::create_string_writer_class());
        self.register_class(java_io::create_print_writer_class());

        // Exceptions
        self.register_class(java_io::create_io_exception_class());
        self.register_class(java_io::create_file_not_found_exception_class());
        self.register_class(java_io::create_eof_exception_class());
        self.register_class(java_io::create_unchecked_io_exception_class());
    }

    fn register_java_math_classes(&mut self) {
        self.register_class(java_math::create_big_integer_class());
        self.register_class(java_math::create_big_decimal_class());
        self.register_class(java_math::create_math_context_class());
        self.register_class(java_math::create_rounding_mode_class());
    }

    fn register_groovy_lang_classes(&mut self) {
        // Core interfaces - THE FOUNDATION
        self.register_interface(groovy_lang::create_groovy_object_interface());
        self.register_class(groovy_lang::create_groovy_object_support_class());

        // MetaClass system - THE DSL ENGINE
        self.register_interface(groovy_lang::create_meta_object_protocol_interface());
        self.register_interface(groovy_lang::create_meta_class_interface());
        self.register_class(groovy_lang::create_meta_method_class());
        self.register_class(groovy_lang::create_meta_property_class());
        self.register_class(groovy_lang::create_meta_bean_property_class());
        self.register_class(groovy_lang::create_meta_class_impl_class());
        self.register_class(groovy_lang::create_expando_meta_class_class());
        self.register_interface(groovy_lang::create_meta_class_registry_interface());
        self.register_interface(groovy_lang::create_mixin_in_meta_class_interface());

        // Script and Binding - Gradle .gradle file support
        self.register_class(groovy_lang::create_script_class());
        self.register_class(groovy_lang::create_delegating_script_class());
        self.register_class(groovy_lang::create_binding_class());

        // Closure - THE HEART OF GRADLE DSL
        self.register_class(groovy_lang::create_closure_class());

        // GString - interpolated strings
        self.register_class(groovy_lang::create_gstring_class());

        // Range classes
        self.register_interface(groovy_lang::create_range_interface());
        self.register_class(groovy_lang::create_int_range_class());
        self.register_class(groovy_lang::create_object_range_class());
        self.register_class(groovy_lang::create_empty_range_class());

        // Utility interfaces
        self.register_interface(groovy_lang::create_writable_interface());
        self.register_interface(groovy_lang::create_groovy_callable_interface());
        self.register_interface(groovy_lang::create_groovy_interceptable_interface());

        // Exception classes
        self.register_class(groovy_lang::create_groovy_runtime_exception_class());
        self.register_class(groovy_lang::create_missing_method_exception_class());
        self.register_class(groovy_lang::create_missing_property_exception_class());

        // Tuple classes
        self.register_class(groovy_lang::create_tuple_class());
        self.register_class(groovy_lang::create_tuple2_class());

        // Utility classes
        self.register_class(groovy_lang::create_reference_class());
        self.register_class(groovy_lang::create_spread_map_class());

        // Shell and ClassLoader
        self.register_class(groovy_lang::create_groovy_shell_class());
        self.register_class(groovy_lang::create_groovy_class_loader_class());
        self.register_class(groovy_lang::create_groovy_code_source_class());
    }

    fn register_java_util_concurrent_classes(&mut self) {
        // Executor framework
        self.register_interface(java_concurrent::create_executor_interface());
        self.register_interface(java_concurrent::create_executor_service_interface());
        self.register_class(java_concurrent::create_executors_class());
        self.register_class(java_concurrent::create_abstract_executor_service_class());
        self.register_class(java_concurrent::create_thread_pool_executor_class());

        // Scheduled execution
        self.register_interface(java_concurrent::create_scheduled_executor_service_interface());
        self.register_interface(java_concurrent::create_scheduled_future_interface());
        self.register_interface(java_concurrent::create_delayed_interface());

        // Fork/Join framework
        self.register_class(java_concurrent::create_fork_join_pool_class());
        self.register_class(java_concurrent::create_fork_join_task_class());
        self.register_class(java_concurrent::create_fork_join_worker_thread_class());
        self.register_interface(
            java_concurrent::create_fork_join_worker_thread_factory_interface(),
        );

        // Futures and Callables
        self.register_interface(java_concurrent::create_future_interface());
        self.register_interface(java_concurrent::create_callable_interface());
        self.register_class(java_concurrent::create_completable_future_class());

        // Locks
        self.register_interface(java_concurrent::create_lock_interface());
        self.register_class(java_concurrent::create_reentrant_lock_class());
        self.register_interface(java_concurrent::create_condition_interface());

        // Synchronization primitives
        self.register_class(java_concurrent::create_semaphore_class());
        self.register_class(java_concurrent::create_count_down_latch_class());
        self.register_class(java_concurrent::create_cyclic_barrier_class());
        self.register_class(java_concurrent::create_phaser_class());
        self.register_class(java_concurrent::create_broken_barrier_exception_class());

        // Atomic classes
        self.register_class(java_concurrent::create_atomic_integer_class());
        self.register_class(java_concurrent::create_atomic_long_class());
        self.register_class(java_concurrent::create_atomic_boolean_class());
        self.register_class(java_concurrent::create_atomic_reference_class());
        self.register_class(java_concurrent::create_atomic_integer_array_class());

        // Concurrent collections
        self.register_class(java_concurrent::create_concurrent_hash_map_class());
        self.register_interface(java_concurrent::create_blocking_queue_interface());

        // Factory interfaces
        self.register_interface(java_concurrent::create_thread_factory_interface());
        self.register_interface(java_concurrent::create_rejected_execution_handler_interface());

        // Time utilities
        self.register_class(java_concurrent::create_time_unit_class());
    }

    fn register_java_util_stream_classes(&mut self) {
        // Core stream interfaces
        self.register_interface(java_stream::create_base_stream_interface());
        self.register_interface(java_stream::create_stream_interface());

        // Primitive stream specializations
        self.register_interface(java_stream::create_int_stream_interface());
        self.register_interface(java_stream::create_long_stream_interface());
        self.register_interface(java_stream::create_double_stream_interface());

        // Collectors
        self.register_interface(java_stream::create_collector_interface());
        self.register_class(java_stream::create_collectors_class());

        // Spliterator interfaces
        self.register_interface(java_stream::create_spliterator_interface());
        self.register_interface(java_stream::create_spliterator_of_primitive_interface());
        self.register_interface(java_stream::create_spliterator_of_int_interface());
        self.register_interface(java_stream::create_spliterator_of_long_interface());
        self.register_interface(java_stream::create_spliterator_of_double_interface());

        // StreamSupport utility class
        self.register_class(java_stream::create_stream_support_class());
    }

    fn register_java_nio_classes(&mut self) {
        // Path and file utilities
        self.register_interface(java_nio::create_path_interface());
        self.register_class(java_nio::create_paths_class());
        self.register_class(java_nio::create_files_class());

        // Buffers
        self.register_class(java_nio::create_byte_buffer_class());
        self.register_class(java_nio::create_char_buffer_class());

        // Channels
        self.register_class(java_nio::create_file_channel_class());

        // Enums and utilities
        self.register_class(java_nio::create_standard_open_option_enum());
        self.register_class(java_nio::create_byte_order_class());

        // FileSystem classes
        self.register_class(java_nio::create_file_system_class());
        self.register_class(java_nio::create_file_systems_class());
        self.register_class(java_nio::create_file_system_provider_class());
        self.register_class(java_nio::create_file_store_class());

        // Watch service interfaces
        self.register_interface(java_nio::create_watch_service_interface());
        self.register_interface(java_nio::create_watch_key_interface());
        self.register_interface(java_nio::create_watch_event_interface());
        self.register_interface(java_nio::create_watch_event_kind_interface());
        self.register_interface(java_nio::create_watch_event_modifier_interface());
        self.register_interface(java_nio::create_watchable_interface());
        self.register_class(java_nio::create_standard_watch_event_kinds_class());

        // Channel interfaces
        self.register_interface(java_nio::create_channel_interface());
        self.register_interface(java_nio::create_readable_byte_channel_interface());
        self.register_interface(java_nio::create_writable_byte_channel_interface());
        self.register_interface(java_nio::create_byte_channel_interface());
        self.register_interface(java_nio::create_seekable_byte_channel_interface());
        self.register_interface(java_nio::create_asynchronous_channel_interface());
        self.register_interface(java_nio::create_asynchronous_byte_channel_interface());
        self.register_interface(java_nio::create_completion_handler_interface());

        // Directory stream and filter
        self.register_interface(java_nio::create_directory_stream_interface());
        self.register_interface(java_nio::create_directory_stream_filter_interface());

        // PathMatcher
        self.register_interface(java_nio::create_path_matcher_interface());

        // Enums and options
        self.register_class(java_nio::create_access_mode_enum());
        self.register_class(java_nio::create_link_option_enum());
        self.register_interface(java_nio::create_copy_option_interface());
        self.register_interface(java_nio::create_open_option_interface());

        // Channels utility class
        self.register_class(java_nio::create_channels_class());
    }
}

impl Default for JdkStubRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a stubbed object instance on the heap
#[derive(Debug, Clone)]
pub struct StubObject {
    /// The class this object is an instance of
    pub class_name: String,
    /// Object fields stored as key-value pairs
    pub fields: HashMap<String, Value>,
}

impl StubObject {
    pub fn new(class_name: &str) -> Self {
        Self {
            class_name: class_name.to_string(),
            fields: HashMap::new(),
        }
    }

    pub fn with_field(mut self, name: &str, value: Value) -> Self {
        self.fields.insert(name.to_string(), value);
        self
    }

    pub fn set_field(&mut self, name: &str, value: Value) {
        self.fields.insert(name.to_string(), value);
    }

    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }
}

/// Convert a stub class to a LoadedClass for the method area
///
/// This allows JDK stub classes to participate in inheritance and method resolution
/// just like bytecode-loaded classes.
impl StubClass {
    /// Convert this stub class to a LoadedClass
    pub fn to_loaded_class(&self) -> super::method_area::LoadedClass {
        use super::method_area::{
            ClassInitState, FieldLayout, LoadedClass, NonVirtualMethodDeclaration,
        };

        let mut loaded = LoadedClass {
            name: self.name.clone(),
            superclass: self.superclass.clone(),
            interfaces: self.interfaces.clone(),
            vtable: Vec::new(),
            resolved_virtual_methods: Vec::new(),
            non_virtual_methods: Vec::new(),
            itable: Vec::new(),
            field_layout: FieldLayout::new(),
            init_state: ClassInitState::Initialized, // Stubs are always initialized
            declared_fields: builtin_field_declarations(&self.name),
            constant_field_values: builtin_constant_field_values(&self.name),
            modifiers: 0x0001, // ACC_PUBLIC
            is_interface: self.superclass.is_none() && !self.name.contains("java/lang/Object"),
            is_native_stub: true,
            default_conflict_count: 0,
            source_file: None,
            nest: None,
            bootstrap_methods: Vec::new(),
        };

        // Add instance methods to vtable
        for method in self.methods.values() {
            if !method.is_static && !method.is_constructor {
                // Stub descriptors do not currently distinguish Java defaults
                // from abstract interface declarations. Treat their
                // native-backed methods as executable; parsed user classes
                // retain the exact ACC_ABSTRACT bit.
                loaded.add_vtable_entry_with_flags(
                    method.name.clone(),
                    method.descriptor.clone(),
                    self.name.clone(),
                    0x0001,
                );
            } else if method.is_static || method.is_constructor {
                loaded
                    .non_virtual_methods
                    .push(NonVirtualMethodDeclaration {
                        name: Arc::from(method.name.clone()),
                        descriptor: Arc::from(method.descriptor.clone()),
                        access_flags: if method.is_static { 0x0009 } else { 0x0001 },
                    });
            }
        }

        loaded
    }
}

fn builtin_field_declarations(class_name: &str) -> Arc<[super::method_area::FieldDeclaration]> {
    use super::method_area::FieldDeclaration;

    const PUBLIC_STATIC_FINAL: u16 = 0x0001 | 0x0008 | 0x0010;
    let fields: &[(&str, &str)] = match class_name {
        "java/lang/System" => &[
            ("out", "Ljava/io/PrintStream;"),
            ("err", "Ljava/io/PrintStream;"),
        ],
        "java/lang/Integer" => &[
            ("MAX_VALUE", "I"),
            ("MIN_VALUE", "I"),
            ("SIZE", "I"),
            ("BYTES", "I"),
        ],
        "java/lang/Long" => &[
            ("MAX_VALUE", "J"),
            ("MIN_VALUE", "J"),
            ("SIZE", "I"),
            ("BYTES", "I"),
        ],
        "java/lang/Short" => &[("MAX_VALUE", "S"), ("MIN_VALUE", "S")],
        "java/lang/Byte" => &[("MAX_VALUE", "B"), ("MIN_VALUE", "B")],
        "java/lang/Character" => &[("MAX_VALUE", "C"), ("MIN_VALUE", "C")],
        "java/lang/Float" => &[("MAX_VALUE", "F"), ("MIN_VALUE", "F")],
        "java/lang/Double" => &[
            ("MAX_VALUE", "D"),
            ("MIN_VALUE", "D"),
            ("POSITIVE_INFINITY", "D"),
            ("NEGATIVE_INFINITY", "D"),
            ("NaN", "D"),
        ],
        "java/lang/Math" => &[("PI", "D"), ("E", "D")],
        "java/awt/Color" => &[
            ("BLACK", "Ljava/awt/Color;"),
            ("black", "Ljava/awt/Color;"),
            ("WHITE", "Ljava/awt/Color;"),
            ("white", "Ljava/awt/Color;"),
            ("RED", "Ljava/awt/Color;"),
            ("red", "Ljava/awt/Color;"),
            ("GREEN", "Ljava/awt/Color;"),
            ("green", "Ljava/awt/Color;"),
            ("BLUE", "Ljava/awt/Color;"),
            ("blue", "Ljava/awt/Color;"),
            ("YELLOW", "Ljava/awt/Color;"),
            ("yellow", "Ljava/awt/Color;"),
            ("CYAN", "Ljava/awt/Color;"),
            ("cyan", "Ljava/awt/Color;"),
            ("MAGENTA", "Ljava/awt/Color;"),
            ("magenta", "Ljava/awt/Color;"),
            ("GRAY", "Ljava/awt/Color;"),
            ("gray", "Ljava/awt/Color;"),
            ("LIGHT_GRAY", "Ljava/awt/Color;"),
            ("lightGray", "Ljava/awt/Color;"),
            ("DARK_GRAY", "Ljava/awt/Color;"),
            ("darkGray", "Ljava/awt/Color;"),
            ("ORANGE", "Ljava/awt/Color;"),
            ("orange", "Ljava/awt/Color;"),
            ("PINK", "Ljava/awt/Color;"),
            ("pink", "Ljava/awt/Color;"),
        ],
        "java/awt/image/BufferedImage" => &[("TYPE_INT_RGB", "I"), ("TYPE_INT_ARGB", "I")],
        _ => &[],
    };
    fields
        .iter()
        .map(|(name, descriptor)| FieldDeclaration::new(*name, *descriptor, PUBLIC_STATIC_FINAL))
        .collect::<Vec<_>>()
        .into()
}

fn builtin_constant_field_values(
    class_name: &str,
) -> Arc<[super::method_area::ConstantFieldValue]> {
    use super::method_area::{ConstantFieldValue, FieldConstantValue};

    let values: &[(&str, FieldConstantValue)] = match class_name {
        "java/lang/Integer" => &[
            ("MAX_VALUE", FieldConstantValue::Int(i32::MAX)),
            ("MIN_VALUE", FieldConstantValue::Int(i32::MIN)),
            ("SIZE", FieldConstantValue::Int(32)),
            ("BYTES", FieldConstantValue::Int(4)),
        ],
        "java/lang/Long" => &[
            ("MAX_VALUE", FieldConstantValue::Long(i64::MAX)),
            ("MIN_VALUE", FieldConstantValue::Long(i64::MIN)),
            ("SIZE", FieldConstantValue::Int(64)),
            ("BYTES", FieldConstantValue::Int(8)),
        ],
        "java/lang/Short" => &[
            ("MAX_VALUE", FieldConstantValue::Int(i32::from(i16::MAX))),
            ("MIN_VALUE", FieldConstantValue::Int(i32::from(i16::MIN))),
        ],
        "java/lang/Byte" => &[
            ("MAX_VALUE", FieldConstantValue::Int(i32::from(i8::MAX))),
            ("MIN_VALUE", FieldConstantValue::Int(i32::from(i8::MIN))),
        ],
        "java/lang/Character" => &[
            ("MAX_VALUE", FieldConstantValue::Int(i32::from(u16::MAX))),
            ("MIN_VALUE", FieldConstantValue::Int(0)),
        ],
        "java/lang/Float" => &[
            ("MAX_VALUE", FieldConstantValue::Float(f32::MAX)),
            ("MIN_VALUE", FieldConstantValue::Float(f32::from_bits(1))),
        ],
        "java/lang/Double" => &[
            ("MAX_VALUE", FieldConstantValue::Double(f64::MAX)),
            ("MIN_VALUE", FieldConstantValue::Double(f64::from_bits(1))),
            (
                "POSITIVE_INFINITY",
                FieldConstantValue::Double(f64::INFINITY),
            ),
            (
                "NEGATIVE_INFINITY",
                FieldConstantValue::Double(f64::NEG_INFINITY),
            ),
            ("NaN", FieldConstantValue::Double(f64::NAN)),
        ],
        "java/lang/Math" => &[
            ("PI", FieldConstantValue::Double(std::f64::consts::PI)),
            ("E", FieldConstantValue::Double(std::f64::consts::E)),
        ],
        "java/awt/image/BufferedImage" => &[
            ("TYPE_INT_RGB", FieldConstantValue::Int(1)),
            ("TYPE_INT_ARGB", FieldConstantValue::Int(2)),
        ],
        _ => &[],
    };
    let declarations = builtin_field_declarations(class_name);
    values
        .iter()
        .filter_map(|(name, value)| {
            declarations
                .iter()
                .position(|field| field.name.as_ref() == *name)
                .map(|declaration_index| ConstantFieldValue {
                    declaration_index: declaration_index as u16,
                    value: value.clone(),
                })
        })
        .collect::<Vec<_>>()
        .into()
}

impl JdkStubRegistry {
    /// Get a LoadedClass for a stub class
    ///
    /// Returns None if the class is not a stub class
    pub fn get_loaded_class(&self, class_name: &str) -> Option<super::method_area::LoadedClass> {
        self.get_class(class_name)
            .map(|stub| stub.to_loaded_class())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::ObjectRef;

    #[test]
    fn builtin_registry_is_shared_and_customization_is_copy_on_write() {
        let shared = JdkStubRegistry::new();
        let mut customized = JdkStubRegistry::new();
        assert!(Arc::ptr_eq(&shared.catalog, &customized.catalog));

        customized.register_class(StubClass {
            name: "test/CustomNative".to_string(),
            superclass: Some("java/lang/Object".to_string()),
            interfaces: vec![],
            methods: HashMap::new(),
        });

        assert!(!Arc::ptr_eq(&shared.catalog, &customized.catalog));
        assert!(!shared.is_stub_class("test/CustomNative"));
        assert!(customized.is_stub_class("test/CustomNative"));
        assert!(Arc::ptr_eq(
            &shared.catalog,
            &JdkStubRegistry::new().catalog
        ));
        assert_eq!(
            std::mem::size_of::<JdkStubRegistry>(),
            std::mem::size_of::<Arc<StubCatalog>>()
        );
    }

    #[test]
    fn every_builtin_stub_has_a_linkable_structural_closure() {
        let registry = JdkStubRegistry::new();
        let mut names: Vec<_> = registry.catalog.classes.keys().cloned().collect();
        names.sort_unstable();
        let method_area = crate::interpreter::method_area::MethodArea::new();

        let mut failures = Vec::new();
        for name in names {
            if let Err(error) = method_area.get_class(&name) {
                failures.push(format!("{name}: {error}"));
            }
        }
        assert!(
            failures.is_empty(),
            "native stubs failed structural linking:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn functional_stubs_are_interfaces_with_transitive_operator_inheritance() {
        let registry = JdkStubRegistry::new();
        for (name, class) in registry
            .catalog
            .classes
            .iter()
            .filter(|(name, _)| name.starts_with("java/util/function/"))
        {
            assert!(
                class.superclass.is_none(),
                "functional interface {name} was encoded as a class"
            );
        }

        let method_area = crate::interpreter::method_area::MethodArea::new();
        let binary_operator = method_area
            .get_class("java/util/function/BinaryOperator")
            .unwrap();
        assert!(binary_operator.is_interface);
        assert_eq!(
            binary_operator
                .resolve_virtual_declaration(
                    "apply",
                    "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
                )
                .unwrap()
                .declaring_class
                .as_ref(),
            "java/util/function/BiFunction"
        );
    }

    #[test]
    fn gradle_api_stubs_are_interfaces() {
        let registry = JdkStubRegistry::new();
        for name in [
            "org/gradle/api/Project",
            "org/gradle/api/Plugin",
            "org/gradle/api/Action",
            "org/gradle/api/plugins/PluginContainer",
            "org/gradle/api/artifacts/dsl/DependencyHandler",
        ] {
            let class = registry
                .get_loaded_class(name)
                .unwrap_or_else(|| panic!("missing Gradle API stub {name}"));
            assert!(
                class.is_interface,
                "Gradle API stub {name} is not an interface"
            );
            assert!(
                !class.vtable.is_empty(),
                "Gradle API stub {name} has no symbolic method declarations"
            );
        }
    }

    #[test]
    fn list_sort_is_an_exact_public_instance_declaration() {
        let list = JdkStubRegistry::new()
            .get_loaded_class("java/util/List")
            .expect("List stub");
        assert!(list.is_interface);
        let declaration = list
            .resolve_virtual_declaration("sort", "(Ljava/util/Comparator;)V")
            .expect("exact List.sort declaration");
        assert_eq!(declaration.declaring_class.as_ref(), "java/util/List");
        assert_eq!(declaration.access_flags & 0x0001, 0x0001);
    }

    #[test]
    fn test_registry_creation() {
        let registry = JdkStubRegistry::new();

        // Check core classes exist
        assert!(registry.is_stub_class("java/lang/Object"));
        assert!(registry.is_stub_class("java/lang/String"));
        assert!(registry.is_stub_class("java/lang/StringBuilder"));
        assert!(registry.is_stub_class("java/lang/Integer"));
        assert!(registry.is_stub_class("java/util/ArrayList"));
        assert!(registry.is_stub_class("java/util/HashMap"));
        assert!(registry.is_stub_class("groovy/lang/IntRange"));
    }

    #[test]
    fn test_class_methods() {
        let registry = JdkStubRegistry::new();

        let string_class = registry.get_class("java/lang/String").unwrap();
        assert!(string_class.methods.contains_key("length:()I"));
        assert!(string_class.methods.contains_key("charAt:(I)C"));

        let array_list = registry.get_class("java/util/ArrayList").unwrap();
        assert!(array_list.methods.contains_key("<init>:()V"));
        assert!(array_list.methods.contains_key("add:(Ljava/lang/Object;)Z"));
    }

    #[test]
    fn test_stub_object() {
        let mut obj = StubObject::new("java/lang/StringBuilder");
        obj.set_field("buffer", Value::Reference(ObjectRef::Null));

        assert_eq!(obj.class_name, "java/lang/StringBuilder");
        assert!(obj.get_field("buffer").is_some());
    }

    #[test]
    fn test_new_java_lang_classes() {
        let registry = JdkStubRegistry::new();

        // Verify ClassLoader
        assert!(registry.is_stub_class("java/lang/ClassLoader"));
        let cl = registry.get_class("java/lang/ClassLoader").unwrap();
        assert!(cl
            .methods
            .contains_key("loadClass:(Ljava/lang/String;)Ljava/lang/Class;"));
        assert!(cl
            .methods
            .contains_key("getSystemClassLoader:()Ljava/lang/ClassLoader;"));
        assert!(cl
            .methods
            .contains_key("getResource:(Ljava/lang/String;)Ljava/net/URL;"));

        // Verify ThreadLocal
        assert!(registry.is_stub_class("java/lang/ThreadLocal"));
        let tl = registry.get_class("java/lang/ThreadLocal").unwrap();
        assert!(tl.methods.contains_key("get:()Ljava/lang/Object;"));
        assert!(tl.methods.contains_key("set:(Ljava/lang/Object;)V"));
        assert!(tl
            .methods
            .contains_key("withInitial:(Ljava/util/function/Supplier;)Ljava/lang/ThreadLocal;"));

        // Verify InheritableThreadLocal
        assert!(registry.is_stub_class("java/lang/InheritableThreadLocal"));
        let itl = registry
            .get_class("java/lang/InheritableThreadLocal")
            .unwrap();
        assert_eq!(itl.superclass, Some("java/lang/ThreadLocal".to_string()));
        assert!(itl
            .methods
            .contains_key("childValue:(Ljava/lang/Object;)Ljava/lang/Object;"));
    }

    #[test]
    fn test_queue_deque_collections() {
        let registry = JdkStubRegistry::new();

        // Queue interface
        assert!(registry.is_stub_class("java/util/Queue"));
        let queue = registry.get_class("java/util/Queue").unwrap();
        assert!(queue.methods.contains_key("offer:(Ljava/lang/Object;)Z"));
        assert!(queue.methods.contains_key("poll:()Ljava/lang/Object;"));
        assert!(queue.methods.contains_key("peek:()Ljava/lang/Object;"));

        // Deque interface
        assert!(registry.is_stub_class("java/util/Deque"));
        let deque = registry.get_class("java/util/Deque").unwrap();
        assert!(deque.methods.contains_key("addFirst:(Ljava/lang/Object;)V"));
        assert!(deque.methods.contains_key("addLast:(Ljava/lang/Object;)V"));
        assert!(deque.methods.contains_key("pollFirst:()Ljava/lang/Object;"));
        assert!(deque.methods.contains_key("pollLast:()Ljava/lang/Object;"));
        assert!(deque
            .methods
            .contains_key("descendingIterator:()Ljava/util/Iterator;"));

        // Vector class
        assert!(registry.is_stub_class("java/util/Vector"));
        let vector = registry.get_class("java/util/Vector").unwrap();
        assert_eq!(
            vector.superclass,
            Some("java/util/AbstractList".to_string())
        );
        assert!(vector
            .methods
            .contains_key("addElement:(Ljava/lang/Object;)V"));
        assert!(vector
            .methods
            .contains_key("elementAt:(I)Ljava/lang/Object;"));

        // Stack class
        assert!(registry.is_stub_class("java/util/Stack"));
        let stack = registry.get_class("java/util/Stack").unwrap();
        assert_eq!(stack.superclass, Some("java/util/Vector".to_string()));
        assert!(stack
            .methods
            .contains_key("push:(Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(stack.methods.contains_key("pop:()Ljava/lang/Object;"));
        assert!(stack.methods.contains_key("empty:()Z"));
        assert!(stack.methods.contains_key("search:(Ljava/lang/Object;)I"));

        // PriorityQueue class
        assert!(registry.is_stub_class("java/util/PriorityQueue"));
        let pq = registry.get_class("java/util/PriorityQueue").unwrap();
        assert_eq!(pq.superclass, Some("java/util/AbstractQueue".to_string()));
        assert!(pq.methods.contains_key("<init>:(Ljava/util/Comparator;)V"));
        assert!(pq
            .methods
            .contains_key("comparator:()Ljava/util/Comparator;"));

        // ArrayDeque class
        assert!(registry.is_stub_class("java/util/ArrayDeque"));
        let ad = registry.get_class("java/util/ArrayDeque").unwrap();
        assert_eq!(
            ad.superclass,
            Some("java/util/AbstractCollection".to_string())
        );
        assert!(ad.interfaces.contains(&"java/util/Deque".to_string()));
        assert!(ad.methods.contains_key("addFirst:(Ljava/lang/Object;)V"));
        assert!(ad.methods.contains_key("push:(Ljava/lang/Object;)V"));
        assert!(ad.methods.contains_key("clone:()Ljava/util/ArrayDeque;"));

        // AbstractQueue class
        assert!(registry.is_stub_class("java/util/AbstractQueue"));
        let aq = registry.get_class("java/util/AbstractQueue").unwrap();
        assert_eq!(
            aq.superclass,
            Some("java/util/AbstractCollection".to_string())
        );
        assert!(aq.interfaces.contains(&"java/util/Queue".to_string()));
    }

    #[test]
    fn test_hashtable_properties_classes() {
        let registry = JdkStubRegistry::new();

        // Dictionary abstract class
        assert!(registry.is_stub_class("java/util/Dictionary"));
        let dict = registry.get_class("java/util/Dictionary").unwrap();
        assert_eq!(dict.superclass, Some("java/lang/Object".to_string()));
        assert!(dict.methods.contains_key("keys:()Ljava/util/Enumeration;"));
        assert!(dict
            .methods
            .contains_key("elements:()Ljava/util/Enumeration;"));

        // Enumeration interface
        assert!(registry.is_stub_class("java/util/Enumeration"));
        let enumeration = registry.get_class("java/util/Enumeration").unwrap();
        assert!(enumeration.methods.contains_key("hasMoreElements:()Z"));
        assert!(enumeration
            .methods
            .contains_key("nextElement:()Ljava/lang/Object;"));

        // Hashtable class
        assert!(registry.is_stub_class("java/util/Hashtable"));
        let hashtable = registry.get_class("java/util/Hashtable").unwrap();
        assert_eq!(
            hashtable.superclass,
            Some("java/util/Dictionary".to_string())
        );
        assert!(hashtable.interfaces.contains(&"java/util/Map".to_string()));
        assert!(hashtable.methods.contains_key("<init>:()V"));
        assert!(hashtable.methods.contains_key("<init>:(IF)V"));
        assert!(hashtable
            .methods
            .contains_key("contains:(Ljava/lang/Object;)Z"));
        assert!(hashtable
            .methods
            .contains_key("keys:()Ljava/util/Enumeration;"));
        assert!(hashtable
            .methods
            .contains_key("elements:()Ljava/util/Enumeration;"));

        // Properties class
        assert!(registry.is_stub_class("java/util/Properties"));
        let props = registry.get_class("java/util/Properties").unwrap();
        assert_eq!(props.superclass, Some("java/util/Hashtable".to_string()));
        assert!(props.methods.contains_key("<init>:()V"));
        assert!(props
            .methods
            .contains_key("<init>:(Ljava/util/Properties;)V"));
        assert!(props
            .methods
            .contains_key("getProperty:(Ljava/lang/String;)Ljava/lang/String;"));
        assert!(props
            .methods
            .contains_key("getProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"));
        assert!(props
            .methods
            .contains_key("setProperty:(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;"));
        assert!(props.methods.contains_key("load:(Ljava/io/InputStream;)V"));
        assert!(props
            .methods
            .contains_key("store:(Ljava/io/OutputStream;Ljava/lang/String;)V"));
        assert!(props
            .methods
            .contains_key("propertyNames:()Ljava/util/Enumeration;"));
        assert!(props
            .methods
            .contains_key("stringPropertyNames:()Ljava/util/Set;"));
    }

    #[test]
    fn test_regex_classes() {
        let registry = JdkStubRegistry::new();

        // MatchResult interface
        assert!(registry.is_stub_class("java/util/regex/MatchResult"));
        let match_result = registry.get_class("java/util/regex/MatchResult").unwrap();
        assert!(match_result.superclass.is_none()); // interface
        assert!(match_result.methods.contains_key("start:()I"));
        assert!(match_result.methods.contains_key("end:()I"));
        assert!(match_result
            .methods
            .contains_key("group:()Ljava/lang/String;"));
        assert!(match_result.methods.contains_key("groupCount:()I"));

        // Pattern class
        assert!(registry.is_stub_class("java/util/regex/Pattern"));
        let pattern = registry.get_class("java/util/regex/Pattern").unwrap();
        assert_eq!(pattern.superclass, Some("java/lang/Object".to_string()));
        assert!(pattern
            .interfaces
            .contains(&"java/io/Serializable".to_string()));
        assert!(pattern
            .methods
            .contains_key("compile:(Ljava/lang/String;)Ljava/util/regex/Pattern;"));
        assert!(pattern
            .methods
            .contains_key("compile:(Ljava/lang/String;I)Ljava/util/regex/Pattern;"));
        assert!(pattern
            .methods
            .contains_key("matcher:(Ljava/lang/CharSequence;)Ljava/util/regex/Matcher;"));
        assert!(pattern.methods.contains_key("pattern:()Ljava/lang/String;"));
        assert!(pattern.methods.contains_key("flags:()I"));
        assert!(pattern
            .methods
            .contains_key("split:(Ljava/lang/CharSequence;)[Ljava/lang/String;"));
        assert!(pattern
            .methods
            .contains_key("matches:(Ljava/lang/String;Ljava/lang/CharSequence;)Z"));
        assert!(pattern
            .methods
            .contains_key("quote:(Ljava/lang/String;)Ljava/lang/String;"));

        // Matcher class
        assert!(registry.is_stub_class("java/util/regex/Matcher"));
        let matcher = registry.get_class("java/util/regex/Matcher").unwrap();
        assert_eq!(matcher.superclass, Some("java/lang/Object".to_string()));
        assert!(matcher
            .interfaces
            .contains(&"java/util/regex/MatchResult".to_string()));
        assert!(matcher
            .methods
            .contains_key("pattern:()Ljava/util/regex/Pattern;"));
        assert!(matcher.methods.contains_key("matches:()Z"));
        assert!(matcher.methods.contains_key("find:()Z"));
        assert!(matcher.methods.contains_key("find:(I)Z"));
        assert!(matcher.methods.contains_key("lookingAt:()Z"));
        assert!(matcher.methods.contains_key("group:()Ljava/lang/String;"));
        assert!(matcher.methods.contains_key("group:(I)Ljava/lang/String;"));
        assert!(matcher
            .methods
            .contains_key("group:(Ljava/lang/String;)Ljava/lang/String;"));
        assert!(matcher.methods.contains_key("start:()I"));
        assert!(matcher.methods.contains_key("end:()I"));
        assert!(matcher
            .methods
            .contains_key("reset:()Ljava/util/regex/Matcher;"));
        assert!(matcher
            .methods
            .contains_key("replaceAll:(Ljava/lang/String;)Ljava/lang/String;"));
        assert!(matcher
            .methods
            .contains_key("replaceFirst:(Ljava/lang/String;)Ljava/lang/String;"));
        assert!(matcher
            .methods
            .contains_key("region:(II)Ljava/util/regex/Matcher;"));
        assert!(matcher.methods.contains_key("hitEnd:()Z"));
        assert!(matcher
            .methods
            .contains_key("toMatchResult:()Ljava/util/regex/MatchResult;"));

        // PatternSyntaxException class
        assert!(registry.is_stub_class("java/util/regex/PatternSyntaxException"));
        let pse = registry
            .get_class("java/util/regex/PatternSyntaxException")
            .unwrap();
        assert_eq!(
            pse.superclass,
            Some("java/lang/IllegalArgumentException".to_string())
        );
        assert!(pse
            .methods
            .contains_key("<init>:(Ljava/lang/String;Ljava/lang/String;I)V"));
        assert!(pse
            .methods
            .contains_key("getDescription:()Ljava/lang/String;"));
        assert!(pse.methods.contains_key("getIndex:()I"));
        assert!(pse.methods.contains_key("getPattern:()Ljava/lang/String;"));
    }

    #[test]
    fn test_scanner_random_uuid_classes() {
        let registry = JdkStubRegistry::new();

        // Scanner class
        assert!(registry.is_stub_class("java/util/Scanner"));
        let scanner = registry.get_class("java/util/Scanner").unwrap();
        assert_eq!(scanner.superclass, Some("java/lang/Object".to_string()));
        assert!(scanner
            .interfaces
            .contains(&"java/util/Iterator".to_string()));
        assert!(scanner
            .interfaces
            .contains(&"java/io/Closeable".to_string()));
        assert!(scanner
            .methods
            .contains_key("<init>:(Ljava/io/InputStream;)V"));
        assert!(scanner.methods.contains_key("<init>:(Ljava/lang/String;)V"));
        assert!(scanner.methods.contains_key("hasNext:()Z"));
        assert!(scanner.methods.contains_key("next:()Ljava/lang/String;"));
        assert!(scanner
            .methods
            .contains_key("nextLine:()Ljava/lang/String;"));
        assert!(scanner.methods.contains_key("nextInt:()I"));
        assert!(scanner.methods.contains_key("nextLong:()J"));
        assert!(scanner.methods.contains_key("nextDouble:()D"));
        assert!(scanner.methods.contains_key("close:()V"));

        // Random class
        assert!(registry.is_stub_class("java/util/Random"));
        let random = registry.get_class("java/util/Random").unwrap();
        assert_eq!(random.superclass, Some("java/lang/Object".to_string()));
        assert!(random
            .interfaces
            .contains(&"java/io/Serializable".to_string()));
        assert!(random.methods.contains_key("<init>:()V"));
        assert!(random.methods.contains_key("<init>:(J)V"));
        assert!(random.methods.contains_key("nextInt:()I"));
        assert!(random.methods.contains_key("nextInt:(I)I"));
        assert!(random.methods.contains_key("nextLong:()J"));
        assert!(random.methods.contains_key("nextDouble:()D"));
        assert!(random.methods.contains_key("nextBoolean:()Z"));
        assert!(random
            .methods
            .contains_key("ints:()Ljava/util/stream/IntStream;"));

        // UUID class
        assert!(registry.is_stub_class("java/util/UUID"));
        let uuid = registry.get_class("java/util/UUID").unwrap();
        assert_eq!(uuid.superclass, Some("java/lang/Object".to_string()));
        assert!(uuid
            .interfaces
            .contains(&"java/io/Serializable".to_string()));
        assert!(uuid
            .interfaces
            .contains(&"java/lang/Comparable".to_string()));
        assert!(uuid.methods.contains_key("<init>:(JJ)V"));
        assert!(uuid.methods.contains_key("randomUUID:()Ljava/util/UUID;"));
        assert!(uuid
            .methods
            .contains_key("fromString:(Ljava/lang/String;)Ljava/util/UUID;"));
        assert!(uuid.methods.contains_key("getMostSignificantBits:()J"));
        assert!(uuid.methods.contains_key("getLeastSignificantBits:()J"));
        assert!(uuid.methods.contains_key("toString:()Ljava/lang/String;"));
        assert!(uuid.methods.contains_key("compareTo:(Ljava/util/UUID;)I"));
    }

    #[test]
    fn test_thread_pool_executor_class() {
        let registry = JdkStubRegistry::new();

        // AbstractExecutorService
        assert!(registry.is_stub_class("java/util/concurrent/AbstractExecutorService"));
        let aes = registry
            .get_class("java/util/concurrent/AbstractExecutorService")
            .unwrap();
        assert_eq!(aes.superclass, Some("java/lang/Object".to_string()));
        assert!(aes
            .interfaces
            .contains(&"java/util/concurrent/ExecutorService".to_string()));

        // ThreadPoolExecutor
        assert!(registry.is_stub_class("java/util/concurrent/ThreadPoolExecutor"));
        let tpe = registry
            .get_class("java/util/concurrent/ThreadPoolExecutor")
            .unwrap();
        assert_eq!(
            tpe.superclass,
            Some("java/util/concurrent/AbstractExecutorService".to_string())
        );
        assert!(tpe.methods.contains_key(
            "<init>:(IIJLjava/util/concurrent/TimeUnit;Ljava/util/concurrent/BlockingQueue;)V"
        ));
        assert!(tpe.methods.contains_key("execute:(Ljava/lang/Runnable;)V"));
        assert!(tpe.methods.contains_key("shutdown:()V"));
        assert!(tpe.methods.contains_key("getCorePoolSize:()I"));
        assert!(tpe.methods.contains_key("getMaximumPoolSize:()I"));
        assert!(tpe.methods.contains_key("getActiveCount:()I"));
        assert!(tpe.methods.contains_key("getTaskCount:()J"));
        assert!(tpe
            .methods
            .contains_key("getQueue:()Ljava/util/concurrent/BlockingQueue;"));
        assert!(tpe
            .methods
            .contains_key("setThreadFactory:(Ljava/util/concurrent/ThreadFactory;)V"));
        assert!(tpe.methods.contains_key(
            "setRejectedExecutionHandler:(Ljava/util/concurrent/RejectedExecutionHandler;)V"
        ));
    }

    #[test]
    fn test_fork_join_framework_classes() {
        let registry = JdkStubRegistry::new();

        // ForkJoinPool
        assert!(registry.is_stub_class("java/util/concurrent/ForkJoinPool"));
        let fjp = registry
            .get_class("java/util/concurrent/ForkJoinPool")
            .unwrap();
        assert_eq!(
            fjp.superclass,
            Some("java/util/concurrent/AbstractExecutorService".to_string())
        );
        assert!(fjp.methods.contains_key("<init>:()V"));
        assert!(fjp.methods.contains_key("<init>:(I)V"));
        assert!(fjp
            .methods
            .contains_key("commonPool:()Ljava/util/concurrent/ForkJoinPool;"));
        assert!(fjp.methods.contains_key("getCommonPoolParallelism:()I"));
        assert!(fjp
            .methods
            .contains_key("invoke:(Ljava/util/concurrent/ForkJoinTask;)Ljava/lang/Object;"));
        assert!(fjp.methods.contains_key("getParallelism:()I"));
        assert!(fjp.methods.contains_key("getStealCount:()J"));
        assert!(fjp.methods.contains_key("isQuiescent:()Z"));
        assert!(fjp
            .methods
            .contains_key("awaitQuiescence:(JLjava/util/concurrent/TimeUnit;)Z"));

        // ForkJoinTask
        assert!(registry.is_stub_class("java/util/concurrent/ForkJoinTask"));
        let fjt = registry
            .get_class("java/util/concurrent/ForkJoinTask")
            .unwrap();
        assert_eq!(fjt.superclass, Some("java/lang/Object".to_string()));
        assert!(fjt
            .interfaces
            .contains(&"java/util/concurrent/Future".to_string()));
        assert!(fjt.interfaces.contains(&"java/io/Serializable".to_string()));
        assert!(fjt
            .methods
            .contains_key("fork:()Ljava/util/concurrent/ForkJoinTask;"));
        assert!(fjt.methods.contains_key("join:()Ljava/lang/Object;"));
        assert!(fjt.methods.contains_key("invoke:()Ljava/lang/Object;"));
        assert!(fjt.methods.contains_key(
            "invokeAll:(Ljava/util/concurrent/ForkJoinTask;Ljava/util/concurrent/ForkJoinTask;)V"
        ));
        assert!(fjt.methods.contains_key("isDone:()Z"));
        assert!(fjt.methods.contains_key("isCancelled:()Z"));
        assert!(fjt.methods.contains_key("quietlyJoin:()V"));
        assert!(fjt.methods.contains_key("reinitialize:()V"));
        assert!(fjt.methods.contains_key(
            "adapt:(Ljava/util/concurrent/Callable;)Ljava/util/concurrent/ForkJoinTask;"
        ));

        // ForkJoinWorkerThread
        assert!(registry.is_stub_class("java/util/concurrent/ForkJoinWorkerThread"));
        let fjwt = registry
            .get_class("java/util/concurrent/ForkJoinWorkerThread")
            .unwrap();
        assert_eq!(fjwt.superclass, Some("java/lang/Thread".to_string()));
        assert!(fjwt
            .methods
            .contains_key("<init>:(Ljava/util/concurrent/ForkJoinPool;)V"));
        assert!(fjwt
            .methods
            .contains_key("getPool:()Ljava/util/concurrent/ForkJoinPool;"));
        assert!(fjwt.methods.contains_key("getPoolIndex:()I"));

        // ForkJoinPool$ForkJoinWorkerThreadFactory
        assert!(
            registry.is_stub_class("java/util/concurrent/ForkJoinPool$ForkJoinWorkerThreadFactory")
        );
        let factory = registry
            .get_class("java/util/concurrent/ForkJoinPool$ForkJoinWorkerThreadFactory")
            .unwrap();
        assert!(factory.methods.contains_key(
            "newThread:(Ljava/util/concurrent/ForkJoinPool;)Ljava/util/concurrent/ForkJoinWorkerThread;"
        ));
    }

    #[test]
    fn test_scheduled_executor_classes() {
        let registry = JdkStubRegistry::new();

        // ScheduledExecutorService
        assert!(registry.is_stub_class("java/util/concurrent/ScheduledExecutorService"));
        let ses = registry
            .get_class("java/util/concurrent/ScheduledExecutorService")
            .unwrap();
        assert!(ses.superclass.is_none()); // interface
        assert!(ses
            .interfaces
            .contains(&"java/util/concurrent/ExecutorService".to_string()));
        assert!(ses.methods.contains_key(
            "schedule:(Ljava/lang/Runnable;JLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;"
        ));
        assert!(ses.methods.contains_key(
            "scheduleAtFixedRate:(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;"
        ));
        assert!(ses.methods.contains_key(
            "scheduleWithFixedDelay:(Ljava/lang/Runnable;JJLjava/util/concurrent/TimeUnit;)Ljava/util/concurrent/ScheduledFuture;"
        ));

        // ScheduledFuture
        assert!(registry.is_stub_class("java/util/concurrent/ScheduledFuture"));
        let sf = registry
            .get_class("java/util/concurrent/ScheduledFuture")
            .unwrap();
        assert!(sf.superclass.is_none()); // interface
        assert!(sf
            .interfaces
            .contains(&"java/util/concurrent/Delayed".to_string()));
        assert!(sf
            .interfaces
            .contains(&"java/util/concurrent/Future".to_string()));

        // Delayed
        assert!(registry.is_stub_class("java/util/concurrent/Delayed"));
        let delayed = registry.get_class("java/util/concurrent/Delayed").unwrap();
        assert!(delayed.superclass.is_none()); // interface
        assert!(delayed
            .interfaces
            .contains(&"java/lang/Comparable".to_string()));
        assert!(delayed
            .methods
            .contains_key("getDelay:(Ljava/util/concurrent/TimeUnit;)J"));
    }

    #[test]
    fn test_java_math_comprehensive_stubs() {
        let registry = JdkStubRegistry::new();

        // BigInteger - should have ~60+ methods
        assert!(registry.is_stub_class("java/math/BigInteger"));
        let big_integer = registry.get_class("java/math/BigInteger").unwrap();
        assert!(
            big_integer.methods.len() > 50,
            "BigInteger has {} methods, expected >50",
            big_integer.methods.len()
        );
        assert_eq!(big_integer.superclass, Some("java/lang/Number".to_string()));
        assert!(big_integer
            .interfaces
            .contains(&"java/lang/Comparable".to_string()));

        // Check critical methods
        assert!(big_integer
            .methods
            .contains_key("add:(Ljava/math/BigInteger;)Ljava/math/BigInteger;"));
        assert!(big_integer.methods.contains_key(
            "modPow:(Ljava/math/BigInteger;Ljava/math/BigInteger;)Ljava/math/BigInteger;"
        ));
        assert!(big_integer.methods.contains_key("isProbablePrime:(I)Z"));
        assert!(big_integer
            .methods
            .contains_key("sqrt:()Ljava/math/BigInteger;"));
        assert!(big_integer.methods.contains_key("bitCount:()I"));

        // BigDecimal - should have ~80+ methods
        assert!(registry.is_stub_class("java/math/BigDecimal"));
        let big_decimal = registry.get_class("java/math/BigDecimal").unwrap();
        assert!(
            big_decimal.methods.len() > 70,
            "BigDecimal has {} methods, expected >70",
            big_decimal.methods.len()
        );
        assert_eq!(big_decimal.superclass, Some("java/lang/Number".to_string()));
        assert!(big_decimal
            .interfaces
            .contains(&"java/lang/Comparable".to_string()));

        // Check critical methods
        assert!(big_decimal.methods.contains_key(
            "divide:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;"
        ));
        assert!(big_decimal
            .methods
            .contains_key("setScale:(ILjava/math/RoundingMode;)Ljava/math/BigDecimal;"));
        assert!(big_decimal
            .methods
            .contains_key("sqrt:(Ljava/math/MathContext;)Ljava/math/BigDecimal;"));
        assert!(big_decimal
            .methods
            .contains_key("toEngineeringString:()Ljava/lang/String;"));

        // MathContext - should have 8 methods
        assert!(registry.is_stub_class("java/math/MathContext"));
        let math_context = registry.get_class("java/math/MathContext").unwrap();
        assert!(
            math_context.methods.len() >= 8,
            "MathContext has {} methods, expected >=8",
            math_context.methods.len()
        );
        assert_eq!(
            math_context.superclass,
            Some("java/lang/Object".to_string())
        );
        assert!(math_context
            .interfaces
            .contains(&"java/io/Serializable".to_string()));
        assert!(math_context
            .methods
            .contains_key("getRoundingMode:()Ljava/math/RoundingMode;"));

        // RoundingMode - should have 5 methods
        assert!(registry.is_stub_class("java/math/RoundingMode"));
        let rounding_mode = registry.get_class("java/math/RoundingMode").unwrap();
        assert_eq!(
            rounding_mode.methods.len(),
            5,
            "RoundingMode has {} methods, expected 5",
            rounding_mode.methods.len()
        );
        assert_eq!(rounding_mode.superclass, Some("java/lang/Enum".to_string()));
        assert!(rounding_mode
            .methods
            .contains_key("valueOf:(I)Ljava/math/RoundingMode;"));
        assert!(rounding_mode
            .methods
            .contains_key("values:()[Ljava/math/RoundingMode;"));
    }

    #[test]
    fn test_condition_and_factory_interfaces() {
        let registry = JdkStubRegistry::new();

        // Condition
        assert!(registry.is_stub_class("java/util/concurrent/locks/Condition"));
        let condition = registry
            .get_class("java/util/concurrent/locks/Condition")
            .unwrap();
        assert!(condition.superclass.is_none()); // interface
        assert!(condition.methods.contains_key("await:()V"));
        assert!(condition.methods.contains_key("awaitUninterruptibly:()V"));
        assert!(condition.methods.contains_key("awaitNanos:(J)J"));
        assert!(condition
            .methods
            .contains_key("await:(JLjava/util/concurrent/TimeUnit;)Z"));
        assert!(condition
            .methods
            .contains_key("awaitUntil:(Ljava/util/Date;)Z"));
        assert!(condition.methods.contains_key("signal:()V"));
        assert!(condition.methods.contains_key("signalAll:()V"));

        // ThreadFactory
        assert!(registry.is_stub_class("java/util/concurrent/ThreadFactory"));
        let tf = registry
            .get_class("java/util/concurrent/ThreadFactory")
            .unwrap();
        assert!(tf.superclass.is_none()); // interface
        assert!(tf
            .methods
            .contains_key("newThread:(Ljava/lang/Runnable;)Ljava/lang/Thread;"));

        // RejectedExecutionHandler
        assert!(registry.is_stub_class("java/util/concurrent/RejectedExecutionHandler"));
        let reh = registry
            .get_class("java/util/concurrent/RejectedExecutionHandler")
            .unwrap();
        assert!(reh.superclass.is_none()); // interface
        assert!(reh.methods.contains_key(
            "rejectedExecution:(Ljava/lang/Runnable;Ljava/util/concurrent/ThreadPoolExecutor;)V"
        ));
    }

    #[test]
    fn test_groovy_lang_comprehensive_stubs() {
        let registry = JdkStubRegistry::new();

        // GroovyObject - THE FOUNDATION
        assert!(registry.is_stub_class("groovy/lang/GroovyObject"));
        let groovy_obj = registry.get_class("groovy/lang/GroovyObject").unwrap();
        assert!(groovy_obj.superclass.is_none()); // interface
        assert!(groovy_obj
            .methods
            .contains_key("invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(groovy_obj
            .methods
            .contains_key("getProperty:(Ljava/lang/String;)Ljava/lang/Object;"));
        assert!(groovy_obj
            .methods
            .contains_key("getMetaClass:()Lgroovy/lang/MetaClass;"));

        // GroovyObjectSupport
        assert!(registry.is_stub_class("groovy/lang/GroovyObjectSupport"));
        let gos = registry
            .get_class("groovy/lang/GroovyObjectSupport")
            .unwrap();
        assert_eq!(gos.superclass, Some("java/lang/Object".to_string()));
        assert!(gos
            .interfaces
            .contains(&"groovy/lang/GroovyObject".to_string()));

        // MetaClass - THE DSL ENGINE
        assert!(registry.is_stub_class("groovy/lang/MetaClass"));
        let meta_class = registry.get_class("groovy/lang/MetaClass").unwrap();
        assert!(meta_class.superclass.is_none()); // interface
        assert!(meta_class
            .interfaces
            .contains(&"groovy/lang/MetaObjectProtocol".to_string()));
        assert!(meta_class.methods.contains_key(
            "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;"
        ));
        assert!(meta_class
            .methods
            .contains_key("invokeConstructor:([Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(meta_class
            .methods
            .contains_key("getTheClass:()Ljava/lang/Class;"));

        // MetaMethod
        assert!(registry.is_stub_class("groovy/lang/MetaMethod"));
        let meta_method = registry.get_class("groovy/lang/MetaMethod").unwrap();
        assert!(meta_method
            .methods
            .contains_key("invoke:(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(meta_method.methods.contains_key("isPublic:()Z"));
        assert!(meta_method.methods.contains_key("isStatic:()Z"));

        // MetaProperty
        assert!(registry.is_stub_class("groovy/lang/MetaProperty"));
        let meta_prop = registry.get_class("groovy/lang/MetaProperty").unwrap();
        assert!(meta_prop
            .methods
            .contains_key("getProperty:(Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(meta_prop
            .methods
            .contains_key("getName:()Ljava/lang/String;"));

        // ExpandoMetaClass - Dynamic DSL extension
        assert!(registry.is_stub_class("groovy/lang/ExpandoMetaClass"));
        let emc = registry.get_class("groovy/lang/ExpandoMetaClass").unwrap();
        assert_eq!(
            emc.superclass,
            Some("groovy/lang/MetaClassImpl".to_string())
        );
        assert!(emc
            .interfaces
            .contains(&"groovy/lang/GroovyObject".to_string()));
        assert!(emc
            .methods
            .contains_key("define:(Lgroovy/lang/Closure;)Lgroovy/lang/ExpandoMetaClass;"));
        assert!(emc.methods.contains_key("enableGlobally:()V"));

        // Script - BASE CLASS FOR .gradle FILES
        assert!(registry.is_stub_class("groovy/lang/Script"));
        let script = registry.get_class("groovy/lang/Script").unwrap();
        assert_eq!(
            script.superclass,
            Some("groovy/lang/GroovyObjectSupport".to_string())
        );
        assert!(script.methods.contains_key("run:()Ljava/lang/Object;"));
        assert!(script
            .methods
            .contains_key("getBinding:()Lgroovy/lang/Binding;"));
        assert!(script.methods.contains_key("println:(Ljava/lang/Object;)V"));
        assert!(script
            .methods
            .contains_key("evaluate:(Ljava/lang/String;)Ljava/lang/Object;"));

        // Binding - SCRIPT VARIABLE SCOPE
        assert!(registry.is_stub_class("groovy/lang/Binding"));
        let binding = registry.get_class("groovy/lang/Binding").unwrap();
        assert!(binding
            .methods
            .contains_key("getVariable:(Ljava/lang/String;)Ljava/lang/Object;"));
        assert!(binding
            .methods
            .contains_key("setVariable:(Ljava/lang/String;Ljava/lang/Object;)V"));
        assert!(binding
            .methods
            .contains_key("hasVariable:(Ljava/lang/String;)Z"));

        // Closure - THE HEART OF GRADLE DSL
        assert!(registry.is_stub_class("groovy/lang/Closure"));
        let closure = registry.get_class("groovy/lang/Closure").unwrap();
        assert!(closure
            .interfaces
            .contains(&"java/lang/Runnable".to_string()));
        assert!(closure
            .interfaces
            .contains(&"java/util/concurrent/Callable".to_string()));
        assert!(
            closure.methods.len() > 30,
            "Closure should have 30+ methods, has {}",
            closure.methods.len()
        );
        assert!(closure.methods.contains_key("call:()Ljava/lang/Object;"));
        assert!(closure
            .methods
            .contains_key("call:([Ljava/lang/Object;)Ljava/lang/Object;"));
        assert!(closure.methods.contains_key("doCall:()Ljava/lang/Object;"));
        assert!(closure
            .methods
            .contains_key("getDelegate:()Ljava/lang/Object;"));
        assert!(closure
            .methods
            .contains_key("setDelegate:(Ljava/lang/Object;)V"));
        assert!(closure.methods.contains_key("getResolveStrategy:()I"));
        assert!(closure
            .methods
            .contains_key("curry:(Ljava/lang/Object;)Lgroovy/lang/Closure;"));
        assert!(closure
            .methods
            .contains_key("memoize:()Lgroovy/lang/Closure;"));
        assert!(closure.methods.contains_key("rehydrate:(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lgroovy/lang/Closure;"));

        // GString
        assert!(registry.is_stub_class("groovy/lang/GString"));
        let gstring = registry.get_class("groovy/lang/GString").unwrap();
        assert!(gstring
            .interfaces
            .contains(&"java/lang/CharSequence".to_string()));
        assert!(gstring
            .interfaces
            .contains(&"groovy/lang/Writable".to_string()));
        assert!(gstring
            .methods
            .contains_key("getStrings:()[Ljava/lang/String;"));
        assert!(gstring
            .methods
            .contains_key("getValues:()[Ljava/lang/Object;"));

        // Range interface
        assert!(registry.is_stub_class("groovy/lang/Range"));
        let range = registry.get_class("groovy/lang/Range").unwrap();
        assert!(range.interfaces.contains(&"java/util/List".to_string()));
        assert!(range
            .methods
            .contains_key("getFrom:()Ljava/lang/Comparable;"));
        assert!(range.methods.contains_key("isReverse:()Z"));
        assert!(range.methods.contains_key("step:(ILgroovy/lang/Closure;)V"));

        // IntRange
        assert!(registry.is_stub_class("groovy/lang/IntRange"));
        let int_range = registry.get_class("groovy/lang/IntRange").unwrap();
        assert!(int_range
            .interfaces
            .contains(&"groovy/lang/Range".to_string()));
        assert!(int_range.methods.contains_key("getFromInt:()I"));

        // Exception classes
        assert!(registry.is_stub_class("groovy/lang/GroovyRuntimeException"));
        assert!(registry.is_stub_class("groovy/lang/MissingMethodException"));
        assert!(registry.is_stub_class("groovy/lang/MissingPropertyException"));

        // Utility classes
        assert!(registry.is_stub_class("groovy/lang/Tuple"));
        assert!(registry.is_stub_class("groovy/lang/Tuple2"));
        assert!(registry.is_stub_class("groovy/lang/Reference"));
        assert!(registry.is_stub_class("groovy/lang/SpreadMap"));

        // Shell and ClassLoader
        assert!(registry.is_stub_class("groovy/lang/GroovyShell"));
        let shell = registry.get_class("groovy/lang/GroovyShell").unwrap();
        assert!(shell
            .methods
            .contains_key("evaluate:(Ljava/lang/String;)Ljava/lang/Object;"));
        assert!(shell
            .methods
            .contains_key("parse:(Ljava/lang/String;)Lgroovy/lang/Script;"));

        assert!(registry.is_stub_class("groovy/lang/GroovyClassLoader"));
        let gcl = registry.get_class("groovy/lang/GroovyClassLoader").unwrap();
        assert!(gcl
            .methods
            .contains_key("parseClass:(Ljava/lang/String;)Ljava/lang/Class;"));
        assert!(gcl
            .methods
            .contains_key("addClasspath:(Ljava/lang/String;)V"));
    }

    #[test]
    fn test_java_nio_filesystem_classes() {
        let registry = JdkStubRegistry::new();

        // FileSystem abstract class
        assert!(registry.is_stub_class("java/nio/file/FileSystem"));
        let fs = registry.get_class("java/nio/file/FileSystem").unwrap();
        assert_eq!(fs.superclass, Some("java/lang/Object".to_string()));
        assert!(fs.interfaces.contains(&"java/io/Closeable".to_string()));
        assert!(fs
            .methods
            .contains_key("provider:()Ljava/nio/file/spi/FileSystemProvider;"));
        assert!(fs.methods.contains_key("getSeparator:()Ljava/lang/String;"));
        assert!(fs
            .methods
            .contains_key("getPath:(Ljava/lang/String;[Ljava/lang/String;)Ljava/nio/file/Path;"));
        assert!(fs
            .methods
            .contains_key("newWatchService:()Ljava/nio/file/WatchService;"));

        // FileSystems utility class
        assert!(registry.is_stub_class("java/nio/file/FileSystems"));
        let fss = registry.get_class("java/nio/file/FileSystems").unwrap();
        assert!(fss
            .methods
            .contains_key("getDefault:()Ljava/nio/file/FileSystem;"));
        assert!(fss.methods.contains_key(
            "newFileSystem:(Ljava/net/URI;Ljava/util/Map;)Ljava/nio/file/FileSystem;"
        ));

        // FileSystemProvider abstract class
        assert!(registry.is_stub_class("java/nio/file/spi/FileSystemProvider"));
        let fsp = registry
            .get_class("java/nio/file/spi/FileSystemProvider")
            .unwrap();
        assert!(fsp
            .methods
            .contains_key("installedProviders:()Ljava/util/List;"));
        assert!(fsp.methods.contains_key("getScheme:()Ljava/lang/String;"));
        assert!(fsp.methods.contains_key("delete:(Ljava/nio/file/Path;)V"));

        // FileStore abstract class
        assert!(registry.is_stub_class("java/nio/file/FileStore"));
        let store = registry.get_class("java/nio/file/FileStore").unwrap();
        assert!(store.methods.contains_key("name:()Ljava/lang/String;"));
        assert!(store.methods.contains_key("getTotalSpace:()J"));
        assert!(store.methods.contains_key("getBlockSize:()J"));

        // WatchService interface
        assert!(registry.is_stub_class("java/nio/file/WatchService"));
        let ws = registry.get_class("java/nio/file/WatchService").unwrap();
        assert!(ws.superclass.is_none()); // interface
        assert!(ws.interfaces.contains(&"java/io/Closeable".to_string()));
        assert!(ws.methods.contains_key("poll:()Ljava/nio/file/WatchKey;"));
        assert!(ws.methods.contains_key("take:()Ljava/nio/file/WatchKey;"));

        // WatchKey interface
        assert!(registry.is_stub_class("java/nio/file/WatchKey"));
        let wk = registry.get_class("java/nio/file/WatchKey").unwrap();
        assert!(wk.superclass.is_none()); // interface
        assert!(wk.methods.contains_key("isValid:()Z"));
        assert!(wk.methods.contains_key("pollEvents:()Ljava/util/List;"));
        assert!(wk.methods.contains_key("reset:()Z"));
        assert!(wk
            .methods
            .contains_key("watchable:()Ljava/nio/file/Watchable;"));

        // WatchEvent interface
        assert!(registry.is_stub_class("java/nio/file/WatchEvent"));
        let we = registry.get_class("java/nio/file/WatchEvent").unwrap();
        assert!(we
            .methods
            .contains_key("kind:()Ljava/nio/file/WatchEvent$Kind;"));
        assert!(we.methods.contains_key("count:()I"));
        assert!(we.methods.contains_key("context:()Ljava/lang/Object;"));

        // WatchEvent$Kind interface
        assert!(registry.is_stub_class("java/nio/file/WatchEvent$Kind"));
        let wek = registry.get_class("java/nio/file/WatchEvent$Kind").unwrap();
        assert!(wek.methods.contains_key("name:()Ljava/lang/String;"));
        assert!(wek.methods.contains_key("type:()Ljava/lang/Class;"));

        // Watchable interface
        assert!(registry.is_stub_class("java/nio/file/Watchable"));
        let watchable = registry.get_class("java/nio/file/Watchable").unwrap();
        assert!(watchable.methods.contains_key(
            "register:(Ljava/nio/file/WatchService;[Ljava/nio/file/WatchEvent$Kind;)Ljava/nio/file/WatchKey;"
        ));

        // StandardWatchEventKinds
        assert!(registry.is_stub_class("java/nio/file/StandardWatchEventKinds"));
    }

    #[test]
    fn test_java_nio_channel_interfaces() {
        let registry = JdkStubRegistry::new();

        // Channel interface
        assert!(registry.is_stub_class("java/nio/channels/Channel"));
        let ch = registry.get_class("java/nio/channels/Channel").unwrap();
        assert!(ch.superclass.is_none()); // interface
        assert!(ch.interfaces.contains(&"java/io/Closeable".to_string()));
        assert!(ch.methods.contains_key("isOpen:()Z"));
        assert!(ch.methods.contains_key("close:()V"));

        // ReadableByteChannel interface
        assert!(registry.is_stub_class("java/nio/channels/ReadableByteChannel"));
        let rbc = registry
            .get_class("java/nio/channels/ReadableByteChannel")
            .unwrap();
        assert!(rbc
            .interfaces
            .contains(&"java/nio/channels/Channel".to_string()));
        assert!(rbc.methods.contains_key("read:(Ljava/nio/ByteBuffer;)I"));

        // WritableByteChannel interface
        assert!(registry.is_stub_class("java/nio/channels/WritableByteChannel"));
        let wbc = registry
            .get_class("java/nio/channels/WritableByteChannel")
            .unwrap();
        assert!(wbc.methods.contains_key("write:(Ljava/nio/ByteBuffer;)I"));

        // ByteChannel interface
        assert!(registry.is_stub_class("java/nio/channels/ByteChannel"));
        let bc = registry.get_class("java/nio/channels/ByteChannel").unwrap();
        assert!(bc
            .interfaces
            .contains(&"java/nio/channels/ReadableByteChannel".to_string()));
        assert!(bc
            .interfaces
            .contains(&"java/nio/channels/WritableByteChannel".to_string()));

        // SeekableByteChannel interface
        assert!(registry.is_stub_class("java/nio/channels/SeekableByteChannel"));
        let sbc = registry
            .get_class("java/nio/channels/SeekableByteChannel")
            .unwrap();
        assert!(sbc
            .interfaces
            .contains(&"java/nio/channels/ByteChannel".to_string()));
        assert!(sbc.methods.contains_key("position:()J"));
        assert!(sbc
            .methods
            .contains_key("position:(J)Ljava/nio/channels/SeekableByteChannel;"));
        assert!(sbc.methods.contains_key("size:()J"));
        assert!(sbc
            .methods
            .contains_key("truncate:(J)Ljava/nio/channels/SeekableByteChannel;"));

        // AsynchronousByteChannel interface
        assert!(registry.is_stub_class("java/nio/channels/AsynchronousByteChannel"));
        let abc = registry
            .get_class("java/nio/channels/AsynchronousByteChannel")
            .unwrap();
        assert!(abc
            .interfaces
            .contains(&"java/nio/channels/AsynchronousChannel".to_string()));
        assert!(abc
            .methods
            .contains_key("read:(Ljava/nio/ByteBuffer;)Ljava/util/concurrent/Future;"));

        // CompletionHandler interface
        assert!(registry.is_stub_class("java/nio/channels/CompletionHandler"));
        let handler = registry
            .get_class("java/nio/channels/CompletionHandler")
            .unwrap();
        assert!(handler
            .methods
            .contains_key("completed:(Ljava/lang/Object;Ljava/lang/Object;)V"));
        assert!(handler
            .methods
            .contains_key("failed:(Ljava/lang/Throwable;Ljava/lang/Object;)V"));

        // Channels utility class
        assert!(registry.is_stub_class("java/nio/channels/Channels"));
        let channels = registry.get_class("java/nio/channels/Channels").unwrap();
        assert!(channels.methods.contains_key(
            "newInputStream:(Ljava/nio/channels/ReadableByteChannel;)Ljava/io/InputStream;"
        ));
        assert!(channels.methods.contains_key(
            "newChannel:(Ljava/io/InputStream;)Ljava/nio/channels/ReadableByteChannel;"
        ));
        assert!(channels.methods.contains_key(
            "newReader:(Ljava/nio/channels/ReadableByteChannel;Ljava/nio/charset/Charset;)Ljava/io/Reader;"
        ));
    }

    #[test]
    fn test_java_nio_file_options() {
        let registry = JdkStubRegistry::new();

        // DirectoryStream interface
        assert!(registry.is_stub_class("java/nio/file/DirectoryStream"));
        let ds = registry.get_class("java/nio/file/DirectoryStream").unwrap();
        assert!(ds.interfaces.contains(&"java/io/Closeable".to_string()));
        assert!(ds.interfaces.contains(&"java/lang/Iterable".to_string()));

        // DirectoryStream$Filter interface
        assert!(registry.is_stub_class("java/nio/file/DirectoryStream$Filter"));
        let dsf = registry
            .get_class("java/nio/file/DirectoryStream$Filter")
            .unwrap();
        assert!(dsf.methods.contains_key("accept:(Ljava/lang/Object;)Z"));

        // PathMatcher interface
        assert!(registry.is_stub_class("java/nio/file/PathMatcher"));
        let pm = registry.get_class("java/nio/file/PathMatcher").unwrap();
        assert!(pm.methods.contains_key("matches:(Ljava/nio/file/Path;)Z"));

        // AccessMode enum
        assert!(registry.is_stub_class("java/nio/file/AccessMode"));
        let am = registry.get_class("java/nio/file/AccessMode").unwrap();
        assert_eq!(am.superclass, Some("java/lang/Enum".to_string()));

        // LinkOption enum
        assert!(registry.is_stub_class("java/nio/file/LinkOption"));
        let lo = registry.get_class("java/nio/file/LinkOption").unwrap();
        assert_eq!(lo.superclass, Some("java/lang/Enum".to_string()));
        assert!(lo
            .interfaces
            .contains(&"java/nio/file/OpenOption".to_string()));
        assert!(lo
            .interfaces
            .contains(&"java/nio/file/CopyOption".to_string()));

        // CopyOption interface
        assert!(registry.is_stub_class("java/nio/file/CopyOption"));

        // OpenOption interface
        assert!(registry.is_stub_class("java/nio/file/OpenOption"));
    }
}
