// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! VM error types

use thiserror::Error;

/// Result type for VM operations
pub type VMResult<T> = Result<T, VMError>;

/// JVM interpreter errors
#[derive(Debug, Error)]
pub enum VMError {
    #[error("Class not found: {0}")]
    ClassNotFound(String),

    /// Per JVM Spec §12.4.1: thrown when class initialization previously failed
    #[error("NoClassDefFoundError: {0}")]
    NoClassDefFoundError(String),

    #[error("No such method: {0}")]
    NoSuchMethod(String),

    #[error("No such field: {0}")]
    NoSuchField(String),

    #[error("Stack overflow")]
    StackOverflow,

    #[error("Stack underflow")]
    StackUnderflow,

    #[error("Null pointer exception")]
    NullPointerException,

    #[error("Array index out of bounds: index={index}, length={length}")]
    ArrayIndexOutOfBounds { index: i32, length: i32 },

    #[error("String index out of bounds: index={index}, length={length}")]
    StringIndexOutOfBounds { index: i32, length: i32 },

    #[error("Negative array size: {0}")]
    NegativeArraySize(i32),

    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: String, got: String },

    #[error("ClassCastException: cannot cast {from} to {to}")]
    ClassCastException { from: String, to: String },

    #[error("Arithmetic exception: {0}")]
    ArithmeticException(String),

    #[error("Number format exception: {0}")]
    NumberFormatException(String),

    #[error("Illegal argument exception: {0}")]
    IllegalArgumentException(String),

    #[error("Illegal state exception: {0}")]
    IllegalStateException(String),

    #[error("No such element: {0}")]
    NoSuchElementException(String),

    #[error("Unsupported operation: {0}")]
    UnsupportedOperationException(String),

    #[error("Pattern syntax exception: {0}")]
    PatternSyntaxException(String),

    #[error("Execution limit exceeded")]
    ExecutionLimitExceeded,

    #[error("Execution timeout exceeded")]
    ExecutionTimeout,

    #[error("Java exception")]
    JavaException,

    #[error("Unsatisfied link error: {0}")]
    UnsatisfiedLinkError(String),

    #[error("Class format error: {0}")]
    ClassFormatError(String),

    #[error("Invalid bytecode")]
    InvalidBytecode,

    #[error("Invalid program counter")]
    InvalidProgramCounter,

    #[error("Invalid jump target")]
    InvalidJumpTarget,

    #[error("Invalid local variable index: {0}")]
    InvalidLocalIndex(u16),

    #[error("Exception in initializer: {0}")]
    ExceptionInInitializer(String),

    #[error("Unimplemented opcode: {0}")]
    UnimplementedOpcode(String),

    #[error("Unimplemented: {0}")]
    Unimplemented(String),

    #[error("Memory limit exceeded")]
    MemoryLimitExceeded,

    #[error("Class initialization in progress")]
    ClassInitializationCycle,

    #[error("No active frame")]
    NoActiveFrame,

    #[error("Invalid constant pool index: {0}")]
    InvalidConstantPoolIndex(u16),

    #[error("Incompatible class change error: {0}")]
    IncompatibleClassChangeError(String),

    #[error("Instantiation error: {0}")]
    InstantiationError(String),

    #[error("Abstract method error: {0}")]
    AbstractMethodError(String),

    #[error("Illegal access error: {0}")]
    IllegalAccessError(String),

    #[error("Illegal monitor state: {0}")]
    IllegalMonitorStateException(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Internal VM error: {0}")]
    InternalError(String),

    #[error("Bootstrap method error: {0}")]
    BootstrapMethodError(String),

    #[error("Exception thrown: {0:?}")]
    ThrownException(crate::interpreter::value::ObjectRef),

    #[error("Uncaught exception: {exception_class}\n{stack_trace}")]
    UncaughtException {
        exception_class: String,
        stack_trace: String,
        /// Every Java exception has a real heap identity, including exceptions
        /// synthesized by the VM. Nested runners transport that identity back
        /// to the enclosing instruction without rematerializing it.
        exception: crate::interpreter::value::ObjectRef,
    },

    #[error("Array store exception: cannot store {value_type} in {array_type}[]")]
    ArrayStoreException {
        array_type: String,
        value_type: String,
    },

    #[error("Bytecode verification error: {0}")]
    VerifyError(String),
}

impl VMError {
    /// Create a type mismatch error
    pub fn type_mismatch(expected: &str, got: &str) -> Self {
        VMError::TypeMismatch {
            expected: expected.to_string(),
            got: got.to_string(),
        }
    }

    /// Create a ClassCastException
    pub fn class_cast_exception(from: &str, to: &str) -> Self {
        VMError::ClassCastException {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    /// Create an internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        VMError::InternalError(msg.into())
    }

    /// Create a negative array size error
    pub fn negative_array_size() -> Self {
        VMError::NegativeArraySize(-1)
    }

    /// Create an out of memory error
    pub fn out_of_memory() -> Self {
        VMError::MemoryLimitExceeded
    }

    /// Create an invalid object reference error
    pub fn invalid_object_ref() -> Self {
        VMError::NullPointerException
    }

    /// Create an invalid field offset error
    pub fn invalid_field_offset() -> Self {
        VMError::InternalError("Invalid field offset".to_string())
    }

    /// Create an array index out of bounds error
    pub fn array_index_out_of_bounds(index: i32, length: usize) -> Self {
        VMError::ArrayIndexOutOfBounds {
            index,
            length: i32::try_from(length).unwrap_or(i32::MAX),
        }
    }

    /// Create a null pointer exception
    pub fn null_pointer() -> Self {
        VMError::NullPointerException
    }

    /// Create an illegal monitor state exception
    pub fn illegal_monitor_state(msg: impl Into<String>) -> Self {
        VMError::IllegalMonitorStateException(msg.into())
    }

    /// Check if this error represents a catchable Java exception
    pub fn is_catchable(&self) -> bool {
        matches!(
            self,
            VMError::NullPointerException
                | VMError::ArithmeticException(_)
                | VMError::NumberFormatException(_)
                | VMError::IllegalArgumentException(_)
                | VMError::IllegalStateException(_)
                | VMError::NoSuchElementException(_)
                | VMError::UnsupportedOperationException(_)
                | VMError::PatternSyntaxException(_)
                | VMError::ArrayIndexOutOfBounds { .. }
                | VMError::StringIndexOutOfBounds { .. }
                | VMError::NegativeArraySize(_)
                | VMError::ClassCastException { .. }
                | VMError::ArrayStoreException { .. }
                | VMError::IllegalMonitorStateException(_)
                | VMError::NoClassDefFoundError(_)
                | VMError::NoSuchMethod(_)
                | VMError::NoSuchField(_)
                | VMError::UnsatisfiedLinkError(_)
                | VMError::ClassFormatError(_)
                | VMError::ExceptionInInitializer(_)
                | VMError::IncompatibleClassChangeError(_)
                | VMError::InstantiationError(_)
                | VMError::AbstractMethodError(_)
                | VMError::IllegalAccessError(_)
                | VMError::BootstrapMethodError(_)
                | VMError::StackOverflow
                | VMError::MemoryLimitExceeded
                | VMError::VerifyError(_)
                | VMError::UncaughtException { .. }
        )
    }

    /// Get the Java exception class name for catchable exceptions
    pub fn exception_class(&self) -> String {
        match self {
            VMError::NullPointerException => "java/lang/NullPointerException".to_string(),
            VMError::ArithmeticException(_) => "java/lang/ArithmeticException".to_string(),
            VMError::NumberFormatException(_) => "java/lang/NumberFormatException".to_string(),
            VMError::IllegalArgumentException(_) => {
                "java/lang/IllegalArgumentException".to_string()
            }
            VMError::IllegalStateException(_) => "java/lang/IllegalStateException".to_string(),
            VMError::NoSuchElementException(_) => "java/util/NoSuchElementException".to_string(),
            VMError::UnsupportedOperationException(_) => {
                "java/lang/UnsupportedOperationException".to_string()
            }
            VMError::PatternSyntaxException(_) => {
                "java/util/regex/PatternSyntaxException".to_string()
            }
            VMError::ArrayIndexOutOfBounds { .. } => {
                "java/lang/ArrayIndexOutOfBoundsException".to_string()
            }
            VMError::StringIndexOutOfBounds { .. } => {
                "java/lang/StringIndexOutOfBoundsException".to_string()
            }
            VMError::NegativeArraySize(_) => "java/lang/NegativeArraySizeException".to_string(),
            VMError::ClassCastException { .. } => "java/lang/ClassCastException".to_string(),
            VMError::ArrayStoreException { .. } => "java/lang/ArrayStoreException".to_string(),
            VMError::IllegalMonitorStateException(_) => {
                "java/lang/IllegalMonitorStateException".to_string()
            }
            VMError::NoClassDefFoundError(_) => "java/lang/NoClassDefFoundError".to_string(),
            VMError::NoSuchMethod(_) => "java/lang/NoSuchMethodError".to_string(),
            VMError::NoSuchField(_) => "java/lang/NoSuchFieldError".to_string(),
            VMError::UnsatisfiedLinkError(_) => "java/lang/UnsatisfiedLinkError".to_string(),
            VMError::ClassFormatError(_) => "java/lang/ClassFormatError".to_string(),
            VMError::ExceptionInInitializer(_) => {
                "java/lang/ExceptionInInitializerError".to_string()
            }
            VMError::IncompatibleClassChangeError(_) => {
                "java/lang/IncompatibleClassChangeError".to_string()
            }
            VMError::InstantiationError(_) => "java/lang/InstantiationError".to_string(),
            VMError::AbstractMethodError(_) => "java/lang/AbstractMethodError".to_string(),
            VMError::IllegalAccessError(_) => "java/lang/IllegalAccessError".to_string(),
            VMError::BootstrapMethodError(_) => "java/lang/BootstrapMethodError".to_string(),
            VMError::VerifyError(_) => "java/lang/VerifyError".to_string(),
            VMError::StackOverflow => "java/lang/StackOverflowError".to_string(),
            VMError::MemoryLimitExceeded => "java/lang/OutOfMemoryError".to_string(),
            VMError::UncaughtException {
                exception_class, ..
            } => exception_class.clone(),
            _ => "java/lang/RuntimeException".to_string(),
        }
    }

    /// Detail message used when a VM error is materialized as a Java object.
    /// Structural prefixes in `Display` are intentionally excluded.
    pub fn java_message(&self) -> Option<String> {
        match self {
            VMError::NullPointerException
            | VMError::StackOverflow
            | VMError::MemoryLimitExceeded => None,
            VMError::ArrayIndexOutOfBounds { index, length } => {
                Some(format!("Index {index} out of bounds for length {length}"))
            }
            VMError::StringIndexOutOfBounds { index, length } => Some(format!(
                "String index out of range: {index} (length {length})"
            )),
            VMError::NegativeArraySize(length) => Some(length.to_string()),
            VMError::ClassCastException { from, to } => {
                Some(format!("{from} cannot be cast to {to}"))
            }
            VMError::ArrayStoreException {
                array_type,
                value_type,
            } => Some(format!("cannot store {value_type} in {array_type}[]")),
            VMError::ArithmeticException(message)
            | VMError::NumberFormatException(message)
            | VMError::IllegalArgumentException(message)
            | VMError::IllegalStateException(message)
            | VMError::NoSuchElementException(message)
            | VMError::UnsupportedOperationException(message)
            | VMError::PatternSyntaxException(message)
            | VMError::NoClassDefFoundError(message)
            | VMError::NoSuchMethod(message)
            | VMError::NoSuchField(message)
            | VMError::UnsatisfiedLinkError(message)
            | VMError::ClassFormatError(message)
            | VMError::ExceptionInInitializer(message)
            | VMError::IncompatibleClassChangeError(message)
            | VMError::InstantiationError(message)
            | VMError::AbstractMethodError(message)
            | VMError::IllegalAccessError(message)
            | VMError::IllegalMonitorStateException(message)
            | VMError::BootstrapMethodError(message)
            | VMError::VerifyError(message) => Some(message.clone()),
            VMError::UncaughtException { .. } | VMError::ThrownException(_) => None,
            _ => Some(self.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VMError;

    #[test]
    fn verify_error_is_a_catchable_java_error() {
        let error = VMError::VerifyError("bad stack map".to_string());
        assert!(error.is_catchable());
        assert_eq!(error.exception_class(), "java/lang/VerifyError");
    }

    #[test]
    fn mapped_linkage_and_vm_errors_are_catchable() {
        let cases = [
            (
                VMError::NoSuchField("C.x:I".to_string()),
                "java/lang/NoSuchFieldError",
            ),
            (
                VMError::NoSuchMethod("C.m()V".to_string()),
                "java/lang/NoSuchMethodError",
            ),
            (
                VMError::IncompatibleClassChangeError("kind".to_string()),
                "java/lang/IncompatibleClassChangeError",
            ),
            (
                VMError::IllegalAccessError("access".to_string()),
                "java/lang/IllegalAccessError",
            ),
            (
                VMError::AbstractMethodError("abstract".to_string()),
                "java/lang/AbstractMethodError",
            ),
            (
                VMError::UnsatisfiedLinkError("native".to_string()),
                "java/lang/UnsatisfiedLinkError",
            ),
            (
                VMError::ClassFormatError("format".to_string()),
                "java/lang/ClassFormatError",
            ),
            (
                VMError::BootstrapMethodError("bootstrap".to_string()),
                "java/lang/BootstrapMethodError",
            ),
            (VMError::StackOverflow, "java/lang/StackOverflowError"),
            (VMError::MemoryLimitExceeded, "java/lang/OutOfMemoryError"),
        ];
        for (error, expected_class) in cases {
            assert!(error.is_catchable(), "{error} should be catchable");
            assert_eq!(error.exception_class(), expected_class);
        }
    }
}
