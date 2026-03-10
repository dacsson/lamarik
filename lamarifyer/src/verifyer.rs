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
pub const MAX_OPERAND_STACK_SIZE: usize = 0xffff;
pub const MAX_ARG_LEN: usize = 50;
pub const MAX_SEXP_MEMBERS: usize = 0xffff;
pub const MAX_ARRAY_MEMBERS: usize = 0xffff;
pub const MAX_PARAMS: usize = 0xffff;

const UNSEEN: u32 = u32::MAX;
const TAG_MASK: u32 = 1;
const TAG_VISITED: u32 = 0;
const TAG_PENDING: u32 = 1;

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
    ExceededCodeSize(usize),
    InvalidControlFlow,
    NegativeArity,
    ExpectedBegin,
    ExpectedInstruction,
    ExpectedFunction,
    StackUnbalanced(u32, isize),
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
            VerifierError::ExceededCodeSize(n) => {
                write!(f, "Exceeded code size: {}", n)
            }
            VerifierError::InvalidControlFlow => {
                write!(f, "Invalid control flow")
            }
            VerifierError::NegativeArity => {
                write!(f, "Negative arity")
            }
            VerifierError::ExpectedBegin => {
                write!(f, "Expected BEGIN instruction")
            }
            VerifierError::ExpectedInstruction => {
                write!(f, "Expected to find instruction")
            }
            VerifierError::ExpectedFunction => {
                write!(f, "Expected to find function")
            }
            VerifierError::StackUnbalanced(expected, actual) => {
                write!(
                    f,
                    "Stack unbalance found at merge point, expected stack depth {}, but got {}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for VerifierError {}

pub struct Verifier {
    pub decoder: Decoder,
    // TODO: maybe we can make it as a single stack?
    instr_queue: [i64; MAX_OPERAND_STACK_SIZE / 2], // each element is (offset << 32 | func_count) packed in i64
    func_queue: [i64; MAX_OPERAND_STACK_SIZE / 2], // each element is (offset << 32 | stack_depth) packed in i64
    instr_len: usize,
    func_len: usize,
}

impl Verifier {
    pub fn new(decoder: Decoder) -> Self {
        Verifier {
            decoder,
            instr_queue: [0; MAX_OPERAND_STACK_SIZE / 2],
            func_queue: [0; MAX_OPERAND_STACK_SIZE / 2],
            instr_len: 0,
            func_len: 0,
        }
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

    fn decode_at(&mut self, addr: u32) -> Instruction {
        self.decoder.ip = addr as usize;

        let enc = self.decoder.next::<u8>().unwrap();
        self.decoder.decode(enc).unwrap()
    }

    fn get_opcode(&mut self, offset: u32) -> u8 {
        self.decoder.ip = offset as usize;
        self.decoder.next::<u8>().unwrap()
    }

    fn instruction_length(&mut self, start: u32) -> usize {
        let _ = self.decode_at(start);
        self.decoder.ip - start as usize
    }

    /// Pushes (offset, func_count) of the current instruction onto the queue
    fn queue_instruction(&mut self, offset: u32, func_count: usize) {
        self.instr_queue[self.instr_len] = (offset as i64) << 32 | func_count as i64;
        self.instr_len += 1;
    }

    /// Returns (offset, func_count) of the current instruction from the queue
    fn instruction_queue_pop(&mut self) -> (u32, usize) {
        self.instr_len -= 1;
        let val = self.instr_queue[self.instr_len];
        ((val >> 32) as u32, (val & 0xffffffff) as usize)
    }

    /// Pushes (offset, stack_depth) of the current func
    fn queue_function(&mut self, offset: u32, curr_stack_depth: isize) {
        self.func_queue[self.func_len] = (offset as i64) << 32 | curr_stack_depth as i64;
        self.func_len += 1;
    }

    /// Returns (offset, stack_depth) of the current function in queue
    fn function_queue_pop(&mut self) -> (u32, usize) {
        self.func_len -= 1;
        let val = self.func_queue[self.func_len];
        ((val >> 32) as u32, (val & 0xffffffff) as usize)
    }

    fn instruction_queue_is_empty(&self) -> bool {
        self.instr_len == 0
    }

    fn function_queue_is_empty(&self) -> bool {
        self.func_len == 0
    }

    // Change stack depth of current func
    fn set_function_stack_depth(&mut self, stack_depth: isize) {
        let curr_func = self.function_queue_pop();
        let offset = curr_func.0;
        self.queue_function(offset, stack_depth);
    }

    /// Returns (offset, func_count)
    fn peek_instruction(&self, index: usize) -> Option<(u32, usize)> {
        if self.instr_len == 0 || index >= self.instr_len {
            None
        } else {
            let val = self.instr_queue[self.instr_len - index - 1];
            Some(((val >> 32) as u32, val as usize))
        }
    }

    /// Returns (offset, stack_depth)
    fn peek_function(&self) -> Option<(usize, isize)> {
        if self.func_len == 0 {
            None
        } else {
            let val = self.func_queue[self.func_len - 1];
            Some(((val >> 32) as usize, val as isize))
        }
    }

    /// Writes the stack depth at big word of begin instruction byte
    fn write_function_stack_depth_at(&mut self, offset: usize, stack_depth: isize) {
        let begin_instr_bytes = &self.decoder.bf.code_section[offset + 1..offset + 1 + 4];
        let mut payload = u32::from_le_bytes(begin_instr_bytes.try_into().unwrap());
        payload |= (stack_depth.to_le() as u32) << 16;
        self.decoder.bf.code_section[offset + 1..offset + 1 + 4]
            .copy_from_slice(&payload.to_le_bytes());
    }

    fn drop_function(&mut self) -> (usize, isize) {
        let val = self.func_queue[self.func_len - 1];
        let offset = (val >> 32) as usize;
        let stack_depth = val as isize;
        self.func_len -= 1;
        (offset, stack_depth)
    }

    pub fn verify(&mut self) -> Result<(), VerifierError> {
        // We pick arbitrary big enough length of array, because otherwise we'd need to allocate dynamically
        // Oh boy do i want to just slap `self.decoder.code_section_len` in there
        let mut stack_depth = [UNSEEN; MAX_OPERAND_STACK_SIZE];

        let mut curr_stack_depth = 0;
        let mut func_count = 0;
        // Main is the only starting point
        self.queue_instruction(self.decoder.bf.main_offset, func_count);

        while !self.instruction_queue_is_empty() {
            let (offset_at, curr_func) = self.instruction_queue_pop();
            let instr = self.decode_at(offset_at);
            let next_offset = self.decoder.ip as u32;
            let length = self.instruction_length(offset_at as u32);
            let offset = offset_at as usize;

            println!("[LOG] instr: {:?} offset {}", instr, offset);

            if offset + length > self.decoder.code_section_len {
                return Err(VerifierError::ExceededCodeSize(offset));
            }

            if Verifier::is_pending(stack_depth[offset]) {
                // Jump target
                curr_stack_depth = Verifier::decode_depth(stack_depth[offset]) as isize;
                println!("[LOG] stack depth: {:?}", curr_stack_depth);
            }

            let stack_effect = instr.stack_size_effect();
            println!("[LOG] stack effect: {:?}", stack_effect);
            curr_stack_depth += stack_effect;
            println!("[LOG] stack depth: {:?}", curr_stack_depth);

            if curr_stack_depth < 0 {
                return Err(VerifierError::StackUnderflow(curr_stack_depth));
            }
            if curr_stack_depth > MAX_OPERAND_STACK_SIZE as isize {
                return Err(VerifierError::StackOverflow);
            }

            // Skip visited
            if Verifier::is_visited(stack_depth[offset]) {
                if self.instruction_queue_is_empty() {
                    continue;
                }

                let (next_offset, _) = self
                    .peek_instruction(0)
                    .ok_or(VerifierError::ExpectedInstruction)?;

                curr_stack_depth =
                    Verifier::decode_depth(stack_depth[next_offset as usize]) as isize;
                if Verifier::is_unseen(stack_depth[next_offset as usize]) {
                    return Err(VerifierError::InvalidControlFlow);
                }

                if Verifier::is_visited(stack_depth[next_offset as usize]) {
                    // Force the next isntr to have the correct stack depth
                    stack_depth[next_offset as usize] =
                        Verifier::encode_pending(curr_stack_depth as u32);
                    println!("[LOG] stack depth: {:?} of next_offset: {}", stack_depth[next_offset as usize], next_offset);
                }
                continue;
            }

            stack_depth[offset] = Verifier::encode_visited(curr_stack_depth as u32);
            println!("[LOG] stack depth: {:?} at offset: {}", stack_depth[offset], curr_stack_depth);

            if !self.function_queue_is_empty() {
                self.set_function_stack_depth(curr_stack_depth);
            }

            // Beware, a new function arrives
            match instr {
                Instruction::BEGIN { .. } | Instruction::CBEGIN { .. } => {
                    self.queue_function(offset as u32, curr_stack_depth);
                    func_count += 1;
                }
                _ => {}
            }

            if self.function_queue_is_empty() {
                return Err(VerifierError::InvalidControlFlow);
            }

            let (func_offset, func_stack_depth) = self
                .peek_function()
                .ok_or(VerifierError::ExpectedFunction)?;
            // if curr_stack_depth < func_stack_depth {
            //     return Err(VerifierError::InvalidControlFlow);
            // }

            // Handle jumps with targets
            match instr {
                Instruction::CJMP {
                    offset: target_at, ..
                }
                | Instruction::JMP { offset: target_at } => {
                    let target = target_at as usize;

                    if target + self.instruction_length(target_at as u32)
                        > self.decoder.code_section_len
                    {
                        return Err(VerifierError::InvalidJumpOffset(
                            offset,
                            target_at,
                            self.decoder.code_section_len,
                        ));
                    }

                    if !Verifier::is_unseen(stack_depth[target]) {
                        // We are at merge point of control flow
                        // TODO: check legitamate
                        if Verifier::decode_depth(stack_depth[target]) != curr_stack_depth as u32 {
                            // TODO: Specify that we failed at cfg merge point
                            return Err(VerifierError::StackUnbalanced(
                                Verifier::decode_depth(stack_depth[target]),
                                curr_stack_depth,
                            ));
                        }
                    } else {
                        stack_depth[target] = Verifier::encode_pending(curr_stack_depth as u32);
                        println!("[LOG] stack depth: {:?} at target: {}", stack_depth[target], target);
                        self.queue_instruction(target_at as u32, func_count);
                    }

                    // Skip direct jumps
                    if let Instruction::JMP { .. } = instr {
                        continue;
                    }
                }
                Instruction::CALL {
                    offset: target_at,
                    n,
                } => {
                    let target = target_at as usize;
                    if target + self.instruction_length(target_at as u32)
                        > self.decoder.code_section_len
                    {
                        return Err(VerifierError::InvalidJumpOffset(
                            offset,
                            target_at,
                            self.decoder.code_section_len,
                        ));
                    }
                    stack_depth[target] = Verifier::encode_pending(curr_stack_depth as u32);
                    println!("[LOG] stack depth: {:?} at target: {}", stack_depth[target], target);
                    self.queue_instruction(target_at as u32, func_count);
                }
                _ => {}
            }

            // NOTE: offset is after the opcode (? might change later)
            self.verify_instruction(func_offset, &instr)?;

            // Now check for terminal instructions
            if Verifier::is_terminal(&instr) {
                let need_to_drop_curr_func = self.func_len > 1
                    && self.peek_instruction(0).is_some()
                    && self
                        .peek_instruction(0)
                        .ok_or(VerifierError::ExpectedInstruction)?
                        .1
                        != curr_func
                    && self.peek_instruction(1).is_some()
                    && self
                        .peek_instruction(1)
                        .ok_or(VerifierError::ExpectedInstruction)?
                        .1
                        != curr_func;
                if need_to_drop_curr_func {
                    self.drop_function();
                }

                self.write_function_stack_depth_at(func_offset, func_stack_depth);

                if self.instruction_queue_is_empty() {
                    continue;
                }

                let (next_offset, next_func_count) = self
                    .peek_instruction(0)
                    .ok_or(VerifierError::ExpectedInstruction)?;
                curr_stack_depth =
                    Verifier::decode_depth(stack_depth[next_offset as usize]) as isize;
                if Verifier::is_visited(stack_depth[next_offset as usize]) {
                    stack_depth[next_offset as usize] = Verifier::encode_pending(curr_stack_depth as u32);
                    println!("[LOG] stack depth: {:?} at next_offset: {}", stack_depth[next_offset as usize], next_offset);
                }

                continue;
            }

            self.queue_instruction(next_offset, func_count);
        }

        Ok(())
    }

    #[inline]
    pub fn encode_visited(depth: u32) -> u32 {
        // low bit 0
        (depth << 1) | TAG_VISITED
    }

    #[inline]
    pub fn encode_pending(depth: u32) -> u32 {
        // low bit 1
        (depth << 1) | TAG_PENDING
    }

    #[inline]
    pub fn is_unseen(v: u32) -> bool {
        v == UNSEEN
    }

    #[inline]
    pub fn is_pending(v: u32) -> bool {
        v != UNSEEN && (v & TAG_MASK) == TAG_PENDING
    }

    #[inline]
    pub fn is_visited(v: u32) -> bool {
        v != UNSEEN && (v & TAG_MASK) == TAG_VISITED
    }

    /// Returns the stored stack depth for either VISITED or PENDING values.
    /// (Callers should typically check `is_unseen` first.)
    #[inline]
    pub fn decode_depth(v: u32) -> u32 {
        v >> 1
    }

    /// Convenience: mark an offset as a required jump target depth if it is currently UNSEEN.
    /// Returns true if it was set, false if it was already non-UNSEEN.
    #[inline]
    pub fn mark_pending_if_unseen(stack_depth: &mut [u32], idx: usize, required: u32) -> bool {
        if stack_depth[idx] == UNSEEN {
            stack_depth[idx] = Verifier::encode_pending(required);
            true
        } else {
            false
        }
    }

    fn verify_instruction(
        &mut self,
        func_offset: usize,
        instruction: &Instruction,
    ) -> Result<(), VerifierError> {
        match instruction {
            Instruction::BEGIN { args, locals } | Instruction::CBEGIN { args, locals } => {
                // TODO: main check for 2 arguments

                if *args as usize > MAX_PARAMS {
                    return Err(VerifierError::InvalidBeginArgs(*args));
                }
            }
            Instruction::END => {}
            Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= self.decoder.code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        self.decoder.ip,
                        *offset,
                        self.decoder.code_section_len,
                    ));
                }
            }
            Instruction::CALL { offset, n } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= self.decoder.code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        self.decoder.ip,
                        *offset,
                        self.decoder.code_section_len,
                    ));
                }

                if *n < 0 {
                    return Err(VerifierError::NegativeArity);
                }
            }
            Instruction::STRING { index } => {
                let string_index = *index as usize;
                if string_index >= self.decoder.bf.stringtab_size as usize {
                    return Err(VerifierError::StringIndexOutOfBounds);
                }
            }
            Instruction::SEXP { s_index, n_members } => {
                let string_index = *s_index as usize;
                if string_index >= self.decoder.bf.stringtab_size as usize {
                    return Err(VerifierError::StringIndexOutOfBounds);
                }

                if *n_members < 0 {
                    return Err(VerifierError::NegativeArity);
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
            Instruction::TAG { index, n } => {
                if *n < 0 {
                    return Err(VerifierError::NegativeArity);
                }

                if *index < 0 || *index >= self.decoder.bf.stringtab_size as i32 {
                    return Err(VerifierError::StringIndexOutOfBounds);
                }
            }
            Instruction::ARRAY { n } => {
                if *n < 0 {
                    return Err(VerifierError::NegativeArity);
                }

                let array_members = *n as usize;
                if array_members >= MAX_ARRAY_MEMBERS {
                    return Err(VerifierError::TooMuchMembers(
                        array_members,
                        MAX_ARRAY_MEMBERS,
                    ));
                }
            }
            Instruction::STORE { rel, index } | Instruction::LOAD { rel, index } => match rel {
                ValueRel::Global => {
                    let el_index = *index as usize;

                    if el_index >= self.decoder.bf.global_area_size as usize {
                        if let Instruction::STORE { .. } = instruction {
                            return Err(VerifierError::InvalidStoreIndex(
                                ValueRel::Global,
                                *index,
                                self.decoder.bf.global_area_size as i64,
                            ));
                        } else {
                            return Err(VerifierError::InvalidLoadIndex(
                                ValueRel::Global,
                                *index,
                                self.decoder.bf.global_area_size as i64,
                            ));
                        }
                    }
                }
                ValueRel::Local => {
                    let instr = self.decode_at(func_offset as u32);
                    let Instruction::BEGIN { args: _, locals } = instr else {
                        return Err(VerifierError::ExpectedBegin);
                    };

                    if *index >= locals {
                        return Err(VerifierError::InvalidLoadIndex(
                            ValueRel::Local,
                            *index,
                            locals as i64,
                        ));
                    }
                }
                ValueRel::Arg => {
                    let instr = self.decode_at(func_offset as u32);
                    let Instruction::BEGIN { args, .. } = instr else {
                        return Err(VerifierError::ExpectedBegin);
                    };

                    if *index >= args {
                        return Err(VerifierError::InvalidLoadIndex(
                            ValueRel::Arg,
                            *index,
                            args as i64,
                        ));
                    }
                }
                _ => {}
            },
            Instruction::CLOSURE { offset, arity } => {
                let offset_at = *offset as usize;

                if (*offset) < 0 || offset_at >= self.decoder.code_section_len {
                    return Err(VerifierError::InvalidJumpOffset(
                        self.decoder.ip,
                        *offset,
                        self.decoder.code_section_len,
                    ));
                }

                let arity = *arity as usize;

                if arity >= MAX_CAPTURES {
                    return Err(VerifierError::TooManyCaptures(arity));
                }
            }
            _ => {}
        };

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
                // /*frame_ptr*/
                // 1 + /* closure */ 1 + /*arg count */1 + /*local count*/1
                // + /*ret frame ptr*/ 1 + /*ret ip*/ 1 + /*args*/ *args as isize + /*locals*/ *locals as isize
                (3 + locals) as isize // args do not get moved
            }
            // depends on the current frame metadata
            Instruction::END | Instruction::RET => 0,
            Instruction::STORE { .. } => 0,
            Instruction::LOAD { .. } => 1,
            Instruction::DROP => -1,
            Instruction::DUP => 1,  // pop 1, push 2 copies
            Instruction::SWAP => 0, // pop 2, push 2 (swapped)
            Instruction::LINE { .. } => 0,
            Instruction::CALL { .. } => 2,
            Instruction::CALLBUILTIN { name, n } => match name {
                Builtin::Barray => 1 - *n as isize,
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
            Instruction::CALLC { .. } => 1,
            Instruction::PATT { .. } => 0,
            _ => 0,
        }
    }
}
