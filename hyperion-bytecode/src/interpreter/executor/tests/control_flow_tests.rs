// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

use super::*;

#[test]
fn test_ifeq_branch_taken() -> VMResult<()> {
    // Bytecode: iconst_0, ifeq +3 (skip to nop at position 5), iconst_1, return, nop
    // Position: 0       , 1      2    3                      , 4       , 5
    // If ifeq takes branch, we skip iconst_1 and land on nop at position 5
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x03, // 0: iconst_0 (push 0)
            0x99, 0x00, 0x04, // 1: ifeq +4 (branch to position 1+4=5 if top == 0)
            0x04, // 4: iconst_1 (push 1) - skipped if branch taken
            0x00, // 5: nop (landing pad)
        ])
        .build()?;

    vm.step()?; // iconst_0 - push 0
    assert_eq!(vm.pc(), 1);

    vm.step()?; // ifeq - should branch since 0 == 0
    assert_eq!(vm.pc(), 5); // Jumped to nop

    // Stack should be empty (0 was popped by ifeq)
    assert!(vm.pop().is_err());

    Ok(())
}

#[test]
fn test_ifeq_branch_not_taken() -> VMResult<()> {
    // Bytecode: iconst_1, ifeq +3, iconst_2, nop
    // If value != 0, branch not taken, continue to iconst_2
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0x99, 0x00, 0x04, // 1: ifeq +4 (would go to position 5, but branch not taken)
            0x05, // 4: iconst_2 (push 2) - executed because 1 != 0
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // ifeq - branch NOT taken (1 != 0)
    assert_eq!(vm.pc(), 4); // Continues to next instruction

    vm.step()?; // iconst_2
    assert_eq!(vm.stack_top()?.as_int()?, 2);

    Ok(())
}

#[test]
fn test_ifne_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1, non-zero)
            0x9a, 0x00, 0x04, // 1: ifne +4 (branch if != 0)
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // ifne - branch taken (1 != 0)
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_iflt_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x02, // 0: iconst_m1 (push -1)
            0x9b, 0x00, 0x04, // 1: iflt +4 (branch if < 0)
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_m1
    vm.step()?; // iflt - branch taken (-1 < 0)
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_ifge_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x03, // 0: iconst_0 (push 0)
            0x9c, 0x00, 0x04, // 1: ifge +4 (branch if >= 0)
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_0
    vm.step()?; // ifge - branch taken (0 >= 0)
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_ifgt_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0x9d, 0x00, 0x04, // 1: ifgt +4 (branch if > 0)
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // ifgt - branch taken (1 > 0)
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_ifle_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x02, // 0: iconst_m1 (push -1)
            0x9e, 0x00, 0x04, // 1: ifle +4 (branch if <= 0)
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop
        ])
        .build()?;

    vm.step()?; // iconst_m1
    vm.step()?; // ifle - branch taken (-1 <= 0)
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_if_icmpeq_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x05, // 0: iconst_2 (push 2)
            0x05, // 1: iconst_2 (push 2)
            0x9f, 0x00, 0x04, // 2: if_icmpeq +4 (branch if v1 == v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_2
    vm.step()?; // iconst_2
    vm.step()?; // if_icmpeq - branch taken (2 == 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_icmpne_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0x05, // 1: iconst_2 (push 2)
            0xa0, 0x00, 0x04, // 2: if_icmpne +4 (branch if v1 != v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // iconst_2
    vm.step()?; // if_icmpne - branch taken (1 != 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_icmplt_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0x05, // 1: iconst_2 (push 2)
            0xa1, 0x00, 0x04, // 2: if_icmplt +4 (branch if v1 < v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // iconst_2
    vm.step()?; // if_icmplt - branch taken (1 < 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_icmpge_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x05, // 0: iconst_2 (push 2)
            0x05, // 1: iconst_2 (push 2)
            0xa2, 0x00, 0x04, // 2: if_icmpge +4 (branch if v1 >= v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_2
    vm.step()?; // iconst_2
    vm.step()?; // if_icmpge - branch taken (2 >= 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_icmpgt_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x06, // 0: iconst_3 (push 3)
            0x05, // 1: iconst_2 (push 2)
            0xa3, 0x00, 0x04, // 2: if_icmpgt +4 (branch if v1 > v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_3
    vm.step()?; // iconst_2
    vm.step()?; // if_icmpgt - branch taken (3 > 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_icmple_branch_taken() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0x05, // 1: iconst_2 (push 2)
            0xa4, 0x00, 0x04, // 2: if_icmple +4 (branch if v1 <= v2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // iconst_2
    vm.step()?; // if_icmple - branch taken (1 <= 2)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_acmpeq_null_null() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x01, // 0: aconst_null
            0x01, // 1: aconst_null
            0xa5, 0x00, 0x04, // 2: if_acmpeq +4 (branch if ref1 == ref2)
            0x04, // 5: iconst_1 - skipped
            0x00, // 6: nop
        ])
        .build()?;

    vm.step()?; // aconst_null
    vm.step()?; // aconst_null
    vm.step()?; // if_acmpeq - branch taken (null == null)
    assert_eq!(vm.pc(), 6);

    Ok(())
}

#[test]
fn test_if_acmpne_branch_taken() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;
    use crate::interpreter::value::ObjectRef;

    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xa6, 0x00, 0x04, // 0: if_acmpne +4 (branch if ref1 != ref2)
            0x04, // 3: iconst_1 - skipped
            0x00, // 4: nop
        ])
        .with_heap_size(1024 * 1024)
        .build()?;

    // Create two different array references
    let array1 = vm.heap_mut().allocate_array(ArrayElementType::Int, 5)?;
    let array2 = vm.heap_mut().allocate_array(ArrayElementType::Int, 5)?;

    // Push two different references
    vm.push(Value::Reference(ObjectRef::Array(array1)))?;
    vm.push(Value::Reference(ObjectRef::Array(array2)))?;

    vm.step()?; // if_acmpne - branch taken (array1 != array2)
    assert_eq!(vm.pc(), 4);

    Ok(())
}

#[test]
fn test_goto() -> VMResult<()> {
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xa7, 0x00, 0x05, // 0: goto +5 (jump to position 5)
            0x04, // 3: iconst_1 - skipped
            0x05, // 4: iconst_2 - skipped
            0x00, // 5: nop (landing)
        ])
        .build()?;

    vm.step()?; // goto
    assert_eq!(vm.pc(), 5);

    Ok(())
}

#[test]
fn test_backward_branch() -> VMResult<()> {
    // Test backward branch (negative offset) - common in loops
    // Bytecode layout:
    // 0: nop (target of backward branch)
    // 1: iconst_0
    // 2: goto -2 (jump back to position 0)
    // Offset is relative to the opcode position.
    // Opcode at position 2, offset -2 means target = 2 + (-2) = 0

    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x00, // 0: nop (landing pad)
            0x03, // 1: iconst_0
            0xa7, 0xff, 0xfe, // 2: goto -2 (offset = -2 in i16, target = 2 + (-2) = 0)
        ])
        .build()?;

    vm.step()?; // nop, pc = 1
    vm.step()?; // iconst_0, pc = 2
    vm.step()?; // goto -2, should go to pc = 0
    assert_eq!(vm.pc(), 0);

    Ok(())
}

#[test]
fn test_goto_w() -> VMResult<()> {
    // Test goto_w with a large 4-byte offset
    // Bytecode layout:
    // 0: nop (start)
    // 1: goto_w (0xC8)
    // 2-5: offset (4 bytes) = +1000 (to jump over padding)
    // 6-1005: padding (nops)
    // 1006: nop (landing pad)
    //
    // Opcode at position 1, 4-byte offset reads bytes 2-5
    // After executing goto_w, pc will be at 6 (1 + 4 + 1)
    // Target = (pc at 6) - 5 + offset = 1 + 1000 = 1001... wait
    // Let's recalculate: opcode is at 1, after reading 4 offset bytes we're at 6
    // Opcode position = 6 - 5 = 1
    // So target = 1 + offset
    // We want to jump to 1006, so offset = 1006 - 1 = 1005

    let mut bytecode = vec![0x00]; // 0: nop
    bytecode.push(0xC8); // 1: goto_w

    // Add 4-byte offset (1005 in big-endian)
    let offset = 1005i32;
    bytecode.extend_from_slice(&offset.to_be_bytes());

    // Pad bytes 6-1005 with NOPs
    while bytecode.len() < 1006 {
        bytecode.push(0x00);
    }

    // At byte 1006, add landing pad
    bytecode.push(0x00); // nop

    let mut vm = TestVMBuilder::new().with_bytecode(bytecode).build()?;

    vm.step()?; // nop, pc = 1
    vm.step()?; // goto_w, should jump to 1006
    assert_eq!(vm.pc(), 1006);

    Ok(())
}

#[test]
fn test_simple_loop() -> VMResult<()> {
    // Simulate a simple loop: int i = 3; while (i > 0) { i--; }
    // Bytecode positions:
    // 0: iconst_3          ; push 3
    // 1: istore_0          ; store to local 0 (i = 3)
    // 2: iload_0           ; load i (loop start)
    // 3-5: ifle +10        ; if i <= 0, exit loop (jump to position 3+10=13)
    // 6: iload_0           ; load i
    // 7: iconst_1          ; push 1
    // 8: isub              ; i - 1
    // 9: istore_0          ; store back (i--)
    // 10-12: goto -8       ; jump back to position 10+(-8)=2
    // 13: return           ; end

    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x06, // 0: iconst_3
            0x3b, // 1: istore_0
            0x1a, // 2: iload_0 (loop start)
            0x9e, 0x00, 0x0a, // 3: ifle +10 (to position 13)
            0x1a, // 6: iload_0
            0x04, // 7: iconst_1
            0x64, // 8: isub
            0x3b, // 9: istore_0
            0xa7, 0xff, 0xf8, // 10: goto -8 (to position 2)
            0xb1, // 13: return
        ])
        .with_max_frames(100)
        .build()?;

    // Run until we hit return (position 13)
    let mut iterations = 0;
    while vm.pc() != 13 && iterations < 50 {
        vm.step()?;
        iterations += 1;
    }

    // Verify we reached the return
    assert_eq!(vm.pc(), 13);

    // Verify i == 0
    let i = vm.local(0)?.as_int()?;
    assert_eq!(i, 0);

    Ok(())
}

#[test]
fn test_tableswitch_in_range() -> VMResult<()> {
    // Test tableswitch with value in range [0, 2]
    // Switch on value 1, should jump to offset at index 1
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x04, // 0: iconst_1 (push 1)
            0xaa, // 1: tableswitch
            0x00, 0x00, // 2-3: padding (align to offset 4)
            0x00, 0x00, 0x00, 0x0c, // 4-7: default offset = +12 (to position 13)
            0x00, 0x00, 0x00, 0x00, // 8-11: low = 0
            0x00, 0x00, 0x00, 0x02, // 12-15: high = 2
            0x00, 0x00, 0x00, 0x14, // 16-19: offset[0] = +20 (to position 21)
            0x00, 0x00, 0x00, 0x17, // 20-23: offset[1] = +23 (to position 24)
            0x00, 0x00, 0x00, 0x1a, // 24-27: offset[2] = +26 (to position 27)
            0x00, // 28: nop (default target at 13 = 1 + 12)
                  // But we should jump to offset[1] = position 24 = 1 + 23
        ])
        .build()?;

    vm.step()?; // iconst_1
    vm.step()?; // tableswitch

    // Should jump to offset[1] = 1 + 23 = 24
    assert_eq!(vm.pc(), 24);

    Ok(())
}

#[test]
fn test_tableswitch_out_of_range() -> VMResult<()> {
    // Test tableswitch with value outside range [0, 2]
    // Switch on value 5, should jump to default offset
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x08, // 0: iconst_5 (push 5)
            0xaa, // 1: tableswitch
            0x00, 0x00, // 2-3: padding
            0x00, 0x00, 0x00, 0x14, // 4-7: default offset = +20 (to position 21)
            0x00, 0x00, 0x00, 0x00, // 8-11: low = 0
            0x00, 0x00, 0x00, 0x02, // 12-15: high = 2
            0x00, 0x00, 0x00, 0x0c, // 16-19: offset[0] = +12
            0x00, 0x00, 0x00, 0x0f, // 20-23: offset[1] = +15
            0x00, 0x00, 0x00, 0x12, // 24-27: offset[2] = +18
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // iconst_5
    vm.step()?; // tableswitch

    // Should jump to default: 1 + 20 = 21
    assert_eq!(vm.pc(), 21);

    Ok(())
}

#[test]
fn test_tableswitch_at_low_boundary() -> VMResult<()> {
    // Test tableswitch with value exactly at low boundary
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x0a, // 0-1: bipush 10 (push 10)
            0xaa, // 2: tableswitch
            0x00, // 3: padding (align to offset 4)
            0x00, 0x00, 0x00, 0x0e, // 4-7: default offset = +14 (to position 16)
            0x00, 0x00, 0x00, 0x0a, // 8-11: low = 10
            0x00, 0x00, 0x00, 0x0c, // 12-15: high = 12
            0x00, 0x00, 0x00, 0x16, // 16-19: offset[0] = +22 (to position 24)
            0x00, 0x00, 0x00, 0x19, // 20-23: offset[1] = +25
            0x00, 0x00, 0x00, 0x1c, // 24-27: offset[2] = +28
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // bipush 10
    vm.step()?; // tableswitch

    // Should jump to offset[0] (10 - 10 = 0): 2 + 22 = 24
    assert_eq!(vm.pc(), 24);

    Ok(())
}

#[test]
fn test_tableswitch_at_high_boundary() -> VMResult<()> {
    // Test tableswitch with value exactly at high boundary
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x0c, // 0-1: bipush 12 (push 12)
            0xaa, // 2: tableswitch
            0x00, // 3: padding
            0x00, 0x00, 0x00, 0x0e, // 4-7: default offset = +14
            0x00, 0x00, 0x00, 0x0a, // 8-11: low = 10
            0x00, 0x00, 0x00, 0x0c, // 12-15: high = 12
            0x00, 0x00, 0x00, 0x16, // 16-19: offset[0] = +22
            0x00, 0x00, 0x00, 0x19, // 20-23: offset[1] = +25
            0x00, 0x00, 0x00, 0x1c, // 24-27: offset[2] = +28 (to position 30)
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // bipush 12
    vm.step()?; // tableswitch

    // Should jump to offset[2] (12 - 10 = 2): 2 + 28 = 30
    assert_eq!(vm.pc(), 30);

    Ok(())
}

#[test]
fn test_tableswitch_negative_range() -> VMResult<()> {
    // Test tableswitch with negative values in range
    // This tests that signed integers are handled correctly
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0xfe, // 0-1: bipush -2 (push -2)
            0xaa, // 2: tableswitch
            0x00, // 3: padding (align to offset 4)
            0x00, 0x00, 0x00, 0x0e, // 4-7: default offset = +14 (to position 16)
            0xff, 0xff, 0xff, 0xfd, // 8-11: low = -3
            0xff, 0xff, 0xff, 0xff, // 12-15: high = -1
            0x00, 0x00, 0x00, 0x16, // 16-19: offset[0] = +22 (for -3, to position 24)
            0x00, 0x00, 0x00, 0x19, // 20-23: offset[1] = +25 (for -2, to position 27)
            0x00, 0x00, 0x00, 0x1c, // 24-27: offset[2] = +28 (for -1, to position 30)
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // bipush -2
    vm.step()?; // tableswitch

    // Should jump to offset[1] (-2 - (-3) = 1): 2 + 25 = 27
    assert_eq!(vm.pc(), 27);

    Ok(())
}

#[test]
fn test_tableswitch_alignment_with_no_padding() -> VMResult<()> {
    // Test tableswitch when opcode is already at 4-byte aligned position
    // Opcode at position 3 needs 0 bytes of padding (already aligned at 4)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x03, // 0: iconst_0
            0x03, // 1: iconst_0
            0x03, // 2: iconst_0
            0xaa, // 3: tableswitch (3+1=4, already aligned - no padding needed)
            // Data starts at position 4 (aligned)
            0x00, 0x00, 0x00, 0x0e, // 4-7: default offset = +14 (to position 17)
            0x00, 0x00, 0x00, 0x00, // 8-11: low = 0
            0x00, 0x00, 0x00, 0x01, // 12-15: high = 1
            0x00, 0x00, 0x00, 0x15, // 16-19: offset[0] = +21 (to position 24)
            0x00, 0x00, 0x00, 0x18, // 20-23: offset[1] = +24 (to position 27)
            0x00, // 24: nop
        ])
        .build()?;

    vm.step()?; // iconst_0
    vm.step()?; // iconst_0
    vm.step()?; // iconst_0 (push value to switch on)
    vm.step()?; // tableswitch

    // Should jump to offset[0] (0 - 0 = 0): 3 + 21 = 24
    assert_eq!(vm.pc(), 24);

    Ok(())
}

// ============================================================
// Lookupswitch Tests
// ============================================================

#[test]
fn test_lookupswitch_found() -> VMResult<()> {
    // Test lookupswitch with matching key
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x64, // 0-1: bipush 100 (push 100)
            0xab, // 2: lookupswitch
            0x00, // 3: padding (align to offset 4)
            0x00, 0x00, 0x00, 0x12, // 4-7: default offset = +18 (to position 20)
            0x00, 0x00, 0x00, 0x02, // 8-11: npairs = 2
            0x00, 0x00, 0x00, 0x64, // 12-15: match[0] = 100
            0x00, 0x00, 0x00, 0x1a, // 16-19: offset[0] = +26 (to position 28)
            0x00, 0x00, 0x00, 0xc8, // 20-23: match[1] = 200
            0x00, 0x00, 0x00, 0x1d, // 24-27: offset[1] = +29
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // bipush 100
    vm.step()?; // lookupswitch

    // Should jump to offset[0]: 2 + 26 = 28
    assert_eq!(vm.pc(), 28);

    Ok(())
}

#[test]
fn test_lookupswitch_not_found() -> VMResult<()> {
    // Test lookupswitch with non-matching key
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x32, // 0-1: bipush 50 (push 50)
            0xab, // 2: lookupswitch
            0x00, // 3: padding
            0x00, 0x00, 0x00, 0x12, // 4-7: default offset = +18 (to position 20)
            0x00, 0x00, 0x00, 0x02, // 8-11: npairs = 2
            0x00, 0x00, 0x00, 0x64, // 12-15: match[0] = 100
            0x00, 0x00, 0x00, 0x1a, // 16-19: offset[0] = +26
            0x00, 0x00, 0x00, 0xc8, // 20-23: match[1] = 200
            0x00, 0x00, 0x00, 0x1d, // 24-27: offset[1] = +29
            0x00, // 28: nop
        ])
        .build()?;

    vm.step()?; // bipush 50
    vm.step()?; // lookupswitch

    // Should jump to default: 2 + 18 = 20
    assert_eq!(vm.pc(), 20);

    Ok(())
}

#[test]
fn test_lookupswitch_multiple_pairs() -> VMResult<()> {
    // Test lookupswitch with multiple pairs, match second one
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x10, 0x32, // 0-1: bipush 50 (push 50)
            0xab, // 2: lookupswitch
            0x00, // 3: padding
            0x00, 0x00, 0x00, 0x1e, // 4-7: default offset = +30 (to position 32)
            0x00, 0x00, 0x00, 0x03, // 8-11: npairs = 3
            0x00, 0x00, 0x00, 0x0a, // 12-15: match[0] = 10
            0x00, 0x00, 0x00, 0x26, // 16-19: offset[0] = +38
            0x00, 0x00, 0x00, 0x32, // 20-23: match[1] = 50
            0x00, 0x00, 0x00, 0x29, // 24-27: offset[1] = +41 (to position 43)
            0x00, 0x00, 0x00, 0x64, // 28-31: match[2] = 100
            0x00, 0x00, 0x00, 0x2c, // 32-35: offset[2] = +44
            0x00, // 36: nop
        ])
        .build()?;

    vm.step()?; // bipush 50
    vm.step()?; // lookupswitch

    // Should jump to offset[1]: 2 + 41 = 43
    assert_eq!(vm.pc(), 43);

    Ok(())
}

// ============================================================
// Object Creation Tests
// ============================================================

#[test]
fn test_ifnull_branches_on_null() -> VMResult<()> {
    // Test: push null, ifnull should branch
    // JVM branch offsets are relative to the opcode position (not PC after reading operands)
    // ifnull at offset 1, target = 1 + 5 = 6 (iconst_1)
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x01, // 0: aconst_null
            0xc6, 0x00, 0x05, // 1: ifnull +5 (branch to offset 6)
            0x03, // 4: iconst_0 (if not branched)
            0xac, // 5: ireturn
            0x04, // 6: iconst_1 (branch target)
            0xac, // 7: ireturn
        ])
        .with_heap_size(1024 * 1024)
        .build()?;

    // Step through: aconst_null
    vm.step()?;
    // Step through: ifnull - should branch
    vm.step()?;

    // After branch, PC should point to iconst_1 (offset 6)
    assert_eq!(
        vm.pc(),
        6,
        "ifnull should branch to offset 6 when stack has null"
    );

    Ok(())
}

#[test]
fn test_ifnull_no_branch_on_non_null() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;
    use crate::interpreter::value::ObjectRef;

    // Test: push non-null array ref, ifnull should NOT branch
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc6, 0x00, 0x04, // 0: ifnull +4 (would branch to offset 4 if null)
            0x03, // 3: iconst_0
            0xac, // 4: ireturn
        ])
        .with_heap_size(1024 * 1024)
        .build()?;

    // Push a non-null array reference
    let array_id = vm.heap_mut().allocate_array(ArrayElementType::Int, 5)?;
    vm.push(Value::Reference(ObjectRef::Array(array_id)))?;

    // Step through: ifnull - should NOT branch
    vm.step()?;

    // PC should advance past ifnull (3 bytes) to offset 3
    assert_eq!(
        vm.pc(),
        3,
        "ifnull should NOT branch when stack has non-null"
    );

    Ok(())
}

#[test]
fn test_ifnonnull_branches_on_non_null() -> VMResult<()> {
    use crate::interpreter::heap::ArrayElementType;
    use crate::interpreter::value::ObjectRef;

    // Test: push non-null ref, ifnonnull should branch
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0xc7, 0x00, 0x04, // 0: ifnonnull +4 (branch to offset 4)
            0x03, // 3: iconst_0
            0xac, // 4: ireturn
        ])
        .with_heap_size(1024 * 1024)
        .build()?;

    // Push a non-null array reference
    let array_id = vm.heap_mut().allocate_array(ArrayElementType::Int, 5)?;
    vm.push(Value::Reference(ObjectRef::Array(array_id)))?;

    // Step through: ifnonnull - should branch
    vm.step()?;

    // After branch, PC should point to offset 4
    assert_eq!(
        vm.pc(),
        4,
        "ifnonnull should branch when stack has non-null"
    );

    Ok(())
}

#[test]
fn test_ifnonnull_no_branch_on_null() -> VMResult<()> {
    // Test: push null, ifnonnull should NOT branch
    let mut vm = TestVMBuilder::new()
        .with_bytecode(vec![
            0x01, // 0: aconst_null
            0xc7, 0x00, 0x04, // 1: ifnonnull +4 (would branch to offset 5 if non-null)
            0x03, // 4: iconst_0
            0xac, // 5: ireturn
        ])
        .with_heap_size(1024 * 1024)
        .build()?;

    // Step through: aconst_null
    vm.step()?;
    // Step through: ifnonnull - should NOT branch
    vm.step()?;

    // PC should advance past ifnonnull (3 bytes) to offset 4
    assert_eq!(
        vm.pc(),
        4,
        "ifnonnull should NOT branch when stack has null"
    );

    Ok(())
}

// =========================================================================
// Wide Opcode Tests (JVMS 6.5.wide)
// =========================================================================
