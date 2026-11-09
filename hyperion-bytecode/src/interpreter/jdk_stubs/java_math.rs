// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! java.math JDK stub classes

use super::{StubClass, StubMethodDescriptor};
use std::collections::HashMap;

/// Create java.math.BigInteger stub class
pub fn create_big_integer_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static constants (ZERO, ONE, TWO, TEN are public static final fields, not methods)

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;I)V"),
    );
    methods.insert(
        "<init>:([B)V".to_string(),
        StubMethodDescriptor::constructor("([B)V"),
    );
    methods.insert(
        "<init>:(I[B)V".to_string(),
        StubMethodDescriptor::constructor("(I[B)V"),
    );
    methods.insert(
        "<init>:(IILjava/util/Random;)V".to_string(),
        StubMethodDescriptor::constructor("(IILjava/util/Random;)V"),
    );
    methods.insert(
        "<init>:(ILjava/util/Random;)V".to_string(),
        StubMethodDescriptor::constructor("(ILjava/util/Random;)V"),
    );

    // Static factory methods
    methods.insert(
        "valueOf:(J)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(J)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "probablePrime:(ILjava/util/Random;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::static_method(
            "probablePrime",
            "(ILjava/util/Random;)Ljava/math/BigInteger;",
        ),
    );

    // Arithmetic operations
    methods.insert(
        "add:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "add",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "subtract:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "subtract",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "multiply:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "multiply",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "remainder:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "remainder",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "divideAndRemainder:(Ljava/math/BigInteger;)[Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "divideAndRemainder",
            "(Ljava/math/BigInteger;)[Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "mod:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "mod",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "modPow:(Ljava/math/BigInteger;Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "modPow",
            "(Ljava/math/BigInteger;Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "modInverse:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "modInverse",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "pow:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("pow", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "sqrt:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("sqrt", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "sqrtAndRemainder:()[Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("sqrtAndRemainder", "()[Ljava/math/BigInteger;"),
    );
    methods.insert(
        "gcd:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "gcd",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "abs:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("abs", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "negate:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "signum:()I".to_string(),
        StubMethodDescriptor::instance_method("signum", "()I"),
    );

    // Bit operations
    methods.insert(
        "and:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "and",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "or:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "or",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "xor:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "xor",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "not:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("not", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "andNot:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "andNot",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "shiftLeft:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("shiftLeft", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "shiftRight:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("shiftRight", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "setBit:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("setBit", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "clearBit:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("clearBit", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "flipBit:(I)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("flipBit", "(I)Ljava/math/BigInteger;"),
    );
    methods.insert(
        "testBit:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("testBit", "(I)Z"),
    );
    methods.insert(
        "bitLength:()I".to_string(),
        StubMethodDescriptor::instance_method("bitLength", "()I"),
    );
    methods.insert(
        "bitCount:()I".to_string(),
        StubMethodDescriptor::instance_method("bitCount", "()I"),
    );
    methods.insert(
        "getLowestSetBit:()I".to_string(),
        StubMethodDescriptor::instance_method("getLowestSetBit", "()I"),
    );

    // Comparison
    methods.insert(
        "compareTo:(Ljava/math/BigInteger;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/math/BigInteger;)I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "min:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "min",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );
    methods.insert(
        "max:(Ljava/math/BigInteger;)Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method(
            "max",
            "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        ),
    );

    // Conversion
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "intValueExact:()I".to_string(),
        StubMethodDescriptor::instance_method("intValueExact", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "longValueExact:()J".to_string(),
        StubMethodDescriptor::instance_method("longValueExact", "()J"),
    );
    methods.insert(
        "shortValueExact:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValueExact", "()S"),
    );
    methods.insert(
        "byteValueExact:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValueExact", "()B"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toString:(I)Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "(I)Ljava/lang/String;"),
    );
    methods.insert(
        "toByteArray:()[B".to_string(),
        StubMethodDescriptor::instance_method("toByteArray", "()[B"),
    );

    // Hash
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );

    // Primality
    methods.insert(
        "isProbablePrime:(I)Z".to_string(),
        StubMethodDescriptor::instance_method("isProbablePrime", "(I)Z"),
    );
    methods.insert(
        "nextProbablePrime:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("nextProbablePrime", "()Ljava/math/BigInteger;"),
    );

    StubClass {
        name: "java/math/BigInteger".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.math.BigDecimal stub class
pub fn create_big_decimal_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static constants (ZERO, ONE, TEN are public static final fields, not methods)
    // Rounding mode constants (deprecated, use RoundingMode enum instead)

    // Constructors
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;Ljava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;Ljava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:(D)V".to_string(),
        StubMethodDescriptor::constructor("(D)V"),
    );
    methods.insert(
        "<init>:(DLjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(DLjava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:(Ljava/math/BigInteger;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/math/BigInteger;)V"),
    );
    methods.insert(
        "<init>:(Ljava/math/BigInteger;Ljava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/math/BigInteger;Ljava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:(Ljava/math/BigInteger;I)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/math/BigInteger;I)V"),
    );
    methods.insert(
        "<init>:(Ljava/math/BigInteger;ILjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/math/BigInteger;ILjava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(ILjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(ILjava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:(J)V".to_string(),
        StubMethodDescriptor::constructor("(J)V"),
    );
    methods.insert(
        "<init>:(JLjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("(JLjava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:([C)V".to_string(),
        StubMethodDescriptor::constructor("([C)V"),
    );
    methods.insert(
        "<init>:([CII)V".to_string(),
        StubMethodDescriptor::constructor("([CII)V"),
    );
    methods.insert(
        "<init>:([CIILjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("([CIILjava/math/MathContext;)V"),
    );
    methods.insert(
        "<init>:([CLjava/math/MathContext;)V".to_string(),
        StubMethodDescriptor::constructor("([CLjava/math/MathContext;)V"),
    );

    // Static factory methods
    methods.insert(
        "valueOf:(J)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(J)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "valueOf:(JI)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(JI)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "valueOf:(D)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(D)Ljava/math/BigDecimal;"),
    );

    // Arithmetic operations
    methods.insert(
        "add:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "add",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "add:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "add",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "subtract:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "subtract",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "subtract:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "subtract",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "multiply:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "multiply",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "multiply:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "multiply",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;I)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;Ljava/math/RoundingMode;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;Ljava/math/RoundingMode;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;II)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;II)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;ILjava/math/RoundingMode;)Ljava/math/BigDecimal;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;ILjava/math/RoundingMode;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divide:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divide",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divideAndRemainder:(Ljava/math/BigDecimal;)[Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divideAndRemainder",
            "(Ljava/math/BigDecimal;)[Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divideAndRemainder:(Ljava/math/BigDecimal;Ljava/math/MathContext;)[Ljava/math/BigDecimal;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "divideAndRemainder",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)[Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divideToIntegralValue:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "divideToIntegralValue",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "divideToIntegralValue:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("divideToIntegralValue", "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "remainder:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "remainder",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "remainder:(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;"
            .to_string(),
        StubMethodDescriptor::instance_method(
            "remainder",
            "(Ljava/math/BigDecimal;Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "pow:(I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("pow", "(I)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "pow:(ILjava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "pow",
            "(ILjava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "sqrt:(Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "sqrt",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "abs:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("abs", "()Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "abs:(Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "abs",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "negate:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("negate", "()Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "negate:(Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "negate",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "plus:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("plus", "()Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "plus:(Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "plus",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );

    // Scale operations
    methods.insert(
        "scale:()I".to_string(),
        StubMethodDescriptor::instance_method("scale", "()I"),
    );
    methods.insert(
        "precision:()I".to_string(),
        StubMethodDescriptor::instance_method("precision", "()I"),
    );
    methods.insert(
        "unscaledValue:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("unscaledValue", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "setScale:(I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("setScale", "(I)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "setScale:(II)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("setScale", "(II)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "setScale:(ILjava/math/RoundingMode;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "setScale",
            "(ILjava/math/RoundingMode;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "movePointLeft:(I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("movePointLeft", "(I)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "movePointRight:(I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("movePointRight", "(I)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "scaleByPowerOfTen:(I)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("scaleByPowerOfTen", "(I)Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "stripTrailingZeros:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("stripTrailingZeros", "()Ljava/math/BigDecimal;"),
    );
    methods.insert(
        "round:(Ljava/math/MathContext;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "round",
            "(Ljava/math/MathContext;)Ljava/math/BigDecimal;",
        ),
    );

    // Comparison
    methods.insert(
        "compareTo:(Ljava/math/BigDecimal;)I".to_string(),
        StubMethodDescriptor::instance_method("compareTo", "(Ljava/math/BigDecimal;)I"),
    );
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "min:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "min",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "max:(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method(
            "max",
            "(Ljava/math/BigDecimal;)Ljava/math/BigDecimal;",
        ),
    );
    methods.insert(
        "signum:()I".to_string(),
        StubMethodDescriptor::instance_method("signum", "()I"),
    );

    // Conversion
    methods.insert(
        "intValue:()I".to_string(),
        StubMethodDescriptor::instance_method("intValue", "()I"),
    );
    methods.insert(
        "intValueExact:()I".to_string(),
        StubMethodDescriptor::instance_method("intValueExact", "()I"),
    );
    methods.insert(
        "longValue:()J".to_string(),
        StubMethodDescriptor::instance_method("longValue", "()J"),
    );
    methods.insert(
        "longValueExact:()J".to_string(),
        StubMethodDescriptor::instance_method("longValueExact", "()J"),
    );
    methods.insert(
        "shortValueExact:()S".to_string(),
        StubMethodDescriptor::instance_method("shortValueExact", "()S"),
    );
    methods.insert(
        "byteValueExact:()B".to_string(),
        StubMethodDescriptor::instance_method("byteValueExact", "()B"),
    );
    methods.insert(
        "floatValue:()F".to_string(),
        StubMethodDescriptor::instance_method("floatValue", "()F"),
    );
    methods.insert(
        "doubleValue:()D".to_string(),
        StubMethodDescriptor::instance_method("doubleValue", "()D"),
    );
    methods.insert(
        "toBigInteger:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("toBigInteger", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "toBigIntegerExact:()Ljava/math/BigInteger;".to_string(),
        StubMethodDescriptor::instance_method("toBigIntegerExact", "()Ljava/math/BigInteger;"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toEngineeringString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toEngineeringString", "()Ljava/lang/String;"),
    );
    methods.insert(
        "toPlainString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toPlainString", "()Ljava/lang/String;"),
    );

    // Hash
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );

    // ULP
    methods.insert(
        "ulp:()Ljava/math/BigDecimal;".to_string(),
        StubMethodDescriptor::instance_method("ulp", "()Ljava/math/BigDecimal;"),
    );

    StubClass {
        name: "java/math/BigDecimal".to_string(),
        superclass: Some("java/lang/Number".to_string()),
        interfaces: vec!["java/lang/Comparable".to_string()],
        methods,
    }
}

/// Create java.math.MathContext stub class
pub fn create_math_context_class() -> StubClass {
    let mut methods = HashMap::new();

    // Static constants (UNLIMITED, DECIMAL32, DECIMAL64, DECIMAL128 are public static final fields)

    // Constructors
    methods.insert(
        "<init>:(I)V".to_string(),
        StubMethodDescriptor::constructor("(I)V"),
    );
    methods.insert(
        "<init>:(ILjava/math/RoundingMode;)V".to_string(),
        StubMethodDescriptor::constructor("(ILjava/math/RoundingMode;)V"),
    );
    methods.insert(
        "<init>:(Ljava/lang/String;)V".to_string(),
        StubMethodDescriptor::constructor("(Ljava/lang/String;)V"),
    );

    // Accessors
    methods.insert(
        "getPrecision:()I".to_string(),
        StubMethodDescriptor::instance_method("getPrecision", "()I"),
    );
    methods.insert(
        "getRoundingMode:()Ljava/math/RoundingMode;".to_string(),
        StubMethodDescriptor::instance_method("getRoundingMode", "()Ljava/math/RoundingMode;"),
    );

    // Object methods
    methods.insert(
        "equals:(Ljava/lang/Object;)Z".to_string(),
        StubMethodDescriptor::instance_method("equals", "(Ljava/lang/Object;)Z"),
    );
    methods.insert(
        "hashCode:()I".to_string(),
        StubMethodDescriptor::instance_method("hashCode", "()I"),
    );
    methods.insert(
        "toString:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("toString", "()Ljava/lang/String;"),
    );

    StubClass {
        name: "java/math/MathContext".to_string(),
        superclass: Some("java/lang/Object".to_string()),
        interfaces: vec!["java/io/Serializable".to_string()],
        methods,
    }
}

/// Create java.math.RoundingMode enum stub
pub fn create_rounding_mode_class() -> StubClass {
    let mut methods = HashMap::new();

    // Enum constants (UP, DOWN, CEILING, FLOOR, etc. are public static final fields)

    // Static methods
    methods.insert(
        "valueOf:(I)Ljava/math/RoundingMode;".to_string(),
        StubMethodDescriptor::static_method("valueOf", "(I)Ljava/math/RoundingMode;"),
    );
    methods.insert(
        "valueOf:(Ljava/lang/String;)Ljava/math/RoundingMode;".to_string(),
        StubMethodDescriptor::static_method(
            "valueOf",
            "(Ljava/lang/String;)Ljava/math/RoundingMode;",
        ),
    );
    methods.insert(
        "values:()[Ljava/math/RoundingMode;".to_string(),
        StubMethodDescriptor::static_method("values", "()[Ljava/math/RoundingMode;"),
    );

    // Instance methods (inherited from Enum)
    methods.insert(
        "name:()Ljava/lang/String;".to_string(),
        StubMethodDescriptor::instance_method("name", "()Ljava/lang/String;"),
    );
    methods.insert(
        "ordinal:()I".to_string(),
        StubMethodDescriptor::instance_method("ordinal", "()I"),
    );

    StubClass {
        name: "java/math/RoundingMode".to_string(),
        superclass: Some("java/lang/Enum".to_string()),
        interfaces: vec![],
        methods,
    }
}
