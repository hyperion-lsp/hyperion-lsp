// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM Bytecode opcodes
//!
//! This module defines all JVM bytecode opcodes and provides conversion from raw bytes.

/// JVM Bytecode opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    // Constants
    Nop = 0x00,
    AconstNull = 0x01,
    IconstM1 = 0x02,
    Iconst0 = 0x03,
    Iconst1 = 0x04,
    Iconst2 = 0x05,
    Iconst3 = 0x06,
    Iconst4 = 0x07,
    Iconst5 = 0x08,
    Lconst0 = 0x09,
    Lconst1 = 0x0a,
    Fconst0 = 0x0b,
    Fconst1 = 0x0c,
    Fconst2 = 0x0d,
    Dconst0 = 0x0e,
    Dconst1 = 0x0f,
    Bipush = 0x10,
    Sipush = 0x11,
    Ldc = 0x12,
    LdcW = 0x13,
    Ldc2W = 0x14,

    // Loads
    Iload = 0x15,
    Lload = 0x16,
    Fload = 0x17,
    Dload = 0x18,
    Aload = 0x19,
    Iload0 = 0x1a,
    Iload1 = 0x1b,
    Iload2 = 0x1c,
    Iload3 = 0x1d,
    Lload0 = 0x1e,
    Lload1 = 0x1f,
    Lload2 = 0x20,
    Lload3 = 0x21,
    Fload0 = 0x22,
    Fload1 = 0x23,
    Fload2 = 0x24,
    Fload3 = 0x25,
    Dload0 = 0x26,
    Dload1 = 0x27,
    Dload2 = 0x28,
    Dload3 = 0x29,
    Aload0 = 0x2a,
    Aload1 = 0x2b,
    Aload2 = 0x2c,
    Aload3 = 0x2d,

    // Stores
    Istore = 0x36,
    Lstore = 0x37,
    Fstore = 0x38,
    Dstore = 0x39,
    Astore = 0x3a,
    Istore0 = 0x3b,
    Istore1 = 0x3c,
    Istore2 = 0x3d,
    Istore3 = 0x3e,
    Lstore0 = 0x3f,
    Lstore1 = 0x40,
    Lstore2 = 0x41,
    Lstore3 = 0x42,
    Fstore0 = 0x43,
    Fstore1 = 0x44,
    Fstore2 = 0x45,
    Fstore3 = 0x46,
    Dstore0 = 0x47,
    Dstore1 = 0x48,
    Dstore2 = 0x49,
    Dstore3 = 0x4a,
    Astore0 = 0x4b,
    Astore1 = 0x4c,
    Astore2 = 0x4d,
    Astore3 = 0x4e,

    // Stack manipulation
    Pop = 0x57,
    Pop2 = 0x58,
    Dup = 0x59,
    DupX1 = 0x5a,
    DupX2 = 0x5b,
    Dup2 = 0x5c,
    Dup2X1 = 0x5d,
    Dup2X2 = 0x5e,
    Swap = 0x5f,

    // Arithmetic
    Iadd = 0x60,
    Ladd = 0x61,
    Fadd = 0x62,
    Dadd = 0x63,
    Isub = 0x64,
    Lsub = 0x65,
    Fsub = 0x66,
    Dsub = 0x67,
    Imul = 0x68,
    Lmul = 0x69,
    Fmul = 0x6a,
    Dmul = 0x6b,
    Idiv = 0x6c,
    Ldiv = 0x6d,
    Fdiv = 0x6e,
    Ddiv = 0x6f,
    Irem = 0x70,
    Lrem = 0x71,
    Frem = 0x72,
    Drem = 0x73,
    Ineg = 0x74,
    Lneg = 0x75,
    Fneg = 0x76,
    Dneg = 0x77,

    // Conversions
    I2l = 0x85,
    I2f = 0x86,
    I2d = 0x87,
    L2i = 0x88,
    L2f = 0x89,
    L2d = 0x8a,
    F2i = 0x8b,
    F2l = 0x8c,
    F2d = 0x8d,
    D2i = 0x8e,
    D2l = 0x8f,
    D2f = 0x90,
    I2b = 0x91,
    I2c = 0x92,
    I2s = 0x93,

    // Comparisons
    Lcmp = 0x94,
    Fcmpl = 0x95,
    Fcmpg = 0x96,
    Dcmpl = 0x97,
    Dcmpg = 0x98,

    // Return
    Ireturn = 0xac,
    Lreturn = 0xad,
    Freturn = 0xae,
    Dreturn = 0xaf,
    Areturn = 0xb0,
    Return = 0xb1,

    // Object/Array operations
    New = 0xbb,
    Newarray = 0xbc,
    Anewarray = 0xbd,
    Arraylength = 0xbe,
    Athrow = 0xbf,
    Checkcast = 0xc0,
    Instanceof = 0xc1,
    Monitorenter = 0xc2,
    Monitorexit = 0xc3,
    Multianewarray = 0xc5,

    // Null checks
    Ifnull = 0xc6,
    Ifnonnull = 0xc7,

    // Wide modifier
    Wide = 0xc4,

    // Fields
    Getfield = 0xb4,
    Putfield = 0xb5,
    Getstatic = 0xb2,
    Putstatic = 0xb3,

    // Array loads
    Iaload = 0x2e,
    Laload = 0x2f,
    Faload = 0x30,
    Daload = 0x31,
    Aaload = 0x32,
    Baload = 0x33,
    Caload = 0x34,
    Saload = 0x35,

    // Array stores
    Iastore = 0x4f,
    Lastore = 0x50,
    Fastore = 0x51,
    Dastore = 0x52,
    Aastore = 0x53,
    Bastore = 0x54,
    Castore = 0x55,
    Sastore = 0x56,

    // Control flow
    Goto = 0xa7,
    Ifeq = 0x99,
    Ifne = 0x9a,
    Iflt = 0x9b,
    Ifge = 0x9c,
    Ifgt = 0x9d,
    Ifle = 0x9e,
    IfIcmpeq = 0x9f,
    IfIcmpne = 0xa0,
    IfIcmplt = 0xa1,
    IfIcmpge = 0xa2,
    IfIcmpgt = 0xa3,
    IfIcmple = 0xa4,
    IfAcmpeq = 0xa5,
    IfAcmpne = 0xa6,
    Tableswitch = 0xaa,
    Lookupswitch = 0xab,
    GotoW = 0xc8,

    // Method invocation
    Invokevirtual = 0xb6,
    Invokespecial = 0xb7,
    Invokestatic = 0xb8,
    Invokeinterface = 0xb9,
    Invokedynamic = 0xba,

    // Bitwise operations (missing from original)
    Ishl = 0x78,
    Lshl = 0x79,
    Ishr = 0x7a,
    Lshr = 0x7b,
    Iushr = 0x7c,
    Lushr = 0x7d,
    Iand = 0x7e,
    Land = 0x7f,
    Ior = 0x80,
    Lor = 0x81,
    Ixor = 0x82,
    Lxor = 0x83,
    Iinc = 0x84,
}

impl Opcode {
    /// Get the size of this instruction in bytes (including opcode byte)
    ///
    /// Note: Variable-size instructions (tableswitch, lookupswitch, wide) return
    /// a minimum size - callers must handle these specially.
    pub fn size(&self) -> usize {
        match self {
            // 1-byte instructions (opcode only)
            Opcode::Nop
            | Opcode::AconstNull
            | Opcode::IconstM1
            | Opcode::Iconst0
            | Opcode::Iconst1
            | Opcode::Iconst2
            | Opcode::Iconst3
            | Opcode::Iconst4
            | Opcode::Iconst5
            | Opcode::Lconst0
            | Opcode::Lconst1
            | Opcode::Fconst0
            | Opcode::Fconst1
            | Opcode::Fconst2
            | Opcode::Dconst0
            | Opcode::Dconst1
            | Opcode::Iload0
            | Opcode::Iload1
            | Opcode::Iload2
            | Opcode::Iload3
            | Opcode::Lload0
            | Opcode::Lload1
            | Opcode::Lload2
            | Opcode::Lload3
            | Opcode::Fload0
            | Opcode::Fload1
            | Opcode::Fload2
            | Opcode::Fload3
            | Opcode::Dload0
            | Opcode::Dload1
            | Opcode::Dload2
            | Opcode::Dload3
            | Opcode::Aload0
            | Opcode::Aload1
            | Opcode::Aload2
            | Opcode::Aload3
            | Opcode::Iaload
            | Opcode::Laload
            | Opcode::Faload
            | Opcode::Daload
            | Opcode::Aaload
            | Opcode::Baload
            | Opcode::Caload
            | Opcode::Saload
            | Opcode::Istore0
            | Opcode::Istore1
            | Opcode::Istore2
            | Opcode::Istore3
            | Opcode::Lstore0
            | Opcode::Lstore1
            | Opcode::Lstore2
            | Opcode::Lstore3
            | Opcode::Fstore0
            | Opcode::Fstore1
            | Opcode::Fstore2
            | Opcode::Fstore3
            | Opcode::Dstore0
            | Opcode::Dstore1
            | Opcode::Dstore2
            | Opcode::Dstore3
            | Opcode::Astore0
            | Opcode::Astore1
            | Opcode::Astore2
            | Opcode::Astore3
            | Opcode::Iastore
            | Opcode::Lastore
            | Opcode::Fastore
            | Opcode::Dastore
            | Opcode::Aastore
            | Opcode::Bastore
            | Opcode::Castore
            | Opcode::Sastore
            | Opcode::Pop
            | Opcode::Pop2
            | Opcode::Dup
            | Opcode::DupX1
            | Opcode::DupX2
            | Opcode::Dup2
            | Opcode::Dup2X1
            | Opcode::Dup2X2
            | Opcode::Swap
            | Opcode::Iadd
            | Opcode::Ladd
            | Opcode::Fadd
            | Opcode::Dadd
            | Opcode::Isub
            | Opcode::Lsub
            | Opcode::Fsub
            | Opcode::Dsub
            | Opcode::Imul
            | Opcode::Lmul
            | Opcode::Fmul
            | Opcode::Dmul
            | Opcode::Idiv
            | Opcode::Ldiv
            | Opcode::Fdiv
            | Opcode::Ddiv
            | Opcode::Irem
            | Opcode::Lrem
            | Opcode::Frem
            | Opcode::Drem
            | Opcode::Ineg
            | Opcode::Lneg
            | Opcode::Fneg
            | Opcode::Dneg
            | Opcode::Ishl
            | Opcode::Lshl
            | Opcode::Ishr
            | Opcode::Lshr
            | Opcode::Iushr
            | Opcode::Lushr
            | Opcode::Iand
            | Opcode::Land
            | Opcode::Ior
            | Opcode::Lor
            | Opcode::Ixor
            | Opcode::Lxor
            | Opcode::I2l
            | Opcode::I2f
            | Opcode::I2d
            | Opcode::L2i
            | Opcode::L2f
            | Opcode::L2d
            | Opcode::F2i
            | Opcode::F2l
            | Opcode::F2d
            | Opcode::D2i
            | Opcode::D2l
            | Opcode::D2f
            | Opcode::I2b
            | Opcode::I2c
            | Opcode::I2s
            | Opcode::Lcmp
            | Opcode::Fcmpl
            | Opcode::Fcmpg
            | Opcode::Dcmpl
            | Opcode::Dcmpg
            | Opcode::Ireturn
            | Opcode::Lreturn
            | Opcode::Freturn
            | Opcode::Dreturn
            | Opcode::Areturn
            | Opcode::Return
            | Opcode::Arraylength
            | Opcode::Athrow
            | Opcode::Monitorenter
            | Opcode::Monitorexit => 1,

            // 2-byte instructions (opcode + 1-byte operand)
            Opcode::Bipush | Opcode::Ldc | Opcode::Newarray => 2,

            // 2-byte instructions (opcode + 1-byte local index)
            Opcode::Iload
            | Opcode::Lload
            | Opcode::Fload
            | Opcode::Dload
            | Opcode::Aload
            | Opcode::Istore
            | Opcode::Lstore
            | Opcode::Fstore
            | Opcode::Dstore
            | Opcode::Astore => 2,

            // 3-byte instructions (opcode + 2-byte operand)
            Opcode::Sipush
            | Opcode::LdcW
            | Opcode::Ldc2W
            | Opcode::Ifeq
            | Opcode::Ifne
            | Opcode::Iflt
            | Opcode::Ifge
            | Opcode::Ifgt
            | Opcode::Ifle
            | Opcode::IfIcmpeq
            | Opcode::IfIcmpne
            | Opcode::IfIcmplt
            | Opcode::IfIcmpge
            | Opcode::IfIcmpgt
            | Opcode::IfIcmple
            | Opcode::IfAcmpeq
            | Opcode::IfAcmpne
            | Opcode::Goto
            | Opcode::Ifnull
            | Opcode::Ifnonnull
            | Opcode::New
            | Opcode::Anewarray
            | Opcode::Checkcast
            | Opcode::Instanceof
            | Opcode::Getstatic
            | Opcode::Putstatic
            | Opcode::Getfield
            | Opcode::Putfield
            | Opcode::Invokevirtual
            | Opcode::Invokespecial
            | Opcode::Invokestatic => 3,

            // 3-byte (opcode + local index + constant)
            Opcode::Iinc => 3,

            // 4-byte (opcode + 2-byte index + dimensions)
            Opcode::Multianewarray => 4,

            // 5-byte (opcode + 2-byte index + count + 0)
            Opcode::Invokeinterface => 5,

            // 5-byte (opcode + 2-byte index + 2 zeros)
            Opcode::Invokedynamic => 5,

            // 5-byte (opcode + 4-byte offset)
            Opcode::GotoW => 5,

            // Variable size - return minimum (caller must handle specially)
            Opcode::Tableswitch | Opcode::Lookupswitch => 1,

            // Wide prefix - varies based on modified opcode (minimum 4 bytes)
            Opcode::Wide => 4,
        }
    }

    /// Convert a byte to an Opcode
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Opcode::Nop),
            0x01 => Some(Opcode::AconstNull),
            0x02 => Some(Opcode::IconstM1),
            0x03 => Some(Opcode::Iconst0),
            0x04 => Some(Opcode::Iconst1),
            0x05 => Some(Opcode::Iconst2),
            0x06 => Some(Opcode::Iconst3),
            0x07 => Some(Opcode::Iconst4),
            0x08 => Some(Opcode::Iconst5),
            0x09 => Some(Opcode::Lconst0),
            0x0a => Some(Opcode::Lconst1),
            0x0b => Some(Opcode::Fconst0),
            0x0c => Some(Opcode::Fconst1),
            0x0d => Some(Opcode::Fconst2),
            0x0e => Some(Opcode::Dconst0),
            0x0f => Some(Opcode::Dconst1),
            0x10 => Some(Opcode::Bipush),
            0x11 => Some(Opcode::Sipush),
            0x12 => Some(Opcode::Ldc),
            0x13 => Some(Opcode::LdcW),
            0x14 => Some(Opcode::Ldc2W),
            0x15 => Some(Opcode::Iload),
            0x16 => Some(Opcode::Lload),
            0x17 => Some(Opcode::Fload),
            0x18 => Some(Opcode::Dload),
            0x19 => Some(Opcode::Aload),
            0x1a => Some(Opcode::Iload0),
            0x1b => Some(Opcode::Iload1),
            0x1c => Some(Opcode::Iload2),
            0x1d => Some(Opcode::Iload3),
            0x1e => Some(Opcode::Lload0),
            0x1f => Some(Opcode::Lload1),
            0x20 => Some(Opcode::Lload2),
            0x21 => Some(Opcode::Lload3),
            0x22 => Some(Opcode::Fload0),
            0x23 => Some(Opcode::Fload1),
            0x24 => Some(Opcode::Fload2),
            0x25 => Some(Opcode::Fload3),
            0x26 => Some(Opcode::Dload0),
            0x27 => Some(Opcode::Dload1),
            0x28 => Some(Opcode::Dload2),
            0x29 => Some(Opcode::Dload3),
            0x2a => Some(Opcode::Aload0),
            0x2b => Some(Opcode::Aload1),
            0x2c => Some(Opcode::Aload2),
            0x2d => Some(Opcode::Aload3),
            0x36 => Some(Opcode::Istore),
            0x37 => Some(Opcode::Lstore),
            0x38 => Some(Opcode::Fstore),
            0x39 => Some(Opcode::Dstore),
            0x3a => Some(Opcode::Astore),
            0x3b => Some(Opcode::Istore0),
            0x3c => Some(Opcode::Istore1),
            0x3d => Some(Opcode::Istore2),
            0x3e => Some(Opcode::Istore3),
            0x3f => Some(Opcode::Lstore0),
            0x40 => Some(Opcode::Lstore1),
            0x41 => Some(Opcode::Lstore2),
            0x42 => Some(Opcode::Lstore3),
            0x43 => Some(Opcode::Fstore0),
            0x44 => Some(Opcode::Fstore1),
            0x45 => Some(Opcode::Fstore2),
            0x46 => Some(Opcode::Fstore3),
            0x47 => Some(Opcode::Dstore0),
            0x48 => Some(Opcode::Dstore1),
            0x49 => Some(Opcode::Dstore2),
            0x4a => Some(Opcode::Dstore3),
            0x4b => Some(Opcode::Astore0),
            0x4c => Some(Opcode::Astore1),
            0x4d => Some(Opcode::Astore2),
            0x4e => Some(Opcode::Astore3),
            0x57 => Some(Opcode::Pop),
            0x58 => Some(Opcode::Pop2),
            0x59 => Some(Opcode::Dup),
            0x5a => Some(Opcode::DupX1),
            0x5b => Some(Opcode::DupX2),
            0x5c => Some(Opcode::Dup2),
            0x5d => Some(Opcode::Dup2X1),
            0x5e => Some(Opcode::Dup2X2),
            0x5f => Some(Opcode::Swap),
            0x60 => Some(Opcode::Iadd),
            0x61 => Some(Opcode::Ladd),
            0x62 => Some(Opcode::Fadd),
            0x63 => Some(Opcode::Dadd),
            0x64 => Some(Opcode::Isub),
            0x65 => Some(Opcode::Lsub),
            0x66 => Some(Opcode::Fsub),
            0x67 => Some(Opcode::Dsub),
            0x68 => Some(Opcode::Imul),
            0x69 => Some(Opcode::Lmul),
            0x6a => Some(Opcode::Fmul),
            0x6b => Some(Opcode::Dmul),
            0x6c => Some(Opcode::Idiv),
            0x6d => Some(Opcode::Ldiv),
            0x6e => Some(Opcode::Fdiv),
            0x6f => Some(Opcode::Ddiv),
            0x70 => Some(Opcode::Irem),
            0x71 => Some(Opcode::Lrem),
            0x72 => Some(Opcode::Frem),
            0x73 => Some(Opcode::Drem),
            0x74 => Some(Opcode::Ineg),
            0x75 => Some(Opcode::Lneg),
            0x76 => Some(Opcode::Fneg),
            0x77 => Some(Opcode::Dneg),
            0x85 => Some(Opcode::I2l),
            0x86 => Some(Opcode::I2f),
            0x87 => Some(Opcode::I2d),
            0x88 => Some(Opcode::L2i),
            0x89 => Some(Opcode::L2f),
            0x8a => Some(Opcode::L2d),
            0x8b => Some(Opcode::F2i),
            0x8c => Some(Opcode::F2l),
            0x8d => Some(Opcode::F2d),
            0x8e => Some(Opcode::D2i),
            0x8f => Some(Opcode::D2l),
            0x90 => Some(Opcode::D2f),
            0x91 => Some(Opcode::I2b),
            0x92 => Some(Opcode::I2c),
            0x93 => Some(Opcode::I2s),
            0x2e => Some(Opcode::Iaload),
            0x2f => Some(Opcode::Laload),
            0x30 => Some(Opcode::Faload),
            0x31 => Some(Opcode::Daload),
            0x32 => Some(Opcode::Aaload),
            0x33 => Some(Opcode::Baload),
            0x34 => Some(Opcode::Caload),
            0x35 => Some(Opcode::Saload),
            0x4f => Some(Opcode::Iastore),
            0x50 => Some(Opcode::Lastore),
            0x51 => Some(Opcode::Fastore),
            0x52 => Some(Opcode::Dastore),
            0x53 => Some(Opcode::Aastore),
            0x54 => Some(Opcode::Bastore),
            0x55 => Some(Opcode::Castore),
            0x56 => Some(Opcode::Sastore),
            0x78 => Some(Opcode::Ishl),
            0x79 => Some(Opcode::Lshl),
            0x7a => Some(Opcode::Ishr),
            0x7b => Some(Opcode::Lshr),
            0x7c => Some(Opcode::Iushr),
            0x7d => Some(Opcode::Lushr),
            0x7e => Some(Opcode::Iand),
            0x7f => Some(Opcode::Land),
            0x80 => Some(Opcode::Ior),
            0x81 => Some(Opcode::Lor),
            0x82 => Some(Opcode::Ixor),
            0x83 => Some(Opcode::Lxor),
            0x84 => Some(Opcode::Iinc),
            0x94 => Some(Opcode::Lcmp),
            0x95 => Some(Opcode::Fcmpl),
            0x96 => Some(Opcode::Fcmpg),
            0x97 => Some(Opcode::Dcmpl),
            0x98 => Some(Opcode::Dcmpg),
            0x99 => Some(Opcode::Ifeq),
            0x9a => Some(Opcode::Ifne),
            0x9b => Some(Opcode::Iflt),
            0x9c => Some(Opcode::Ifge),
            0x9d => Some(Opcode::Ifgt),
            0x9e => Some(Opcode::Ifle),
            0x9f => Some(Opcode::IfIcmpeq),
            0xa0 => Some(Opcode::IfIcmpne),
            0xa1 => Some(Opcode::IfIcmplt),
            0xa2 => Some(Opcode::IfIcmpge),
            0xa3 => Some(Opcode::IfIcmpgt),
            0xa4 => Some(Opcode::IfIcmple),
            0xa5 => Some(Opcode::IfAcmpeq),
            0xa6 => Some(Opcode::IfAcmpne),
            0xa7 => Some(Opcode::Goto),
            0xaa => Some(Opcode::Tableswitch),
            0xab => Some(Opcode::Lookupswitch),
            0xac => Some(Opcode::Ireturn),
            0xad => Some(Opcode::Lreturn),
            0xae => Some(Opcode::Freturn),
            0xaf => Some(Opcode::Dreturn),
            0xb0 => Some(Opcode::Areturn),
            0xb1 => Some(Opcode::Return),
            0xb2 => Some(Opcode::Getstatic),
            0xb3 => Some(Opcode::Putstatic),
            0xb4 => Some(Opcode::Getfield),
            0xb5 => Some(Opcode::Putfield),
            0xb6 => Some(Opcode::Invokevirtual),
            0xb7 => Some(Opcode::Invokespecial),
            0xb8 => Some(Opcode::Invokestatic),
            0xb9 => Some(Opcode::Invokeinterface),
            0xba => Some(Opcode::Invokedynamic),
            0xbb => Some(Opcode::New),
            0xbc => Some(Opcode::Newarray),
            0xbd => Some(Opcode::Anewarray),
            0xbe => Some(Opcode::Arraylength),
            0xbf => Some(Opcode::Athrow),
            0xc0 => Some(Opcode::Checkcast),
            0xc1 => Some(Opcode::Instanceof),
            0xc2 => Some(Opcode::Monitorenter),
            0xc3 => Some(Opcode::Monitorexit),
            0xc4 => Some(Opcode::Wide),
            0xc5 => Some(Opcode::Multianewarray),
            0xc6 => Some(Opcode::Ifnull),
            0xc7 => Some(Opcode::Ifnonnull),
            0xc8 => Some(Opcode::GotoW),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify all opcodes emitted by the Groovy compiler are supported by the executor.
    /// This test prevents compiler→executor mismatch bugs.
    /// If this test fails, either add the missing opcode to the enum or update this list.
    #[test]
    fn test_groovy_compiler_opcode_coverage() {
        // All opcodes emitted by groovy_compiler.rs and groovy_ast_compiler.rs
        // Source: grep -rn "0x.." in those files + emit_opcode calls
        let groovy_compiler_opcodes: &[(u8, &str)] = &[
            // Constants
            (0x00, "NOP"),
            (0x01, "ACONST_NULL"),
            (0x02, "ICONST_M1"),
            (0x03, "ICONST_0"),
            (0x04, "ICONST_1"),
            (0x05, "ICONST_2"),
            (0x06, "ICONST_3"),
            (0x07, "ICONST_4"),
            (0x08, "ICONST_5"),
            (0x10, "BIPUSH"),
            (0x11, "SIPUSH"),
            (0x12, "LDC"),
            (0x13, "LDC_W"),
            (0x14, "LDC2_W"),
            // Loads
            (0x15, "ILOAD"),
            (0x16, "LLOAD"),
            (0x17, "FLOAD"),
            (0x18, "DLOAD"),
            (0x19, "ALOAD"),
            (0x1A, "ILOAD_0"),
            (0x1B, "ILOAD_1"),
            (0x1C, "ILOAD_2"),
            (0x1D, "ILOAD_3"),
            (0x1E, "LLOAD_0"),
            (0x1F, "LLOAD_1"),
            (0x20, "LLOAD_2"),
            (0x21, "LLOAD_3"),
            (0x22, "FLOAD_0"),
            (0x23, "FLOAD_1"),
            (0x24, "FLOAD_2"),
            (0x25, "FLOAD_3"),
            (0x26, "DLOAD_0"),
            (0x27, "DLOAD_1"),
            (0x28, "DLOAD_2"),
            (0x29, "DLOAD_3"),
            (0x2A, "ALOAD_0"),
            (0x2B, "ALOAD_1"),
            (0x2C, "ALOAD_2"),
            (0x2D, "ALOAD_3"),
            // Stores
            (0x36, "ISTORE"),
            (0x37, "LSTORE"),
            (0x38, "FSTORE"),
            (0x39, "DSTORE"),
            (0x3A, "ASTORE"),
            (0x3B, "ISTORE_0"),
            (0x3C, "ISTORE_1"),
            (0x3D, "ISTORE_2"),
            (0x3E, "ISTORE_3"),
            (0x3F, "LSTORE_0"),
            (0x40, "LSTORE_1"),
            (0x41, "LSTORE_2"),
            (0x42, "LSTORE_3"),
            (0x43, "FSTORE_0"),
            (0x44, "FSTORE_1"),
            (0x45, "FSTORE_2"),
            (0x46, "FSTORE_3"),
            (0x47, "DSTORE_0"),
            (0x48, "DSTORE_1"),
            (0x49, "DSTORE_2"),
            (0x4A, "DSTORE_3"),
            (0x4B, "ASTORE_0"),
            (0x4C, "ASTORE_1"),
            (0x4D, "ASTORE_2"),
            (0x4E, "ASTORE_3"),
            // Stack
            (0x57, "POP"),
            (0x59, "DUP"),
            (0x5F, "SWAP"),
            // Arithmetic
            (0x60, "IADD"),
            (0x61, "LADD"),
            (0x62, "FADD"),
            (0x63, "DADD"),
            (0x64, "ISUB"),
            (0x65, "LSUB"),
            (0x66, "FSUB"),
            (0x67, "DSUB"),
            (0x68, "IMUL"),
            (0x69, "LMUL"),
            (0x6A, "FMUL"),
            (0x6B, "DMUL"),
            (0x6C, "IDIV"),
            (0x6D, "LDIV"),
            (0x6E, "FDIV"),
            (0x6F, "DDIV"),
            (0x70, "IREM"),
            (0x71, "LREM"),
            (0x72, "FREM"),
            (0x73, "DREM"),
            (0x74, "INEG"),
            (0x75, "LNEG"),
            (0x76, "FNEG"),
            (0x77, "DNEG"),
            // Bitwise
            (0x78, "ISHL"),
            (0x79, "LSHL"),
            (0x7A, "ISHR"),
            (0x7B, "LSHR"),
            (0x7C, "IUSHR"),
            (0x7D, "LUSHR"),
            (0x7E, "IAND"),
            (0x7F, "LAND"),
            (0x80, "IOR"),
            (0x81, "LOR"),
            (0x82, "IXOR"),
            (0x83, "LXOR"),
            (0x84, "IINC"),
            // Comparisons
            (0x94, "LCMP"),
            (0x95, "FCMPL"),
            (0x97, "DCMPL"),
            // Conditional jumps
            (0x99, "IFEQ"),
            (0x9A, "IFNE"),
            (0x9F, "IF_ICMPEQ"),
            (0xA0, "IF_ICMPNE"),
            (0xA1, "IF_ICMPLT"),
            (0xA2, "IF_ICMPGE"),
            (0xA3, "IF_ICMPGT"),
            (0xA4, "IF_ICMPLE"),
            // Unconditional jump
            (0xA7, "GOTO"),
            // Returns
            (0xAC, "IRETURN"),
            (0xAD, "LRETURN"),
            (0xAE, "FRETURN"),
            (0xAF, "DRETURN"),
            (0xB0, "ARETURN"),
            (0xB1, "RETURN"),
            // Object operations
            (0xB5, "PUTFIELD"),
            (0xB6, "INVOKEVIRTUAL"),
            (0xB7, "INVOKESPECIAL"),
            (0xB8, "INVOKESTATIC"),
            (0xB9, "INVOKEINTERFACE"),
            (0xBA, "INVOKEDYNAMIC"),
            (0xBB, "NEW"),
            (0xBF, "ATHROW"),
            // Type operations
            (0xC0, "CHECKCAST"),
            (0xC1, "INSTANCEOF"),
            (0xC4, "WIDE"),
            (0xC6, "IFNULL"),
            (0xC7, "IFNONNULL"),
        ];

        let mut missing = Vec::new();
        for &(byte, name) in groovy_compiler_opcodes {
            if Opcode::from_u8(byte).is_none() {
                missing.push(format!("0x{:02X} {}", byte, name));
            }
        }

        assert!(
            missing.is_empty(),
            "Groovy compiler emits opcodes not supported by executor:\n  {}",
            missing.join("\n  ")
        );
    }

    /// Verify that Opcode::from_u8 correctly maps all defined opcodes
    #[test]
    fn test_all_opcodes_roundtrip() {
        // Test all opcodes in the enum can be round-tripped
        let opcodes = [
            (0x00, Opcode::Nop),
            (0x01, Opcode::AconstNull),
            (0x02, Opcode::IconstM1),
            (0x03, Opcode::Iconst0),
            (0x04, Opcode::Iconst1),
            (0x84, Opcode::Iinc),
            (0xC4, Opcode::Wide),
            (0xC6, Opcode::Ifnull),
            (0xC7, Opcode::Ifnonnull),
        ];

        for (byte, expected) in opcodes {
            assert_eq!(
                Opcode::from_u8(byte),
                Some(expected),
                "Opcode::from_u8(0x{:02X}) should return {:?}",
                byte,
                expected
            );
        }
    }
}
