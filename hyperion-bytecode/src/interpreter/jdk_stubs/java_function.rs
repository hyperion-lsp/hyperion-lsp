// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.util.function package stub implementations
//!
//! Functional interfaces introduced in Java 8 for lambda expressions and method references.
//! These are critical for modern Java and stream API support.

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

// ==================== Core Functional Interfaces ====================

/// Create java.util.function.Function<T, R> interface stub
pub fn create_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "apply:(Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("apply", "(Ljava/lang/Object;)Ljava/lang/Object;"),
    );

    // Default methods
    methods.insert(
        "compose:(Ljava/util/function/Function;)Ljava/util/function/Function;".to_string(),
        StubMethodDescriptor::instance_method(
            "compose",
            "(Ljava/util/function/Function;)Ljava/util/function/Function;",
        ),
    );
    methods.insert(
        "andThen:(Ljava/util/function/Function;)Ljava/util/function/Function;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/Function;)Ljava/util/function/Function;",
        ),
    );

    // Static method
    methods.insert(
        "identity:()Ljava/util/function/Function;".to_string(),
        StubMethodDescriptor::static_method("identity", "()Ljava/util/function/Function;"),
    );

    StubClass {
        name: "java/util/function/Function".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.BiFunction<T, U, R> interface stub
pub fn create_bifunction_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "apply:(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method(
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        ),
    );

    // Default method
    methods.insert(
        "andThen:(Ljava/util/function/Function;)Ljava/util/function/BiFunction;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/Function;)Ljava/util/function/BiFunction;",
        ),
    );

    StubClass {
        name: "java/util/function/BiFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.Consumer<T> interface stub
pub fn create_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "accept:(Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;)V"),
    );

    // Default method
    methods.insert(
        "andThen:(Ljava/util/function/Consumer;)Ljava/util/function/Consumer;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/Consumer;)Ljava/util/function/Consumer;",
        ),
    );

    StubClass {
        name: "java/util/function/Consumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.BiConsumer<T, U> interface stub
pub fn create_biconsumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "accept:(Ljava/lang/Object;Ljava/lang/Object;)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;Ljava/lang/Object;)V"),
    );

    // Default method
    methods.insert(
        "andThen:(Ljava/util/function/BiConsumer;)Ljava/util/function/BiConsumer;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/BiConsumer;)Ljava/util/function/BiConsumer;",
        ),
    );

    StubClass {
        name: "java/util/function/BiConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.Supplier<T> interface stub
pub fn create_supplier_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "get:()Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("get", "()Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/function/Supplier".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.Predicate<T> interface stub
pub fn create_predicate_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "test:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("test", "(Ljava/lang/Object;)Z"),
    );

    // Default methods
    methods.insert(
        "and:(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/Predicate;)Ljava/util/function/Predicate;",
        ),
    );
    methods.insert(
        "negate:()Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/function/Predicate;"),
    );

    // Static method
    methods.insert(
        "isEqual:(Ljava/lang/Object;)Ljava/util/function/Predicate;".to_string(),
        StubMethodDescriptor::static_method(
            "isEqual",
            "(Ljava/lang/Object;)Ljava/util/function/Predicate;",
        ),
    );

    StubClass {
        name: "java/util/function/Predicate".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.BiPredicate<T, U> interface stub
pub fn create_bipredicate_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Abstract method
    methods.insert(
        "test:(Ljava/lang/Object;Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("test", "(Ljava/lang/Object;Ljava/lang/Object;)Z"),
    );

    // Default methods
    methods.insert(
        "and:(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/BiPredicate;)Ljava/util/function/BiPredicate;",
        ),
    );
    methods.insert(
        "negate:()Ljava/util/function/BiPredicate;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/function/BiPredicate;"),
    );

    StubClass {
        name: "java/util/function/BiPredicate".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.UnaryOperator<T> interface stub
pub fn create_unary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Inherits apply from Function - no new abstract methods

    // Static method
    methods.insert(
        "identity:()Ljava/util/function/UnaryOperator;".to_string(),
        StubMethodDescriptor::static_method("identity", "()Ljava/util/function/UnaryOperator;"),
    );

    StubClass {
        name: "java/util/function/UnaryOperator".to_string(),
        superclass: None,
        interfaces: vec!["java/util/function/Function".to_string()],
        methods,
    }
}

/// Create java.util.function.BinaryOperator<T> interface stub
pub fn create_binary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    // Inherits apply from BiFunction - no new abstract methods

    // Static methods
    methods.insert(
        "minBy:(Ljava/util/Comparator;)Ljava/util/function/BinaryOperator;".to_string(),
        StubMethodDescriptor::static_method(
            "minBy",
            "(Ljava/util/Comparator;)Ljava/util/function/BinaryOperator;",
        ),
    );
    methods.insert(
        "maxBy:(Ljava/util/Comparator;)Ljava/util/function/BinaryOperator;".to_string(),
        StubMethodDescriptor::static_method(
            "maxBy",
            "(Ljava/util/Comparator;)Ljava/util/function/BinaryOperator;",
        ),
    );

    StubClass {
        name: "java/util/function/BinaryOperator".to_string(),
        superclass: None,
        interfaces: vec!["java/util/function/BiFunction".to_string()],
        methods,
    }
}

// ==================== Int Primitive Specializations ====================

/// Create java.util.function.IntFunction<R> interface stub
pub fn create_int_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "apply:(I)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("apply", "(I)Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/function/IntFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntConsumer interface stub
pub fn create_int_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(I)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(I)V"),
    );

    methods.insert(
        "andThen:(Ljava/util/function/IntConsumer;)Ljava/util/function/IntConsumer;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/IntConsumer;)Ljava/util/function/IntConsumer;",
        ),
    );

    StubClass {
        name: "java/util/function/IntConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntSupplier interface stub
pub fn create_int_supplier_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getAsInt:()I".to_string(),
        StubMethodDescriptor::instance_method("getAsInt", "()I"),
    );

    StubClass {
        name: "java/util/function/IntSupplier".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntPredicate interface stub
pub fn create_int_predicate_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "test:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("test", "(I)Z"),
    );

    methods.insert(
        "and:(Ljava/util/function/IntPredicate;)Ljava/util/function/IntPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/util/function/IntPredicate;)Ljava/util/function/IntPredicate;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/IntPredicate;)Ljava/util/function/IntPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/IntPredicate;)Ljava/util/function/IntPredicate;",
        ),
    );
    methods.insert(
        "negate:()Ljava/util/function/IntPredicate;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/function/IntPredicate;"),
    );

    StubClass {
        name: "java/util/function/IntPredicate".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntUnaryOperator interface stub
pub fn create_int_unary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(I)I".to_string(),
        StubMethodDescriptor::instance_method("applyAsInt", "(I)I"),
    );

    methods.insert(
        "compose:(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "compose",
            "(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;",
        ),
    );
    methods.insert(
        "andThen:(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/IntUnaryOperator;)Ljava/util/function/IntUnaryOperator;",
        ),
    );

    methods.insert(
        "identity:()Ljava/util/function/IntUnaryOperator;".to_string(),
        StubMethodDescriptor::static_method("identity", "()Ljava/util/function/IntUnaryOperator;"),
    );

    StubClass {
        name: "java/util/function/IntUnaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntBinaryOperator interface stub
pub fn create_int_binary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(II)I".to_string(),
        StubMethodDescriptor::instance_method("applyAsInt", "(II)I"),
    );

    StubClass {
        name: "java/util/function/IntBinaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ToIntFunction<T> interface stub
pub fn create_to_int_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method("applyAsInt", "(Ljava/lang/Object;)I"),
    );

    StubClass {
        name: "java/util/function/ToIntFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== Long Primitive Specializations ====================

/// Create java.util.function.LongFunction<R> interface stub
pub fn create_long_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "apply:(J)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("apply", "(J)Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/function/LongFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongConsumer interface stub
pub fn create_long_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(J)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(J)V"),
    );

    methods.insert(
        "andThen:(Ljava/util/function/LongConsumer;)Ljava/util/function/LongConsumer;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/LongConsumer;)Ljava/util/function/LongConsumer;",
        ),
    );

    StubClass {
        name: "java/util/function/LongConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongSupplier interface stub
pub fn create_long_supplier_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getAsLong:()J".to_string(),
        StubMethodDescriptor::instance_method("getAsLong", "()J"),
    );

    StubClass {
        name: "java/util/function/LongSupplier".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongPredicate interface stub
pub fn create_long_predicate_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "test:(J)Z".to_string(),
        StubMethodDescriptor::instance_method("test", "(J)Z"),
    );

    methods.insert(
        "and:(Ljava/util/function/LongPredicate;)Ljava/util/function/LongPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/util/function/LongPredicate;)Ljava/util/function/LongPredicate;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/LongPredicate;)Ljava/util/function/LongPredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/LongPredicate;)Ljava/util/function/LongPredicate;",
        ),
    );
    methods.insert(
        "negate:()Ljava/util/function/LongPredicate;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/function/LongPredicate;"),
    );

    StubClass {
        name: "java/util/function/LongPredicate".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongUnaryOperator interface stub
pub fn create_long_unary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(J)J".to_string(),
        StubMethodDescriptor::instance_method("applyAsLong", "(J)J"),
    );

    methods.insert(
        "compose:(Ljava/util/function/LongUnaryOperator;)Ljava/util/function/LongUnaryOperator;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "compose",
            "(Ljava/util/function/LongUnaryOperator;)Ljava/util/function/LongUnaryOperator;",
        ),
    );
    methods.insert(
        "andThen:(Ljava/util/function/LongUnaryOperator;)Ljava/util/function/LongUnaryOperator;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/LongUnaryOperator;)Ljava/util/function/LongUnaryOperator;",
        ),
    );

    methods.insert(
        "identity:()Ljava/util/function/LongUnaryOperator;".to_string(),
        StubMethodDescriptor::static_method("identity", "()Ljava/util/function/LongUnaryOperator;"),
    );

    StubClass {
        name: "java/util/function/LongUnaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongBinaryOperator interface stub
pub fn create_long_binary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(JJ)J".to_string(),
        StubMethodDescriptor::instance_method("applyAsLong", "(JJ)J"),
    );

    StubClass {
        name: "java/util/function/LongBinaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ToLongFunction<T> interface stub
pub fn create_to_long_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(Ljava/lang/Object;)J".to_string(),
        StubMethodDescriptor::instance_method("applyAsLong", "(Ljava/lang/Object;)J"),
    );

    StubClass {
        name: "java/util/function/ToLongFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== Double Primitive Specializations ====================

/// Create java.util.function.DoubleFunction<R> interface stub
pub fn create_double_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "apply:(D)Ljava/lang/Object;".to_string(),
        StubMethodDescriptor::instance_method("apply", "(D)Ljava/lang/Object;"),
    );

    StubClass {
        name: "java/util/function/DoubleFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleConsumer interface stub
pub fn create_double_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(D)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(D)V"),
    );

    methods.insert(
        "andThen:(Ljava/util/function/DoubleConsumer;)Ljava/util/function/DoubleConsumer;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/DoubleConsumer;)Ljava/util/function/DoubleConsumer;",
        ),
    );

    StubClass {
        name: "java/util/function/DoubleConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleSupplier interface stub
pub fn create_double_supplier_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "getAsDouble:()D".to_string(),
        StubMethodDescriptor::instance_method("getAsDouble", "()D"),
    );

    StubClass {
        name: "java/util/function/DoubleSupplier".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoublePredicate interface stub
pub fn create_double_predicate_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "test:(D)Z".to_string(),
        StubMethodDescriptor::instance_method("test", "(D)Z"),
    );

    methods.insert(
        "and:(Ljava/util/function/DoublePredicate;)Ljava/util/function/DoublePredicate;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/util/function/DoublePredicate;)Ljava/util/function/DoublePredicate;",
        ),
    );
    methods.insert(
        "or:(Ljava/util/function/DoublePredicate;)Ljava/util/function/DoublePredicate;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/util/function/DoublePredicate;)Ljava/util/function/DoublePredicate;",
        ),
    );
    methods.insert(
        "negate:()Ljava/util/function/DoublePredicate;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/util/function/DoublePredicate;"),
    );

    StubClass {
        name: "java/util/function/DoublePredicate".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleUnaryOperator interface stub
pub fn create_double_unary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(D)D".to_string(),
        StubMethodDescriptor::instance_method("applyAsDouble", "(D)D"),
    );

    methods.insert(
        "compose:(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/function/DoubleUnaryOperator;".to_string(),
        StubMethodDescriptor::instance_method(
            "compose",
            "(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/function/DoubleUnaryOperator;",
        ),
    );
    methods.insert(
        "andThen:(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/function/DoubleUnaryOperator;".to_string(),
        StubMethodDescriptor::instance_method(
            "andThen",
            "(Ljava/util/function/DoubleUnaryOperator;)Ljava/util/function/DoubleUnaryOperator;",
        ),
    );

    methods.insert(
        "identity:()Ljava/util/function/DoubleUnaryOperator;".to_string(),
        StubMethodDescriptor::static_method(
            "identity",
            "()Ljava/util/function/DoubleUnaryOperator;",
        ),
    );

    StubClass {
        name: "java/util/function/DoubleUnaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleBinaryOperator interface stub
pub fn create_double_binary_operator_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(DD)D".to_string(),
        StubMethodDescriptor::instance_method("applyAsDouble", "(DD)D"),
    );

    StubClass {
        name: "java/util/function/DoubleBinaryOperator".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ToDoubleFunction<T> interface stub
pub fn create_to_double_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(Ljava/lang/Object;)D".to_string(),
        StubMethodDescriptor::instance_method("applyAsDouble", "(Ljava/lang/Object;)D"),
    );

    StubClass {
        name: "java/util/function/ToDoubleFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== Binary Object-Primitive Consumers ====================

/// Create java.util.function.ObjIntConsumer<T> interface stub
pub fn create_obj_int_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/lang/Object;I)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;I)V"),
    );

    StubClass {
        name: "java/util/function/ObjIntConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ObjLongConsumer<T> interface stub
pub fn create_obj_long_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/lang/Object;J)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;J)V"),
    );

    StubClass {
        name: "java/util/function/ObjLongConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ObjDoubleConsumer<T> interface stub
pub fn create_obj_double_consumer_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "accept:(Ljava/lang/Object;D)V".to_string(),
        StubMethodDescriptor::instance_method("accept", "(Ljava/lang/Object;D)V"),
    );

    StubClass {
        name: "java/util/function/ObjDoubleConsumer".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== Binary ToX Functions ====================

/// Create java.util.function.ToIntBiFunction<T, U> interface stub
pub fn create_to_int_bifunction_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(Ljava/lang/Object;Ljava/lang/Object;)I".to_string(),
        StubMethodDescriptor::instance_method(
            "applyAsInt",
            "(Ljava/lang/Object;Ljava/lang/Object;)I",
        ),
    );

    StubClass {
        name: "java/util/function/ToIntBiFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ToLongBiFunction<T, U> interface stub
pub fn create_to_long_bifunction_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(Ljava/lang/Object;Ljava/lang/Object;)J".to_string(),
        StubMethodDescriptor::instance_method(
            "applyAsLong",
            "(Ljava/lang/Object;Ljava/lang/Object;)J",
        ),
    );

    StubClass {
        name: "java/util/function/ToLongBiFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.ToDoubleBiFunction<T, U> interface stub
pub fn create_to_double_bifunction_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(Ljava/lang/Object;Ljava/lang/Object;)D".to_string(),
        StubMethodDescriptor::instance_method(
            "applyAsDouble",
            "(Ljava/lang/Object;Ljava/lang/Object;)D",
        ),
    );

    StubClass {
        name: "java/util/function/ToDoubleBiFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

// ==================== Cross-Primitive Conversion Functions ====================

/// Create java.util.function.IntToLongFunction interface stub
pub fn create_int_to_long_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(I)J".to_string(),
        StubMethodDescriptor::instance_method("applyAsLong", "(I)J"),
    );

    StubClass {
        name: "java/util/function/IntToLongFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.IntToDoubleFunction interface stub
pub fn create_int_to_double_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(I)D".to_string(),
        StubMethodDescriptor::instance_method("applyAsDouble", "(I)D"),
    );

    StubClass {
        name: "java/util/function/IntToDoubleFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongToIntFunction interface stub
pub fn create_long_to_int_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(J)I".to_string(),
        StubMethodDescriptor::instance_method("applyAsInt", "(J)I"),
    );

    StubClass {
        name: "java/util/function/LongToIntFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.LongToDoubleFunction interface stub
pub fn create_long_to_double_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsDouble:(J)D".to_string(),
        StubMethodDescriptor::instance_method("applyAsDouble", "(J)D"),
    );

    StubClass {
        name: "java/util/function/LongToDoubleFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleToIntFunction interface stub
pub fn create_double_to_int_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsInt:(D)I".to_string(),
        StubMethodDescriptor::instance_method("applyAsInt", "(D)I"),
    );

    StubClass {
        name: "java/util/function/DoubleToIntFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}

/// Create java.util.function.DoubleToLongFunction interface stub
pub fn create_double_to_long_function_interface() -> StubClass {
    let mut methods = HashMap::new();

    methods.insert(
        "applyAsLong:(D)J".to_string(),
        StubMethodDescriptor::instance_method("applyAsLong", "(D)J"),
    );

    StubClass {
        name: "java/util/function/DoubleToLongFunction".to_string(),
        superclass: None,
        interfaces: vec![],
        methods,
    }
}
