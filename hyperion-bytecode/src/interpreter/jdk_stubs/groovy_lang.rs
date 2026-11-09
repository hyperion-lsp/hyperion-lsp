// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! groovy.lang JDK stub classes
//!
//! Comprehensive stubs for Groovy language runtime support.
//! These classes are CRITICAL for Gradle DSL execution:
//! - GroovyObject: Base interface for dynamic method dispatch
//! - MetaClass: The engine for Groovy's dynamic method resolution
//! - Closure: Heart of Gradle DSL (configuration blocks)
//! - Script: Base class for .gradle files
//! - Binding: Script variable scope management

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

// =============================================================================
// CORE INTERFACES
// =============================================================================

/// Create groovy.lang.GroovyObject interface
/// THE FOUNDATION - Every Gradle object extends this
pub fn create_groovy_object_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Core dynamic dispatch methods
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );

    StubClass {
        name: "groovy/lang/GroovyObject".to_string(),
        superclass: None, // interface
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyObjectSupport abstract class
/// Base implementation of GroovyObject
pub fn create_groovy_object_support_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );

    // GroovyObject implementation
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );

    StubClass {
        name: "groovy/lang/GroovyObjectSupport".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["groovy/lang/GroovyObject".to_string()],
        methods,
    }
}

// =============================================================================
// METACLASS SYSTEM - THE DSL ENGINE
// =============================================================================

/// Create groovy.lang.MetaObjectProtocol interface
pub fn create_meta_object_protocol_interface() -> StubClass {
    let mut methods = HashMap::new();

    // respondsTo methods
    methods.insert(
        "respondsTo:(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "respondsTo",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "respondsTo:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/util/List;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "respondsTo",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/util/List;",
        ),
    );

    // hasProperty
    methods.insert(
        "hasProperty:(Ljava/lang/Object;Ljava/lang/String;)Lgroovy/lang/MetaProperty;".to_string(),
        StubMethodDescriptor::instance_method(
            "hasProperty",
            "(Ljava/lang/Object;Ljava/lang/String;)Lgroovy/lang/MetaProperty;",
        ),
    );

    // Property access (full signature)
    methods.insert(
        "getProperty:(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;ZZ)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;ZZ)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;ZZ)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;ZZ)V",
        ),
    );

    // Attribute access
    methods.insert(
        "getAttribute:(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Z)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getAttribute",
            "(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Z)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setAttribute:(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;ZZ)V"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "setAttribute",
            "(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;ZZ)V",
        ),
    );

    // Method invocation
    methods.insert(
        "invokeMethod:(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;ZZ)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Class;Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;ZZ)Ljava/lang/Object;",
        ),
    );

    // getTheClass
    methods.insert(
        "getTheClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getTheClass", "()Ljava/lang/Class;"),
    );

    StubClass {
        name: "groovy/lang/MetaObjectProtocol".to_string(),
        superclass: None, // interface
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.MetaClass interface - THE DSL ENGINE
pub fn create_meta_class_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Method invocation - the core dispatch mechanism
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeStaticMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeStaticMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeConstructor:([Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeConstructor",
            "([Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );

    // Property access
    methods.insert(
        "getProperty:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );

    // Attribute access
    methods.insert(
        "getAttribute:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAttribute",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setAttribute:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setAttribute",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );

    // Meta method introspection
    methods.insert(
        "getMetaMethod:(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMetaMethod",
            "(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;",
        ),
    );
    methods.insert(
        "getStaticMetaMethod:(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "getStaticMetaMethod",
            "(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;",
        ),
    );
    methods.insert(
        "getMetaMethods:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getMetaMethods", "()Ljava/util/List;"),
    );
    methods.insert(
        "getMethods:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getMethods", "()Ljava/util/List;"),
    );

    // Property introspection
    methods.insert(
        "getProperties:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getProperties", "()Ljava/util/List;"),
    );
    methods.insert(
        "getMetaProperty:(Ljava/lang/String;)Lgroovy/lang/MetaProperty;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMetaProperty",
            "(Ljava/lang/String;)Lgroovy/lang/MetaProperty;",
        ),
    );
    methods.insert(
        "hasProperty:(Ljava/lang/Object;Ljava/lang/String;)Lgroovy/lang/MetaProperty;".to_string(),
        StubMethodDescriptor::instance_method(
            "hasProperty",
            "(Ljava/lang/Object;Ljava/lang/String;)Lgroovy/lang/MetaProperty;",
        ),
    );

    // Class info
    methods.insert(
        "getTheClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getTheClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "initialize:()V".to_string(),
        StubMethodDescriptor::instance_method("initialize", "()V"),
    );

    // respondsTo (from MetaObjectProtocol)
    methods.insert(
        "respondsTo:(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "respondsTo",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;",
        ),
    );
    methods.insert(
        "respondsTo:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/util/List;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "respondsTo",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/util/List;",
        ),
    );

    // Missing method/property handling (for DSL support)
    methods.insert(
        "invokeMissingMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMissingMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeMissingProperty:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;Z)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMissingProperty",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;Z)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "groovy/lang/MetaClass".to_string(),
        superclass: None, // interface
        interfaces: vec!["groovy/lang/MetaObjectProtocol".to_string()],
        methods,
    }
}

/// Create groovy.lang.MetaMethod abstract class
pub fn create_meta_method_class() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract invoke method
    methods.insert(
        "invoke:(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invoke",
            "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );

    // Abstract getters
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getReturnType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getReturnType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getDeclaringClass:()Lorg/codehaus/groovy/reflection/CachedClass;".to_string(),
        StubMethodDescriptor::instance_method(
            "getDeclaringClass",
            "()Lorg/codehaus/groovy/reflection/CachedClass;",
        ),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );
    methods.insert(
        "getNativeParameterTypes:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getNativeParameterTypes", "()[Ljava/lang/Class;"),
    );
    methods.insert(
        "getParameterTypes:()[Lorg/codehaus/groovy/reflection/CachedClass;".to_string(),
        StubMethodDescriptor::instance_method(
            "getParameterTypes",
            "()[Lorg/codehaus/groovy/reflection/CachedClass;",
        ),
    );

    // Modifier check methods
    methods.insert(
        "isPrivate:()Z".to_string(),
        StubMethodDescriptor::instance_method("isPrivate", "()Z"),
    );
    methods.insert(
        "isProtected:()Z".to_string(),
        StubMethodDescriptor::instance_method("isProtected", "()Z"),
    );
    methods.insert(
        "isPublic:()Z".to_string(),
        StubMethodDescriptor::instance_method("isPublic", "()Z"),
    );
    methods.insert(
        "isStatic:()Z".to_string(),
        StubMethodDescriptor::instance_method("isStatic", "()Z"),
    );
    methods.insert(
        "isAbstract:()Z".to_string(),
        StubMethodDescriptor::instance_method("isAbstract", "()Z"),
    );
    methods.insert(
        "isCacheable:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCacheable", "()Z"),
    );

    // Validation
    methods.insert(
        "isValidMethod:([Ljava/lang/Class;)Z".to_string(),
        StubMethodDescriptor::instance_method("isValidMethod", "([Ljava/lang/Class;)Z"),
    );

    // Descriptor/Signature
    methods.insert(
        "getDescriptor:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getDescriptor", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getSignature:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getSignature", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getMopName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMopName", "()Ljava/lang/String;"),
    );

    // Comparable
    methods.insert(
        "compareTo:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Object;)I"),
    );

    StubClass {
        name: "groovy/lang/MetaMethod".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create groovy.lang.MetaProperty abstract class
pub fn create_meta_property_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/Class;)V"),
    );

    // Abstract property access
    methods.insert(
        "getProperty:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/Object;Ljava/lang/Object;)V",
        ),
    );

    // Getters
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getModifiers:()I".to_string(),
        StubMethodDescriptor::instance_method("getModifiers", "()I"),
    );

    // Comparable
    methods.insert(
        "compareTo:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Object;)I"),
    );

    StubClass {
        name: "groovy/lang/MetaProperty".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create groovy.lang.MetaBeanProperty class
pub fn create_meta_bean_property_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;Lgroovy/lang/MetaMethod;Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/String;Ljava/lang/Class;Lgroovy/lang/MetaMethod;Lgroovy/lang/MetaMethod;)V",
        ),
    );

    // Inherited from MetaProperty
    methods.insert(
        "getProperty:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/Object;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getType", "()Ljava/lang/Class;"),
    );

    // Bean property specific
    methods.insert(
        "getGetter:()Lgroovy/lang/MetaMethod;".to_string(),
        StubMethodDescriptor::instance_method("getGetter", "()Lgroovy/lang/MetaMethod;"),
    );
    methods.insert(
        "getSetter:()Lgroovy/lang/MetaMethod;".to_string(),
        StubMethodDescriptor::instance_method("getSetter", "()Lgroovy/lang/MetaMethod;"),
    );
    methods.insert(
        "setGetter:(Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::instance_method("setGetter", "(Lgroovy/lang/MetaMethod;)V"),
    );
    methods.insert(
        "setSetter:(Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::instance_method("setSetter", "(Lgroovy/lang/MetaMethod;)V"),
    );
    methods.insert(
        "getField:()Lorg/codehaus/groovy/reflection/CachedField;".to_string(),
        StubMethodDescriptor::instance_method(
            "getField",
            "()Lorg/codehaus/groovy/reflection/CachedField;",
        ),
    );
    methods.insert(
        "setField:(Lorg/codehaus/groovy/reflection/CachedField;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setField",
            "(Lorg/codehaus/groovy/reflection/CachedField;)V",
        ),
    );

    StubClass {
        name: "groovy/lang/MetaBeanProperty".to_string(),
        superclass: Some("groovy/lang/MetaProperty".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.MetaClassImpl class
/// Standard implementation of MetaClass
pub fn create_meta_class_impl_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Class;)V"),
    );
    methods.insert(
        "<init>:(Lgroovy/lang/MetaClassRegistry;Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::constructor("(Lgroovy/lang/MetaClassRegistry;Ljava/lang/Class;)V"),
    );

    // All MetaClass methods
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeStaticMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeStaticMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeConstructor:([Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeConstructor",
            "([Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getAttribute:(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getAttribute",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setAttribute:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setAttribute",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getTheClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getTheClass", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "initialize:()V".to_string(),
        StubMethodDescriptor::instance_method("initialize", "()V"),
    );
    methods.insert(
        "getMetaMethods:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getMetaMethods", "()Ljava/util/List;"),
    );
    methods.insert(
        "getMethods:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getMethods", "()Ljava/util/List;"),
    );
    methods.insert(
        "getProperties:()Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("getProperties", "()Ljava/util/List;"),
    );
    methods.insert(
        "getMetaProperty:(Ljava/lang/String;)Lgroovy/lang/MetaProperty;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMetaProperty",
            "(Ljava/lang/String;)Lgroovy/lang/MetaProperty;",
        ),
    );
    methods.insert(
        "getMetaMethod:(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMetaMethod",
            "(Ljava/lang/String;[Ljava/lang/Object;)Lgroovy/lang/MetaMethod;",
        ),
    );
    methods.insert(
        "respondsTo:(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method(
            "respondsTo",
            "(Ljava/lang/Object;Ljava/lang/String;)Ljava/util/List;",
        ),
    );

    // Implementation-specific methods
    methods.insert(
        "pickMethod:(Ljava/lang/String;[Ljava/lang/Class;)Lgroovy/lang/MetaMethod;".to_string(),
        StubMethodDescriptor::instance_method(
            "pickMethod",
            "(Ljava/lang/String;[Ljava/lang/Class;)Lgroovy/lang/MetaMethod;",
        ),
    );
    methods.insert(
        "isModified:()Z".to_string(),
        StubMethodDescriptor::instance_method("isModified", "()Z"),
    );
    methods.insert(
        "addNewInstanceMethod:(Ljava/lang/reflect/Method;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "addNewInstanceMethod",
            "(Ljava/lang/reflect/Method;)V",
        ),
    );
    methods.insert(
        "addNewStaticMethod:(Ljava/lang/reflect/Method;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "addNewStaticMethod",
            "(Ljava/lang/reflect/Method;)V",
        ),
    );

    StubClass {
        name: "groovy/lang/MetaClassImpl".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["groovy/lang/MetaClass".to_string()],
        methods,
    }
}

/// Create groovy.lang.ExpandoMetaClass - Dynamic metaclass for runtime DSL extension
pub fn create_expando_meta_class_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Class;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Class;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Class;Z)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Class;ZZ)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Class;ZZ)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Class;ZZ[Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Class;ZZ[Lgroovy/lang/MetaMethod;)V"),
    );

    // Initialization
    methods.insert(
        "initialize:()V".to_string(),
        StubMethodDescriptor::instance_method("initialize", "()V"),
    );
    methods.insert(
        "isInitialized:()Z".to_string(),
        StubMethodDescriptor::instance_method("isInitialized", "()Z"),
    );

    // Method addition
    methods.insert(
        "addMixinClass:(Lgroovy/lang/MixinInMetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("addMixinClass", "(Lgroovy/lang/MixinInMetaClass;)V"),
    );
    methods.insert(
        "addMethod:(Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::instance_method("addMethod", "(Lgroovy/lang/MetaMethod;)V"),
    );
    methods.insert(
        "addMetaBeanProperty:(Lgroovy/lang/MetaBeanProperty;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "addMetaBeanProperty",
            "(Lgroovy/lang/MetaBeanProperty;)V",
        ),
    );
    methods.insert(
        "addMetaMethod:(Lgroovy/lang/MetaMethod;)V".to_string(),
        StubMethodDescriptor::instance_method("addMetaMethod", "(Lgroovy/lang/MetaMethod;)V"),
    );
    methods.insert(
        "isModified:()Z".to_string(),
        StubMethodDescriptor::instance_method("isModified", "()Z"),
    );
    methods.insert(
        "setAllowChangesAfterInit:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("setAllowChangesAfterInit", "(Z)V"),
    );

    // Define closure for DSL
    methods.insert(
        "define:(Lgroovy/lang/Closure;)Lgroovy/lang/ExpandoMetaClass;".to_string(),
        StubMethodDescriptor::instance_method(
            "define",
            "(Lgroovy/lang/Closure;)Lgroovy/lang/ExpandoMetaClass;",
        ),
    );

    // GroovyObject implementation
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );

    // Inherited MetaClass methods
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getTheClass:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getTheClass", "()Ljava/lang/Class;"),
    );

    // Static global enable/disable
    methods.insert(
        "enableGlobally:()V".to_string(),
        StubMethodDescriptor::static_method("enableGlobally", "()V"),
    );
    methods.insert(
        "disableGlobally:()V".to_string(),
        StubMethodDescriptor::static_method("disableGlobally", "()V"),
    );

    StubClass {
        name: "groovy/lang/ExpandoMetaClass".to_string(),
        superclass: Some("groovy/lang/MetaClassImpl".to_string()),
        interfaces: vec!["groovy/lang/GroovyObject".to_string()],
        methods,
    }
}

/// Create groovy.lang.MetaClassRegistry interface
pub fn create_meta_class_registry_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getMetaClass:(Ljava/lang/Class;)Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMetaClass",
            "(Ljava/lang/Class;)Lgroovy/lang/MetaClass;",
        ),
    );
    methods.insert(
        "setMetaClass:(Ljava/lang/Class;Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setMetaClass",
            "(Ljava/lang/Class;Lgroovy/lang/MetaClass;)V",
        ),
    );
    methods.insert(
        "removeMetaClass:(Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::instance_method("removeMetaClass", "(Ljava/lang/Class;)V"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );

    StubClass {
        name: "groovy/lang/MetaClassRegistry".to_string(),
        superclass: None, // interface
        interfaces: vec![],
        methods,
    }
}

// =============================================================================
// SCRIPT AND BINDING - Gradle .gradle file support
// =============================================================================

/// Create groovy.lang.Script abstract class - BASE CLASS FOR ALL .gradle FILES
pub fn create_script_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Lgroovy/lang/Binding;)V".to_string(),
        StubMethodDescriptor::constructor("(Lgroovy/lang/Binding;)V"),
    );

    // Abstract run method - THE ENTRY POINT
    methods.insert(
        "run:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("run", "()Ljava/lang/Object;"),
    );

    // Binding management
    methods.insert(
        "getBinding:()Lgroovy/lang/Binding;".to_string(),
        StubMethodDescriptor::instance_method("getBinding", "()Lgroovy/lang/Binding;"),
    );
    methods.insert(
        "setBinding:(Lgroovy/lang/Binding;)V".to_string(),
        StubMethodDescriptor::instance_method("setBinding", "(Lgroovy/lang/Binding;)V"),
    );

    // GroovyObject implementation (inherited from GroovyObjectSupport)
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );

    // Print methods - common in scripts
    methods.insert(
        "print:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("print", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "println:()V".to_string(),
        StubMethodDescriptor::instance_method("println", "()V"),
    );
    methods.insert(
        "println:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("println", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "printf:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("printf", "(Ljava/lang/String;Ljava/lang/Object;)V"),
    );
    methods.insert(
        "printf:(Ljava/lang/String;[Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("printf", "(Ljava/lang/String;[Ljava/lang/Object;)V"),
    );

    // Evaluate methods
    methods.insert(
        "evaluate:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("evaluate", "(Ljava/lang/String;)Ljava/lang/Object;"),
    );
    methods.insert(
        "evaluate:(Ljava/io/File;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("evaluate", "(Ljava/io/File;)Ljava/lang/Object;"),
    );

    // Run external script
    methods.insert(
        "run:(Ljava/io/File;[Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("run", "(Ljava/io/File;[Ljava/lang/String;)V"),
    );

    StubClass {
        name: "groovy/lang/Script".to_string(),
        superclass: Some("groovy/lang/GroovyObjectSupport".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.DelegatingScript class
pub fn create_delegating_script_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Lgroovy/lang/Binding;)V".to_string(),
        StubMethodDescriptor::constructor("(Lgroovy/lang/Binding;)V"),
    );

    // Delegate management
    methods.insert(
        "getDelegate:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getDelegate", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "setDelegate:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("setDelegate", "(Ljava/lang/Object;)V"),
    );

    // Override GroovyObject methods to delegate
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );

    StubClass {
        name: "groovy/lang/DelegatingScript".to_string(),
        superclass: Some("groovy/lang/Script".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.Binding class - SCRIPT VARIABLE SCOPE
pub fn create_binding_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );
    methods.insert(
        "<init>:([Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("([Ljava/lang/String;)V"),
    );

    // Variable management
    methods.insert(
        "getVariable:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getVariable",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setVariable:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setVariable",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "hasVariable:(Ljava/lang/String;)Z".to_string(),
        StubMethodDescriptor::instance_method("hasVariable", "(Ljava/lang/String;)Z"),
    );
    methods.insert(
        "getVariables:()Ljava/util/Map;".to_string(),
        StubMethodDescriptor::instance_method("getVariables", "()Ljava/util/Map;"),
    );
    methods.insert(
        "setVariables:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::instance_method("setVariables", "(Ljava/util/Map;)V"),
    );

    // GroovyObject implementation
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );

    StubClass {
        name: "groovy/lang/Binding".to_string(),
        superclass: Some("groovy/lang/GroovyObjectSupport".to_string()),
        interfaces: vec![],
        methods,
    }
}

// =============================================================================
// CLOSURE - The heart of Gradle DSL
// =============================================================================

/// Create groovy.lang.Closure class - THE HEART OF GRADLE DSL
pub fn create_closure_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;Ljava/lang/Object;)V"),
    );

    // Core invocation - call variants
    methods.insert(
        "call:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("call", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "call:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("call", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "call:([Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("call", "([Ljava/lang/Object;)Ljava/lang/Object;"),
    );

    // doCall variants (actual closure body implementation)
    methods.insert(
        "doCall:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("doCall", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "doCall:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("doCall", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "doCall:([Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("doCall", "([Ljava/lang/Object;)Ljava/lang/Object;"),
    );

    // Context - owner, delegate, thisObject
    methods.insert(
        "getOwner:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getOwner", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getDelegate:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getDelegate", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "setDelegate:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("setDelegate", "(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "getThisObject:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getThisObject", "()Ljava/lang/Object;"),
    );

    // Resolution strategy - CRITICAL FOR GRADLE DSL
    methods.insert(
        "getResolveStrategy:()I".to_string(),
        StubMethodDescriptor::instance_method("getResolveStrategy", "()I"),
    );
    methods.insert(
        "setResolveStrategy:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setResolveStrategy", "(I)V"),
    );
    methods.insert(
        "getDirective:()I".to_string(),
        StubMethodDescriptor::instance_method("getDirective", "()I"),
    );
    methods.insert(
        "setDirective:(I)V".to_string(),
        StubMethodDescriptor::instance_method("setDirective", "(I)V"),
    );

    // Parameter info
    methods.insert(
        "getMaximumNumberOfParameters:()I".to_string(),
        StubMethodDescriptor::instance_method("getMaximumNumberOfParameters", "()I"),
    );
    methods.insert(
        "getParameterTypes:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getParameterTypes", "()[Ljava/lang/Class;"),
    );

    // Composition operators
    methods.insert(
        "rightShift:(Lgroovy/lang/Closure;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "rightShift",
            "(Lgroovy/lang/Closure;)Lgroovy/lang/Closure;",
        ),
    );
    methods.insert(
        "leftShift:(Lgroovy/lang/Closure;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "leftShift",
            "(Lgroovy/lang/Closure;)Lgroovy/lang/Closure;",
        ),
    );

    // Currying
    methods.insert(
        "curry:(Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("curry", "(Ljava/lang/Object;)Lgroovy/lang/Closure;"),
    );
    methods.insert(
        "curry:([Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "curry",
            "([Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );
    methods.insert(
        "rcurry:(Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "rcurry",
            "(Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );
    methods.insert(
        "rcurry:([Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "rcurry",
            "([Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );
    methods.insert(
        "ncurry:(I[Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "ncurry",
            "(I[Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );

    // Memoization
    methods.insert(
        "memoize:()Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("memoize", "()Lgroovy/lang/Closure;"),
    );
    methods.insert(
        "memoizeAtMost:(I)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("memoizeAtMost", "(I)Lgroovy/lang/Closure;"),
    );
    methods.insert(
        "memoizeAtLeast:(I)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("memoizeAtLeast", "(I)Lgroovy/lang/Closure;"),
    );
    methods.insert(
        "memoizeBetween:(II)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("memoizeBetween", "(II)Lgroovy/lang/Closure;"),
    );

    // Trampoline
    methods.insert(
        "trampoline:()Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("trampoline", "()Lgroovy/lang/Closure;"),
    );
    methods.insert(
        "trampoline:([Ljava/lang/Object;)Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method(
            "trampoline",
            "([Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );

    // Delegation helpers
    methods.insert(
        "rehydrate:(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lgroovy/lang/Closure;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "rehydrate",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)Lgroovy/lang/Closure;",
        ),
    );
    methods.insert(
        "dehydrate:()Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::instance_method("dehydrate", "()Lgroovy/lang/Closure;"),
    );

    // Cloneable/Runnable/Callable
    methods.insert(
        "clone:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("clone", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "run:()V".to_string(),
        StubMethodDescriptor::instance_method("run", "()V"),
    );

    // GroovyObject implementation
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "invokeMethod:(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "invokeMethod",
            "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "getMetaClass:()Lgroovy/lang/MetaClass;".to_string(),
        StubMethodDescriptor::instance_method("getMetaClass", "()Lgroovy/lang/MetaClass;"),
    );
    methods.insert(
        "setMetaClass:(Lgroovy/lang/MetaClass;)V".to_string(),
        StubMethodDescriptor::instance_method("setMetaClass", "(Lgroovy/lang/MetaClass;)V"),
    );

    // asWritable
    methods.insert(
        "asWritable:()Lgroovy/lang/Writable;".to_string(),
        StubMethodDescriptor::instance_method("asWritable", "()Lgroovy/lang/Writable;"),
    );

    // Static IDENTITY closure
    // Note: This is a static field, not a method, but we include a getter pattern
    methods.insert(
        "getIDENTITY:()Lgroovy/lang/Closure;".to_string(),
        StubMethodDescriptor::static_method("getIDENTITY", "()Lgroovy/lang/Closure;"),
    );

    StubClass {
        name: "groovy/lang/Closure".to_string(),
        superclass: Some("groovy/lang/GroovyObjectSupport".to_string()),
        interfaces: vec![
            "java/lang/Cloneable".to_string(),
            "java/lang/Runnable".to_string(),
            "java/util/concurrent/Callable".to_string(),
            "java/io/Serializable".to_string(),
            "groovy/lang/GroovyObject".to_string(),
        ],
        methods,
    }
}

// =============================================================================
// GSTRING - Groovy interpolated strings
// =============================================================================

/// Create groovy.lang.GString class - interpolated strings
pub fn create_gstring_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:([Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("([Ljava/lang/Object;)V"),
    );

    // Abstract methods
    methods.insert(
        "getStrings:()[Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getStrings", "()[Ljava/lang/String;"),
    );

    // Values
    methods.insert(
        "getValues:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getValues", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "getValue:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getValue", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "getValueCount:()I".to_string(),
        StubMethodDescriptor::instance_method("getValueCount", "()I"),
    );

    // CharSequence implementation
    methods.insert(
        "length:()I".to_string(),
        StubMethodDescriptor::instance_method("length", "()I"),
    );
    methods.insert(
        "charAt:(I)C".to_string(),
        StubMethodDescriptor::instance_method("charAt", "(I)C"),
    );
    methods.insert(
        "subSequence:(II)Ljava/lang/CharSequence;".to_string(),
        StubMethodDescriptor::instance_method("subSequence", "(II)Ljava/lang/CharSequence;"),
    );

    // Comparable
    methods.insert(
        "compareTo:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/lang/Object;)I"),
    );

    // String operations
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "plus:(Lgroovy/lang/GString;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("plus", "(Lgroovy/lang/GString;)Ljava/lang/String;"),
    );
    methods.insert(
        "plus:(Ljava/lang/String;)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("plus", "(Ljava/lang/String;)Ljava/lang/String;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );

    // Writable
    methods.insert(
        "writeTo:(Ljava/io/Writer;)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::instance_method("writeTo", "(Ljava/io/Writer;)Ljava/io/Writer;"),
    );

    // Pattern matching
    methods.insert(
        "negate:()Ljava/util/regex/Pattern;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/regex/Pattern;"),
    );
    methods.insert(
        "matches:(Ljava/lang/String;)Ljava/util/regex/Matcher;".to_string(),
        StubMethodDescriptor::instance_method(
            "matches",
            "(Ljava/lang/String;)Ljava/util/regex/Matcher;",
        ),
    );

    StubClass {
        name: "groovy/lang/GString".to_string(),
        superclass: Some("groovy/lang/GroovyObjectSupport".to_string()),
        interfaces: vec![
            "java/lang/Comparable".to_string(),
            "java/lang/CharSequence".to_string(),
            "java/io/Serializable".to_string(),
            "groovy/lang/Writable".to_string(),
        ],
        methods,
    }
}

// =============================================================================
// RANGE - Groovy range support
// =============================================================================

/// Create groovy.lang.Range interface
pub fn create_range_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getFrom:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getFrom", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "getTo:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getTo", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "isReverse:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReverse", "()Z"),
    );
    methods.insert(
        "containsWithinBounds:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsWithinBounds", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "step:(ILgroovy/lang/Closure;)V".to_string(),
        StubMethodDescriptor::instance_method("step", "(ILgroovy/lang/Closure;)V"),
    );
    methods.insert(
        "step:(I)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("step", "(I)Ljava/util/List;"),
    );
    methods.insert(
        "inspect:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("inspect", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "groovy/lang/Range".to_string(),
        superclass: None, // interface
        interfaces: vec!["java/util/List".to_string()],
        methods,
    }
}

/// Create groovy.lang.IntRange class
pub fn create_int_range_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(II)V".to_string(),
        StubMethodDescriptor::constructor("(II)V"),
    );
    methods.insert(
        "<init>:(ZII)V".to_string(),
        StubMethodDescriptor::constructor("(ZII)V"),
    );

    // Range interface implementation
    methods.insert(
        "getFrom:()Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::instance_method("getFrom", "()Ljava/lang/Integer;"),
    );
    methods.insert(
        "getTo:()Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::instance_method("getTo", "()Ljava/lang/Integer;"),
    );
    methods.insert(
        "isReverse:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReverse", "()Z"),
    );
    methods.insert(
        "containsWithinBounds:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsWithinBounds", "(Ljava/lang/Object;)Z"),
    );

    // List interface implementation
    methods.insert(
        "get:(I)Ljava/lang/Integer;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Integer;"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "contains:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("contains", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );
    methods.insert(
        "listIterator:(I)Ljava/util/ListIterator;".to_string(),
        StubMethodDescriptor::instance_method("listIterator", "(I)Ljava/util/ListIterator;"),
    );
    methods.insert(
        "subList:(II)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("subList", "(II)Ljava/util/List;"),
    );

    // Step methods
    methods.insert(
        "step:(ILgroovy/lang/Closure;)V".to_string(),
        StubMethodDescriptor::instance_method("step", "(ILgroovy/lang/Closure;)V"),
    );
    methods.insert(
        "step:(I)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("step", "(I)Ljava/util/List;"),
    );

    // Inspect
    methods.insert(
        "inspect:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("inspect", "()Ljava/lang/String;"),
    );

    // Primitive access
    methods.insert(
        "getFromInt:()I".to_string(),
        StubMethodDescriptor::instance_method("getFromInt", "()I"),
    );
    methods.insert(
        "getToInt:()I".to_string(),
        StubMethodDescriptor::instance_method("getToInt", "()I"),
    );

    StubClass {
        name: "groovy/lang/IntRange".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec!["groovy/lang/Range".to_string()],
        methods,
    }
}

/// Create groovy.lang.ObjectRange class
pub fn create_object_range_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/Comparable;Ljava/lang/Comparable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Comparable;Ljava/lang/Comparable;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/Comparable;Ljava/lang/Comparable;Z)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Comparable;Ljava/lang/Comparable;Z)V"),
    );

    // Range interface implementation
    methods.insert(
        "getFrom:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getFrom", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "getTo:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getTo", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "isReverse:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReverse", "()Z"),
    );
    methods.insert(
        "containsWithinBounds:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsWithinBounds", "(Ljava/lang/Object;)Z"),
    );

    // List interface implementation
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "iterator:()Ljava/util/Iterator;".to_string(),
        StubMethodDescriptor::instance_method("iterator", "()Ljava/util/Iterator;"),
    );

    // Step methods
    methods.insert(
        "step:(ILgroovy/lang/Closure;)V".to_string(),
        StubMethodDescriptor::instance_method("step", "(ILgroovy/lang/Closure;)V"),
    );
    methods.insert(
        "step:(I)Ljava/util/List;".to_string(),
        StubMethodDescriptor::instance_method("step", "(I)Ljava/util/List;"),
    );

    // Inspect
    methods.insert(
        "inspect:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("inspect", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "groovy/lang/ObjectRange".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec!["groovy/lang/Range".to_string()],
        methods,
    }
}

/// Create groovy.lang.EmptyRange class
pub fn create_empty_range_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/Comparable;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Comparable;)V"),
    );

    // Range interface implementation
    methods.insert(
        "getFrom:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getFrom", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "getTo:()Ljava/lang/Comparable;".to_string(),
        StubMethodDescriptor::instance_method("getTo", "()Ljava/lang/Comparable;"),
    );
    methods.insert(
        "isReverse:()Z".to_string(),
        StubMethodDescriptor::instance_method("isReverse", "()Z"),
    );
    methods.insert(
        "containsWithinBounds:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsWithinBounds", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );

    StubClass {
        name: "groovy/lang/EmptyRange".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec!["groovy/lang/Range".to_string()],
        methods,
    }
}

// =============================================================================
// ADDITIONAL GROOVY INTERFACES AND UTILITIES
// =============================================================================

/// Create groovy.lang.Writable interface
pub fn create_writable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "writeTo:(Ljava/io/Writer;)Ljava/io/Writer;".to_string(),
        StubMethodDescriptor::instance_method("writeTo", "(Ljava/io/Writer;)Ljava/io/Writer;"),
    );

    StubClass {
        name: "groovy/lang/Writable".to_string(),
        superclass: None, // interface
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyCallable interface
pub fn create_groovy_callable_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "call:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("call", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "groovy/lang/GroovyCallable".to_string(),
        superclass: None, // interface
        interfaces: vec!["java/util/concurrent/Callable".to_string()],
        methods,
    }
}

/// Create groovy.lang.GroovyInterceptable interface
pub fn create_groovy_interceptable_interface() -> StubClass {
    // Marker interface - no methods, just extends GroovyObject
    StubClass {
        name: "groovy/lang/GroovyInterceptable".to_string(),
        superclass: None, // interface
        interfaces: vec!["groovy/lang/GroovyObject".to_string()],
        methods: HashMap::new(),
    }
}

/// Create groovy.lang.MissingMethodException class
pub fn create_missing_method_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;[Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/String;Ljava/lang/Class;[Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;[Ljava/lang/Object;Z)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/String;Ljava/lang/Class;[Ljava/lang/Object;Z)V",
        ),
    );

    // Getters
    methods.insert(
        "getMethod:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMethod", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getArguments:()[Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getArguments", "()[Ljava/lang/Object;"),
    );
    methods.insert(
        "isStatic:()Z".to_string(),
        StubMethodDescriptor::instance_method("isStatic", "()Z"),
    );
    methods.insert(
        "getMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMessage", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "groovy/lang/MissingMethodException".to_string(),
        superclass: Some("groovy/lang/GroovyRuntimeException".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.MissingPropertyException class
pub fn create_missing_property_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/lang/Class;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/Class;Ljava/lang/Throwable;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/String;Ljava/lang/Class;Ljava/lang/Throwable;)V",
        ),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Getters
    methods.insert(
        "getProperty:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getProperty", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getType:()Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getType", "()Ljava/lang/Class;"),
    );
    methods.insert(
        "getMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMessage", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "groovy/lang/MissingPropertyException".to_string(),
        superclass: Some("groovy/lang/GroovyRuntimeException".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyRuntimeException class
pub fn create_groovy_runtime_exception_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
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

    // Methods
    methods.insert(
        "getMessage:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getMessage", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getMessageWithoutLocationText:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMessageWithoutLocationText",
            "()Ljava/lang/String;",
        ),
    );
    methods.insert(
        "setModule:(Lorg/codehaus/groovy/ast/ModuleNode;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setModule",
            "(Lorg/codehaus/groovy/ast/ModuleNode;)V",
        ),
    );
    methods.insert(
        "getModule:()Lorg/codehaus/groovy/ast/ModuleNode;".to_string(),
        StubMethodDescriptor::instance_method(
            "getModule",
            "()Lorg/codehaus/groovy/ast/ModuleNode;",
        ),
    );

    StubClass {
        name: "groovy/lang/GroovyRuntimeException".to_string(),
        superclass: Some("java/lang/RuntimeException".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.MixinInMetaClass interface placeholder
pub fn create_mixin_in_meta_class_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getMixinClass:()Lorg/codehaus/groovy/reflection/CachedClass;".to_string(),
        StubMethodDescriptor::instance_method(
            "getMixinClass",
            "()Lorg/codehaus/groovy/reflection/CachedClass;",
        ),
    );

    StubClass {
        name: "groovy/lang/MixinInMetaClass".to_string(),
        superclass: None, // interface
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.Tuple class
pub fn create_tuple_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:([Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("([Ljava/lang/Object;)V"),
    );

    // AbstractList methods
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );

    // Tuple-specific
    methods.insert(
        "subTuple:(II)Lgroovy/lang/Tuple;".to_string(),
        StubMethodDescriptor::instance_method("subTuple", "(II)Lgroovy/lang/Tuple;"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );

    StubClass {
        name: "groovy/lang/Tuple".to_string(),
        superclass: Some("java/util/AbstractList".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.Tuple2 class
pub fn create_tuple2_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;Ljava/lang/Object;)V"),
    );

    // Getters
    methods.insert(
        "getFirst:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getFirst", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getSecond:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getSecond", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getV1:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getV1", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "getV2:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("getV2", "()Ljava/lang/Object;"),
    );

    // Size (always 2)
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "get:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(I)Ljava/lang/Object;"),
    );

    StubClass {
        name: "groovy/lang/Tuple2".to_string(),
        superclass: Some("groovy/lang/Tuple".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.Reference class (for closure variable capture)
pub fn create_reference_class() -> StubClass {
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

    // Get/Set
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );
    methods.insert(
        "set:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("set", "(Ljava/lang/Object;)V"),
    );

    // GroovyObject methods
    methods.insert(
        "getProperty:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getProperty",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "setProperty:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setProperty",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );

    StubClass {
        name: "groovy/lang/Reference".to_string(),
        superclass: Some("groovy/lang/GroovyObjectSupport".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create groovy.lang.SpreadMap class
pub fn create_spread_map_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructor
    methods.insert(
        "<init>:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/Object;)V"),
    );
    methods.insert(
        "<init>:(Ljava/util/Map;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/util/Map;)V"),
    );

    // Map methods
    methods.insert(
        "size:()I".to_string(),
        StubMethodDescriptor::instance_method("size", "()I"),
    );
    methods.insert(
        "isEmpty:()Z".to_string(),
        StubMethodDescriptor::instance_method("isEmpty", "()Z"),
    );
    methods.insert(
        "containsKey:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("containsKey", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "get:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );
    methods.insert(
        "keySet:()Ljava/util/Set;".to_string(),
        StubMethodDescriptor::instance_method("keySet", "()Ljava/util/Set;"),
    );
    methods.insert(
        "values:()Ljava/util/Collection;".to_string(),
        StubMethodDescriptor::instance_method("values", "()Ljava/util/Collection;"),
    );

    StubClass {
        name: "groovy/lang/SpreadMap".to_string(),
        superclass: Some("java/util/HashMap".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyShell class
pub fn create_groovy_shell_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Lgroovy/lang/Binding;)V".to_string(),
        StubMethodDescriptor::constructor("(Lgroovy/lang/Binding;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ClassLoader;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ClassLoader;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ClassLoader;Lgroovy/lang/Binding;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ClassLoader;Lgroovy/lang/Binding;)V"),
    );

    // Evaluate methods
    methods.insert(
        "evaluate:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("evaluate", "(Ljava/lang/String;)Ljava/lang/Object;"),
    );
    methods.insert(
        "evaluate:(Ljava/io/File;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("evaluate", "(Ljava/io/File;)Ljava/lang/Object;"),
    );
    methods.insert(
        "evaluate:(Ljava/io/Reader;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("evaluate", "(Ljava/io/Reader;)Ljava/lang/Object;"),
    );
    methods.insert(
        "evaluate:(Lgroovy/lang/GroovyCodeSource;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "evaluate",
            "(Lgroovy/lang/GroovyCodeSource;)Ljava/lang/Object;",
        ),
    );

    // Parse methods
    methods.insert(
        "parse:(Ljava/lang/String;)Lgroovy/lang/Script;".to_string(),
        StubMethodDescriptor::instance_method("parse", "(Ljava/lang/String;)Lgroovy/lang/Script;"),
    );
    methods.insert(
        "parse:(Ljava/io/File;)Lgroovy/lang/Script;".to_string(),
        StubMethodDescriptor::instance_method("parse", "(Ljava/io/File;)Lgroovy/lang/Script;"),
    );
    methods.insert(
        "parse:(Ljava/io/Reader;)Lgroovy/lang/Script;".to_string(),
        StubMethodDescriptor::instance_method("parse", "(Ljava/io/Reader;)Lgroovy/lang/Script;"),
    );

    // Run methods
    methods.insert(
        "run:(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Object;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "run",
            "(Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );
    methods.insert(
        "run:(Ljava/io/File;[Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "run",
            "(Ljava/io/File;[Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );

    // Context
    methods.insert(
        "getContext:()Lgroovy/lang/Binding;".to_string(),
        StubMethodDescriptor::instance_method("getContext", "()Lgroovy/lang/Binding;"),
    );
    methods.insert(
        "setVariable:(Ljava/lang/String;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method(
            "setVariable",
            "(Ljava/lang/String;Ljava/lang/Object;)V",
        ),
    );
    methods.insert(
        "getVariable:(Ljava/lang/String;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "getVariable",
            "(Ljava/lang/String;)Ljava/lang/Object;",
        ),
    );

    // Class loader
    methods.insert(
        "getClassLoader:()Lgroovy/lang/GroovyClassLoader;".to_string(),
        StubMethodDescriptor::instance_method(
            "getClassLoader",
            "()Lgroovy/lang/GroovyClassLoader;",
        ),
    );

    StubClass {
        name: "groovy/lang/GroovyShell".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyClassLoader class
pub fn create_groovy_class_loader_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:()V".to_string(),
        StubMethodDescriptor::constructor("()V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/ClassLoader;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/ClassLoader;)V"),
    );

    // Parse class
    methods.insert(
        "parseClass:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "parseClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
        ),
    );
    methods.insert(
        "parseClass:(Ljava/io/File;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("parseClass", "(Ljava/io/File;)Ljava/lang/Class;"),
    );
    methods.insert(
        "parseClass:(Ljava/io/InputStream;Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "parseClass",
            "(Ljava/io/InputStream;Ljava/lang/String;)Ljava/lang/Class;",
        ),
    );
    methods.insert(
        "parseClass:(Lgroovy/lang/GroovyCodeSource;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "parseClass",
            "(Lgroovy/lang/GroovyCodeSource;)Ljava/lang/Class;",
        ),
    );

    // ClassLoader methods
    methods.insert(
        "loadClass:(Ljava/lang/String;)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("loadClass", "(Ljava/lang/String;)Ljava/lang/Class;"),
    );
    methods.insert(
        "loadClass:(Ljava/lang/String;ZZZ)Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method(
            "loadClass",
            "(Ljava/lang/String;ZZZ)Ljava/lang/Class;",
        ),
    );

    // Resource management
    methods.insert(
        "addClasspath:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::instance_method("addClasspath", "(Ljava/lang/String;)V"),
    );
    methods.insert(
        "addURL:(Ljava/net/URL;)V".to_string(),
        StubMethodDescriptor::instance_method("addURL", "(Ljava/net/URL;)V"),
    );
    methods.insert(
        "getLoadedClasses:()[Ljava/lang/Class;".to_string(),
        StubMethodDescriptor::instance_method("getLoadedClasses", "()[Ljava/lang/Class;"),
    );

    StubClass {
        name: "groovy/lang/GroovyClassLoader".to_string(),
        superclass: Some("java/net/URLClassLoader".to_string()),
        interfaces: vec![],
        methods,
    }
}

/// Create groovy.lang.GroovyCodeSource class
pub fn create_groovy_code_source_class() -> StubClass {
    let mut methods = HashMap::new();

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor(
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
        ),
    );
    methods.insert(
        "<init>:(Ljava/io/File;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;)V"),
    );
    methods.insert(
        "<init>:(Ljava/io/File;Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/io/File;Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/net/URL;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/net/URL;)V"),
    );

    // Getters
    methods.insert(
        "getName:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getName", "()Ljava/lang/String;"),
    );
    methods.insert(
        "getFile:()Ljava/io/File;".to_string(),
        StubMethodDescriptor::instance_method("getFile", "()Ljava/io/File;"),
    );
    methods.insert(
        "getURL:()Ljava/net/URL;".to_string(),
        StubMethodDescriptor::instance_method("getURL", "()Ljava/net/URL;"),
    );
    methods.insert(
        "getScriptText:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("getScriptText", "()Ljava/lang/String;"),
    );
    methods.insert(
        "isCachable:()Z".to_string(),
        StubMethodDescriptor::instance_method("isCachable", "()Z"),
    );
    methods.insert(
        "setCachable:(Z)V".to_string(),
        StubMethodDescriptor::instance_method("setCachable", "(Z)V"),
    );

    StubClass {
        name: "groovy/lang/GroovyCodeSource".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec![],
        methods,
    }
}
