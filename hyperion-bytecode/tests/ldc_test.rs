// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

#![allow(clippy::approx_constant)]

use hyperion_bytecode::interpreter::executor::Executor;
use hyperion_bytecode::interpreter::frame::Frame;
use hyperion_bytecode::interpreter::heap::Heap;
use hyperion_bytecode::interpreter::monitor::MonitorTable;
use hyperion_bytecode::interpreter::thread::Thread;
use hyperion_bytecode::interpreter::value::Value;
use hyperion_bytecode::producer::ConstantPoolItem;

type VMResult<T> = Result<T, hyperion_bytecode::interpreter::errors::VMError>;

#[test]
fn test_ldc_integer() -> VMResult<()> {
    // LDC with integer constant
    let bytecode = vec![
        0x12, // ldc
        0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Integer(42),         // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    // Execute ldc
    executor.step(&mut thread, &mut heap, &mut monitors)?;

    // Check that integer was pushed on the stack
    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_int()?, 42);
    Ok(())
}

#[test]
fn test_ldc_float() -> VMResult<()> {
    // LDC with float constant
    let bytecode = vec![
        0x12, // ldc
        0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Float(3.14),         // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_float()?, 3.14);
    Ok(())
}

#[test]
fn test_ldc_string() -> VMResult<()> {
    // LDC with string constant
    let bytecode = vec![
        0x12, // ldc
        0x01, // cp_index = 1 (points to String entry)
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::String { string_index: 2 }, // CP index 1: String entry
        ConstantPoolItem::Utf8("Hello, World!".to_string()), // CP index 2: UTF8 value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    // Verify it's a reference (interned string)
    assert!(matches!(result, Value::Reference(_)));
    Ok(())
}

#[test]
fn test_ldc_class() -> VMResult<()> {
    // LDC with class constant
    let bytecode = vec![
        0x12, // ldc
        0x01, // cp_index = 1 (points to Class entry)
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Class { name_index: 2 }, // CP index 1: Class entry
        ConstantPoolItem::Utf8("java/lang/String".to_string()), // CP index 2: Class name
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    // Verify it's a reference (Class object)
    assert!(matches!(result, Value::Reference(_)));
    Ok(())
}

#[test]
fn test_ldc_multiple_constants() -> VMResult<()> {
    // Test loading multiple different constants
    let bytecode = vec![
        0x12, 0x01, // ldc index 1 (integer 10)
        0x12, 0x02, // ldc index 2 (integer 20)
        0x60, // iadd
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Integer(10),         // CP index 1
        ConstantPoolItem::Integer(20),         // CP index 2
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    // Execute ldc, ldc, iadd
    executor.step(&mut thread, &mut heap, &mut monitors)?; // ldc 10
    executor.step(&mut thread, &mut heap, &mut monitors)?; // ldc 20
    executor.step(&mut thread, &mut heap, &mut monitors)?; // iadd

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_int()?, 30);
    Ok(())
}

#[test]
fn test_ldc_negative_integer() -> VMResult<()> {
    let bytecode = vec![
        0x12, // ldc
        0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Integer(-12345),     // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_int()?, -12345);
    Ok(())
}

#[test]
fn test_ldc_zero_index_error() -> VMResult<()> {
    // CP index 0 is reserved and should error
    let bytecode = vec![
        0x12, // ldc
        0x00, // cp_index = 0 (invalid!)
    ];

    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // placeholder
        ConstantPoolItem::Integer(42),
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    let result = executor.step(&mut thread, &mut heap, &mut monitors);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_ldc_out_of_bounds() -> VMResult<()> {
    // CP index beyond pool size should error
    let bytecode = vec![
        0x12, // ldc
        0x05, // cp_index = 5 (out of bounds)
    ];

    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // placeholder
        ConstantPoolItem::Integer(42),
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    let result = executor.step(&mut thread, &mut heap, &mut monitors);
    assert!(result.is_err());
    Ok(())
}

#[test]
fn test_ldc_w_integer() -> VMResult<()> {
    // LDC_W with integer constant (2-byte index)
    let bytecode = vec![
        0x13, // ldc_w
        0x00, 0x01, // cp_index = 1 (2 bytes)
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Integer(12345),      // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_int()?, 12345);
    Ok(())
}

#[test]
fn test_ldc_w_string() -> VMResult<()> {
    // LDC_W with string constant (2-byte index)
    let bytecode = vec![
        0x13, // ldc_w
        0x00, 0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::String { string_index: 2 }, // CP index 1: String entry
        ConstantPoolItem::Utf8("Wide index string".to_string()), // CP index 2: UTF8 value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert!(matches!(result, Value::Reference(_)));
    Ok(())
}

#[test]
fn test_ldc2_w_long() -> VMResult<()> {
    // LDC2_W with long constant
    let bytecode = vec![
        0x14, // ldc2_w
        0x00, 0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Long(9876543210_i64), // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_long()?, 9876543210_i64);
    Ok(())
}

#[test]
fn test_ldc2_w_double() -> VMResult<()> {
    // LDC2_W with double constant
    let bytecode = vec![
        0x14, // ldc2_w
        0x00, 0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Double(3.141592653589793), // CP index 1: actual value
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    executor.step(&mut thread, &mut heap, &mut monitors)?;

    let current_frame = thread.current_frame().expect("frame should exist");
    let result = current_frame.peek()?;

    assert_eq!(result.as_double()?, 3.141592653589793);
    Ok(())
}

#[test]
fn test_ldc2_w_invalid_type() -> VMResult<()> {
    // LDC2_W should fail with non-long/double constant
    let bytecode = vec![
        0x14, // ldc2_w
        0x00, 0x01, // cp_index = 1
    ];

    // JVM constant pool is 1-indexed, index 0 is unused
    let constant_pool = vec![
        ConstantPoolItem::Utf8(String::new()), // CP index 0: placeholder
        ConstantPoolItem::Integer(42),         // CP index 1: Invalid for ldc2_w
    ];

    let mut executor = Executor::new(bytecode).with_constant_pool(constant_pool);
    let mut thread = Thread::new(10, None);
    let frame = Frame::new("Test".to_string(), "test".to_string(), 5, 10, vec![])?;
    thread.push_frame(frame)?;

    let mut heap = Heap::new(1024);
    let mut monitors = MonitorTable::new();

    let result = executor.step(&mut thread, &mut heap, &mut monitors);
    assert!(result.is_err());
    Ok(())
}
