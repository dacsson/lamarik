use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};

use bitvec::array::BitArray;
use bitvec::vec::BitVec;
use bitvec::{BitArr, prelude as bv};
use lamacore::bytecode::{Builtin, Instruction, PattKind, ValueRel};
use lamacore::bytefile::Bytefile;
use lamacore::decoder::{Decoder, DecoderError};

pub const MAX_SEXP_TAGLEN: usize = 10;
pub const MAX_CAPTURES: usize = 0x7fffffff;
#[cfg(test)]
pub const MAX_OPERAND_STACK_SIZE: usize = 0xffff;
#[cfg(not(test))]
pub const MAX_OPERAND_STACK_SIZE: usize = 0x7fffffff;
pub const MAX_ARG_LEN: usize = 50;
pub const MAX_SEXP_MEMBERS: usize = 0xffff;
pub const MAX_ARRAY_MEMBERS: usize = 0xffff;
pub const MAX_PARAMS: usize = 0xffff;

#[derive(Debug)]
pub enum VerifierError {
    DecoderError(DecoderError),
    InvalidJumpOffset(usize, i32, usize),
    StringIndexOutOfBounds,
    InvalidStoreIndex(ValueRel, i32, i64),
    InvalidLoadIndex(ValueRel, i32, i64),
    TooMuchMembers(usize, usize),
    TooManyCaptures(usize),
    SexpTagTooLong(usize),
    InvalidBeginArgs(i32),
    StackUnderflow(isize),
    StackOverflow,
}

impl std::fmt::Display for VerifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifierError::DecoderError(e) => {
                write!(f, "{}", e)
            }
            VerifierError::InvalidJumpOffset(ip, offset, code_len) => {
                write!(
                    f,
                    "Invalid jump offset: current ip at {}, offset is {}, but code length is {}",
                    ip, offset, code_len
                )
            }
            VerifierError::StringIndexOutOfBounds => {
                write!(f, "String index out of bounds")
            }
            VerifierError::InvalidStoreIndex(rel, index, n) => {
                write!(f, "Invalid store index {}/{} for {}", index, n, rel)
            }
            VerifierError::InvalidLoadIndex(rel, index, n) => {
                write!(f, "Invalid load index {}/{} for {}", index, n, rel)
            }
            VerifierError::TooMuchMembers(n, max) => {
                write!(f, "Too many members: {} > {}", n, max)
            }
            VerifierError::TooManyCaptures(n) => {
                write!(f, "Too many captures: {} > {}", n, MAX_CAPTURES)
            }
            VerifierError::SexpTagTooLong(n) => {
                write!(f, "Sexp tag too long: {} > {}", n, MAX_SEXP_MEMBERS)
            }
            VerifierError::InvalidBeginArgs(n) => {
                write!(f, "Too much begin arguments: {} > {}", n, MAX_PARAMS)
            }
            VerifierError::StackUnderflow(expected) => {
                write!(f, "Stack underflow, expected at least {}", expected)
            }
            VerifierError::StackOverflow => {
                write!(f, "Stack overflow, max is {}", MAX_OPERAND_STACK_SIZE)
            }
        }
    }
}

impl std::error::Error for VerifierError {}

pub struct Verifier {
    decoder: Decoder,
}

impl Verifier {
    pub fn new(decoder: Decoder) -> Self {
        Verifier { decoder }
    }

    fn is_jump(instr: &Instruction) -> bool {
        match instr {
            Instruction::JMP { .. } | Instruction::CJMP { .. } => true,
            _ => false,
        }
    }

    fn is_terminal(instr: &Instruction) -> bool {
        match instr {
            Instruction::RET
            | Instruction::END
            | Instruction::FAIL { .. }
            | Instruction::JMP { .. } => true,
            _ => false,
        }
    }

    fn is_call(instr: &Instruction) -> bool {
        match instr {
            Instruction::CALL { .. } => true,
            _ => false,
        }
    }

    fn get_call_offset(instr: &Instruction) -> Option<i32> {
        match instr {
            Instruction::CALL { offset, .. } => Some(*offset),
            _ => None,
        }
    }

    fn is_split(instr: &Instruction) -> bool {
        match instr {
            Instruction::RET
            | Instruction::END
            | Instruction::FAIL { .. }
            | Instruction::JMP { .. }
            | Instruction::CALL { .. }
            | Instruction::CALLC { .. } => true,
            _ => false,
        }
    }

    /// Walk bytecode to find reachable offsets, starting from public symbols
    pub fn get_reachables(&mut self) -> Result<ReachableResult, VerifierError> {
        // Initialize offsets in code section with all bits set to false
        let mut reachable_offsets = BitVec::new();
        reachable_offsets.resize(self.decoder.code_section_len, false);

        // Initialize jump targets
        let mut target_offsets = BitVec::new();
        target_offsets.resize(self.decoder.code_section_len, false);

        // Walking queue
        let mut worklist = VecDeque::new();
        worklist.reserve(self.decoder.bf.public_symbols.len());

        // Add public symbols to the worklist
        for (_, offset) in &self.decoder.bf.public_symbols {
            if !worklist.contains(offset) {
                worklist.push_back(*offset);
            }
        }

        while !worklist.is_empty() {
            let offset = worklist.pop_front().unwrap();

            // Move to work element location (offset) in bytecode
            self.decoder.ip = offset as usize;

            let addr = offset as usize;

            // Skip if visited
            if reachable_offsets[addr] {
                continue;
            }

            // Mark visited
            reachable_offsets.set(addr, true);

            let encoding = self
                .decoder
                .next::<u8>()
                .map_err(|e| VerifierError::DecoderError(e))?;

            let instr = self
                .decoder
                .decode(encoding)
                .map_err(|e| VerifierError::DecoderError(e))?;

            // Enqueue functions that are called to process
            if Verifier::is_call(&instr) {
                let Instruction::CALL { .. } = instr else {
                    unreachable!()
                };

                if let Some(offset) = Verifier::get_call_offset(&instr) {
                    if !worklist.contains(&(offset as u32)) {
                        worklist.push_back(offset as u32);
                    }
                }
            }

            // It doesnt mean we will call it!
            if let Instruction::CLOSURE { offset, .. } = instr {
                if !worklist.contains(&(offset as u32)) {
                    worklist.push_back(offset as u32);
                }
            }

            // Enqueue jump targets
            if Verifier::is_jump(&instr) {
                match instr {
                    Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                        if !worklist.contains(&(offset as u32)) {
                            worklist.push_back(offset as u32);
                        }
                        target_offsets.set(offset as usize, true);
                    }
                    _ => {}
                }
            }

            // Push next instruction
            if !Verifier::is_terminal(&instr) {
                if !worklist.contains(&(self.decoder.ip as u32)) {
                    worklist.push_back(self.decoder.ip as u32);
                }
            }
        }

        Ok(ReachableResult {
            reachables: reachable_offsets,
            targets: target_offsets,
        })
    }

    pub fn verify(&mut self) -> Result<(VerificationResult, ReachableResult), VerifierError> {
        let ReachableResult {
            reachables,
            targets,
        } = self.get_reachables()?;

        // Get reachable addresses from bit vector
        let mut addresses = reachables.iter_ones().collect::<Vec<_>>();
        addresses.sort();
        addresses.dedup();

        let mut stack_depths = Vec::new();
        stack_depths.resize(self.decoder.code_section_len, 0);

        let mut current_stack_depth = 0;

        let mut current_function_begin_offset = 9;

        self.decoder.ip = addresses[0];

        for address in addresses {
            if self.decoder.ip == self.decoder.code_section_len - 1 {
                break;
            }

            // Out of code section errors are handled by decoder
            // Unknown instruction errors are handled by decoder
            let encoding = self
                .decoder
                .next::<u8>()
                .map_err(|e| VerifierError::DecoderError(e))?;
            let instr = self
                .decoder
                .decode(encoding)
                .map_err(|e| VerifierError::DecoderError(e))?;

            let current_instr_end = self.decoder.ip;

            self.verify_instruction(
                &instr,
                &mut stack_depths,
                &mut current_stack_depth,
                &mut current_function_begin_offset,
                address,
                current_instr_end,
            )?;
        }

        Ok((
            VerificationResult { stack_depths },
            ReachableResult {
                reachables,
                targets,
            },
        ))
    }

    fn verify_instruction(
        &mut self,
        instruction: &Instruction,
        stack_depths: &mut Vec<isize>,
        current_stack_depth: &mut isize,
        current_function_begin_offset: &mut usize,
        current_begin_instr_address: usize, // ip before decoding instruction
        current_inst_end: usize,
    ) -> Result<(), VerifierError> {
        let code_section_len = self.decoder.code_section_len;
        let ip = self.decoder.ip;
        let stringtab_size = self.decoder.bf.string_table.len();
        let global_area_size = self.decoder.bf.global_area_size;

        let stack_effect = instruction.stack_size_effect();
        *current_stack_depth += stack_effect;

        if *current_stack_depth < 0 {
            return Err(VerifierError::StackUnderflow(
                instruction.stack_size_effect(),
            ));
        }

        if *current_stack_depth > MAX_OPERAND_STACK_SIZE as isize {
            return Err(VerifierError::StackOverflow);
        }

        // Record max stack depth for current function
        if *current_stack_depth > stack_depths[*current_function_begin_offset] {
            stack_depths[*current_function_begin_offset] = *current_stack_depth;
        }

        match instruction {
            Instruction::BEGIN { args, locals } | Instruction::CBEGIN { args, locals } => {
                // TODO: main check for 2 arguments

                if *args as usize > MAX_PARAMS {
                    return Err(VerifierError::InvalidBeginArgs(*args));
                }

                // Record the address of the BEGIN instruction to remember in which function we are
                *current_function_begin_offset = self.decoder.ip;
            }
            Instruction::END => {
                *current_stack_depth = 0;
            }
            Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        ip,
                        *offset,
                        code_section_len,
                    ));
                }
            }
            Instruction::CALL { offset, n } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        ip,
                        *offset,
                        code_section_len,
                    ));
                }
            }
            Instruction::STRING { index } => {
                let string_index = *index as usize;
                if string_index >= stringtab_size as usize {
                    return Err(VerifierError::StringIndexOutOfBounds);
                }
            }
            Instruction::SEXP { s_index, n_members } => {
                let string_index = *s_index as usize;
                if string_index >= stringtab_size as usize {
                    return Err(VerifierError::StringIndexOutOfBounds);
                }

                let mems = *n_members as usize;
                if mems >= MAX_SEXP_MEMBERS {
                    return Err(VerifierError::TooMuchMembers(mems, MAX_SEXP_MEMBERS));
                }

                let tag = self
                    .decoder
                    .bf
                    .get_string_at_offset(string_index)
                    .map_err(|_| VerifierError::StringIndexOutOfBounds)?;

                if tag.len() > MAX_SEXP_TAGLEN {
                    return Err(VerifierError::SexpTagTooLong(tag.len()));
                }
            }
            Instruction::ARRAY { n } => {
                let array_members = *n as usize;
                if array_members >= MAX_ARRAY_MEMBERS {
                    return Err(VerifierError::TooMuchMembers(
                        array_members,
                        MAX_ARRAY_MEMBERS,
                    ));
                }
            }
            Instruction::STORE { rel, index } | Instruction::LOAD { rel, index } => {
                if let ValueRel::Global = rel {
                    let el_index = *index as usize;

                    if el_index >= global_area_size as usize {
                        if let Instruction::STORE { .. } = instruction {
                            return Err(VerifierError::InvalidStoreIndex(
                                ValueRel::Global,
                                *index,
                                global_area_size as i64,
                            ));
                        } else {
                            return Err(VerifierError::InvalidLoadIndex(
                                ValueRel::Global,
                                *index,
                                global_area_size as i64,
                            ));
                        }
                    }
                }
            }
            Instruction::CLOSURE { offset, arity } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        ip,
                        *offset,
                        code_section_len,
                    ));
                }

                let arity = *arity as usize;

                if arity >= MAX_CAPTURES {
                    return Err(VerifierError::TooManyCaptures(arity));
                }
            }
            _ => {}
        };

        if let Instruction::JMP { offset } = instruction {
            self.decoder.ip = *offset as usize;
        } else {
            self.decoder.ip = current_inst_end;
        }
        Ok(())
    }
}

pub struct ReachableResult {
    pub reachables: BitVec,
    pub targets: BitVec,
}

pub struct VerificationResult {
    pub stack_depths: Vec<isize>,
}

trait StackEffect {
    /// Returns the stack effect of the instruction.
    fn stack_size_effect(&self) -> isize;
}

impl StackEffect for Instruction {
    fn stack_size_effect(&self) -> isize {
        match self {
            Instruction::NOP => 0,
            Instruction::BINOP { .. } => -1, // pop two, push one
            Instruction::CONST { .. } => 1,
            Instruction::STRING { .. } => 1,
            Instruction::SEXP { n_members, .. } => {
                // pop n_members arguments, push the new S‑exp
                1 - (*n_members as isize)
            }
            Instruction::JMP { .. } => 0,
            Instruction::STA => -2, // pop 3, push aggregate back
            Instruction::STI => 0,
            Instruction::CBEGIN { args, locals } | Instruction::BEGIN { args, locals } => {
                /*frame_ptr*/
                1 + /* closure */ 1 + /*arg count */1 + /*local count*/1
                + /*ret frame ptr*/ 1 + /*ret ip*/ 1 + /*args*/ *args as isize + /*locals*/ *locals as isize
            }
            // depends on the current frame metadata
            Instruction::END | Instruction::RET => 0,
            Instruction::STORE { .. } => 1,
            Instruction::LOAD { .. } => 1,
            Instruction::DROP => -1,
            Instruction::DUP => 1,  // pop 1, push 2 copies
            Instruction::SWAP => 0, // pop 2, push 2 (swapped)
            Instruction::LINE { .. } => 0,
            Instruction::CALL { .. } => 2,
            Instruction::CALLBUILTIN { name, n } => match name {
                Builtin::Barray => 0 + *n as isize,
                Builtin::Llength => 0,
                Builtin::Lread => 1,
                Builtin::Lwrite => 0,
                Builtin::Lstring => 0,
                _ => 0,
            },
            Instruction::CJMP { .. } => -1,
            Instruction::ELEM => -1, // pop index n container, push result
            Instruction::ARRAY { .. } => 0,
            Instruction::TAG { .. } => 0,
            Instruction::FAIL { .. } => -1,
            Instruction::CLOSURE { .. } => 1,
            Instruction::CALLC { .. } => 0,
            Instruction::PATT { .. } => 0,
            _ => 0,
        }
    }
}

// struct InstructionTrace {
//     instruction: Instruction,
//     offset: usize,
// }

// impl InstructionTrace {
//     fn new(instruction: Instruction, offset: usize) -> Self {
//         InstructionTrace {
//             instruction,
//             offset,
//         }
//     }

// }
