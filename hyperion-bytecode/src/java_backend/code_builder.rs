// Copyright 2025 Adrian Helvik
// SPDX-License-Identifier: PolyForm-Shield-1.0.0

//! JVM instruction buffer, operand-stack accounting, and label fixups.

use crate::producer::{BytecodeError, BytecodeResult};

/// A label id into [`CodeBuilder::labels`].
pub(super) type Label = usize;

pub(super) struct CodeBuilder {
    code: Vec<u8>,
    /// Label id to resolved byte offset (`None` until placed).
    labels: Vec<Option<u32>>,
    /// Operand position, branch-opcode position, and target label.
    fixups: Vec<(usize, usize, Label)>,
    exceptions: Vec<ExceptionRegion>,
    /// Operand position, relative base, and target for four-byte branches.
    wide_fixups: Vec<(usize, usize, Label)>,
    cur_stack: i32,
    max_stack: i32,
    stack_error: Option<String>,
}

/// A protected range `[start, end)` with the sub-ranges it leaves out, its handler, and the
/// constant-pool index of the caught class (0 catches everything).
struct ExceptionRegion {
    start: Label,
    end: Label,
    gaps: Vec<(Label, Label)>,
    handler: Label,
    catch_type: u16,
}

pub(super) struct ExceptionEntry {
    pub(super) start_pc: u16,
    pub(super) end_pc: u16,
    pub(super) handler_pc: u16,
    pub(super) catch_type: u16,
}

impl CodeBuilder {
    pub(super) fn new() -> Self {
        Self {
            code: Vec::new(),
            labels: Vec::new(),
            fixups: Vec::new(),
            exceptions: Vec::new(),
            wide_fixups: Vec::new(),
            cur_stack: 0,
            max_stack: 0,
            stack_error: None,
        }
    }

    /// Protects `[start, end)` except the `gaps`, which become holes between several
    /// exception-table entries.
    pub(super) fn add_exception(
        &mut self,
        start: Label,
        end: Label,
        gaps: &[(Label, Label)],
        handler: Label,
        catch_type: u16,
    ) {
        self.exceptions.push(ExceptionRegion {
            start,
            end,
            gaps: gaps.to_vec(),
            handler,
            catch_type,
        });
    }

    pub(super) fn new_label(&mut self) -> Label {
        self.labels.push(None);
        self.labels.len() - 1
    }

    pub(super) fn place(&mut self, label: Label) {
        self.labels[label] = Some(self.code.len() as u32);
    }

    pub(super) fn op(&mut self, opcode: u8) {
        self.code.push(opcode);
    }

    pub(super) fn u8(&mut self, value: u8) {
        self.code.push(value);
    }

    pub(super) fn u16(&mut self, value: u16) {
        self.code.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn i16(&mut self, value: i16) {
        self.code.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn u32(&mut self, value: u32) {
        self.code.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn i32(&mut self, value: i32) {
        self.code.extend_from_slice(&value.to_be_bytes());
    }

    pub(super) fn wide_branch(&mut self, base: usize, label: Label) {
        let operand_pos = self.code.len();
        self.code.extend_from_slice(&[0, 0, 0, 0]);
        self.wide_fixups.push((operand_pos, base, label));
    }

    pub(super) fn branch(&mut self, opcode: u8, label: Label) {
        let opcode_pos = self.code.len();
        self.code.push(opcode);
        let operand_pos = self.code.len();
        self.code.extend_from_slice(&[0, 0]);
        self.fixups.push((operand_pos, opcode_pos, label));
    }

    pub(super) fn push(&mut self, slots: i32) {
        if slots < 0 {
            self.stack_error
                .get_or_insert_with(|| format!("negative operand stack push: {slots}"));
            return;
        }
        let Some(next) = self.cur_stack.checked_add(slots) else {
            self.stack_error
                .get_or_insert_with(|| "operand stack depth overflow".to_string());
            return;
        };
        self.cur_stack = next;
        self.max_stack = self.max_stack.max(self.cur_stack);
    }

    pub(super) fn pop(&mut self, slots: i32) {
        if slots < 0 {
            self.stack_error
                .get_or_insert_with(|| format!("negative operand stack pop: {slots}"));
        } else if slots > self.cur_stack {
            self.stack_error.get_or_insert_with(|| {
                format!(
                    "operand stack underflow: attempted to pop {slots} slot(s) with {} available",
                    self.cur_stack
                )
            });
            self.cur_stack = 0;
        } else {
            self.cur_stack -= slots;
        }
    }

    pub(super) fn stack_depth(&self) -> i32 {
        self.cur_stack
    }

    pub(super) fn restore_stack_depth(&mut self, depth: i32) {
        self.cur_stack = depth;
    }

    pub(super) fn clear_stack(&mut self) {
        self.cur_stack = 0;
    }

    pub(super) fn code_len(&self) -> usize {
        self.code.len()
    }

    pub(super) fn max_stack(&self) -> i32 {
        self.max_stack
    }

    /// Resolve all branch fixups and return code plus exception-table entries.
    pub(super) fn finish(
        mut self,
        method_name: &str,
    ) -> BytecodeResult<(Vec<u8>, Vec<ExceptionEntry>)> {
        if let Some(message) = self.stack_error.take() {
            return Err(BytecodeError::InternalError { message });
        }
        if self.code.len() > u16::MAX as usize {
            return Err(BytecodeError::CodeSizeExceeded {
                method: method_name.to_string(),
            });
        }
        if self.max_stack > i32::from(u16::MAX) {
            return Err(BytecodeError::InternalError {
                message: format!(
                    "operand stack depth {} exceeds the JVM max_stack limit in {method_name}",
                    self.max_stack
                ),
            });
        }
        for (operand_pos, opcode_pos, label) in &self.fixups {
            let target = self.labels[*label].ok_or_else(|| BytecodeError::InternalError {
                message: format!("unresolved branch label {label}"),
            })?;
            let offset = target as i64 - *opcode_pos as i64;
            let offset = i16::try_from(offset).map_err(|_| BytecodeError::InternalError {
                message: "branch offset exceeds 16 bits (wide branches not supported)".to_string(),
            })?;
            let bytes = offset.to_be_bytes();
            self.code[*operand_pos] = bytes[0];
            self.code[*operand_pos + 1] = bytes[1];
        }

        let resolve = |label: Label| -> BytecodeResult<u16> {
            let offset = self.labels[label].ok_or_else(|| BytecodeError::InternalError {
                message: format!("unresolved exception-region label {label}"),
            })?;
            u16::try_from(offset).map_err(|_| BytecodeError::InternalError {
                message: "exception-region offset exceeds 16 bits".to_string(),
            })
        };
        for (operand_pos, base, label) in &self.wide_fixups {
            let target = self.labels[*label].ok_or_else(|| BytecodeError::InternalError {
                message: format!("unresolved switch label {label}"),
            })?;
            let offset = (target as i64 - *base as i64) as i32;
            self.code[*operand_pos..*operand_pos + 4].copy_from_slice(&offset.to_be_bytes());
        }

        let mut table = Vec::with_capacity(self.exceptions.len());
        for region in &self.exceptions {
            let handler_pc = resolve(region.handler)?;
            let end = resolve(region.end)?;
            let mut gaps = region
                .gaps
                .iter()
                .map(|(gap_start, gap_end)| Ok((resolve(*gap_start)?, resolve(*gap_end)?)))
                .collect::<BytecodeResult<Vec<_>>>()?;
            gaps.sort_unstable();
            let mut start = resolve(region.start)?;
            for (gap_start, gap_end) in gaps.into_iter().chain([(end, end)]) {
                let segment_end = gap_start.min(end);
                if start < segment_end {
                    table.push(ExceptionEntry {
                        start_pc: start,
                        end_pc: segment_end,
                        handler_pc,
                        catch_type: region.catch_type,
                    });
                }
                start = start.max(gap_end);
            }
        }
        Ok((self.code, table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_rejects_stack_accounting_underflow() {
        let mut builder = CodeBuilder::new();
        builder.pop(1);

        let error = match builder.finish("underflowing") {
            Err(error) => error,
            Ok(_) => panic!("underflow must fail closed"),
        };
        assert!(matches!(
            error,
            BytecodeError::InternalError { message }
                if message.contains("operand stack underflow")
        ));
    }

    #[test]
    fn finish_rejects_jvm_method_limits_before_serialization() {
        let mut oversized_code = CodeBuilder::new();
        for _ in 0..=u16::MAX {
            oversized_code.op(0);
        }
        assert!(matches!(
            oversized_code.finish("huge"),
            Err(BytecodeError::CodeSizeExceeded { method }) if method == "huge"
        ));

        let mut oversized_stack = CodeBuilder::new();
        oversized_stack.push(i32::from(u16::MAX) + 1);
        assert!(matches!(
            oversized_stack.finish("deep"),
            Err(BytecodeError::InternalError { message })
                if message.contains("max_stack limit") && message.contains("deep")
        ));
    }
}
