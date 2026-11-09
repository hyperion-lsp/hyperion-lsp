// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_opcode_from_u8() {
    assert_eq!(Opcode::from_u8(0x00), Some(Opcode::Nop));
    assert_eq!(Opcode::from_u8(0x01), Some(Opcode::AconstNull));
    assert_eq!(Opcode::from_u8(0x60), Some(Opcode::Iadd));
    assert_eq!(Opcode::from_u8(0xff), None);
}

#[test]
fn test_executor_creation() {
    let bytecode = vec![0x00, 0xb1]; // nop, return
    let vm = TestVMBuilder::new()
        .with_bytecode(bytecode)
        .build()
        .unwrap();
    assert_eq!(vm.pc(), 0);
}

#[test]
fn test_wide_iload() -> VMResult<()> {
    // Test: wide iload with 16-bit index 300
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc4, // 0: wide
            0x15, // 1: iload
            0x01, 0x2c, // 2-3: index 300 (0x012c)
            0xac, // 4: ireturn
        ])
        .with_max_locals(500)
        .with_max_stack(10)
        .with_heap_size(1024 * 1024)
        .build()?;

    // Set local 300 to value 42
    vm.set_local(300, Value::Int(42))?;

    // Step through: wide iload
    vm.step()?;

    // Check stack has the value
    let val = vm.pop()?.as_int()?;
    assert_eq!(val, 42, "wide iload should load from local 300");

    Ok(())
}

#[test]
fn test_wide_istore() -> VMResult<()> {
    // Test: wide istore with 16-bit index 400
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x63, // 0-1: bipush 99
            0xc4, // 2: wide
            0x36, // 3: istore
            0x01, 0x90, // 4-5: index 400 (0x0190)
            0xb1, // 6: return
        ])
        .with_max_locals(500)
        .with_max_stack(10)
        .with_heap_size(1024 * 1024)
        .build()?;

    // Step through: bipush 99
    vm.step()?;
    // Step through: wide istore
    vm.step()?;

    // Check local 400 has the value
    let val = vm.local(400)?.as_int()?;
    assert_eq!(val, 99, "wide istore should store to local 400");

    Ok(())
}

#[test]
fn test_wide_iinc() -> VMResult<()> {
    // Test: wide iinc with 16-bit index and 16-bit const
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc4, // 0: wide
            0x84, // 1: iinc
            0x01, 0x00, // 2-3: index 256 (0x0100)
            0x01, 0xf4, // 4-5: const 500 (0x01f4)
            0xb1, // 6: return
        ])
        .with_max_locals(500)
        .with_max_stack(10)
        .with_heap_size(1024 * 1024)
        .build()?;

    // Set local 256 to value 100
    vm.set_local(256, Value::Int(100))?;

    // Step through: wide iinc
    vm.step()?;

    // Check local 256 is now 600 (100 + 500)
    let val = vm.local(256)?.as_int()?;
    assert_eq!(val, 600, "wide iinc should increment local 256 by 500");

    Ok(())
}

#[test]
fn test_wide_aload() -> VMResult<()> {
    // Test: wide aload with 16-bit index 350
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc4, // 0: wide
            0x19, // 1: aload
            0x01, 0x5e, // 2-3: index 350 (0x015e)
            0xb0, // 4: areturn
        ])
        .with_max_locals(500)
        .with_max_stack(10)
        .with_heap_size(1024 * 1024)
        .build()?;

    // Set local 350 to a null reference
    vm.set_local(350, Value::Reference(ObjectRef::Null))?;

    // Step through: wide aload
    vm.step()?;

    // Check stack has null reference
    let val = vm.pop()?;
    assert!(
        matches!(val, Value::Reference(ObjectRef::Null)),
        "wide aload should load null from local 350"
    );

    Ok(())
}

// ============================================================
// Multianewarray Validation Tests
// ============================================================

#[test]
fn test_access_flags_constants() {
    // Verify access flag constants match JVMS 4.6
    assert_eq!(access_flags::ACC_PUBLIC, 0x0001);
    assert_eq!(access_flags::ACC_PRIVATE, 0x0002);
    assert_eq!(access_flags::ACC_PROTECTED, 0x0004);
    assert_eq!(access_flags::ACC_STATIC, 0x0008);
    assert_eq!(access_flags::ACC_SYNCHRONIZED, 0x0020);
}
