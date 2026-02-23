//! Implements static analysis of Lama VM bytecode, for frequency analysis of instructions.

use crate::bytecode::ValueRel;
use crate::disasm::Bytefile;
use crate::object::ObjectError;
use crate::{bytecode::Instruction, interpreter::InstructionTrace};
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter, Write};
use std::path::Path;

const MAX_SEXP_TAGLEN: usize = 10;
const MAX_SEXP_MEMBERS: usize = 0xffff;
const MAX_ARRAY_MEMBERS: usize = 0xffff;
const MAX_CAPTURES: usize = 0x7fffffff;
const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024; // 1GB

// TODO: add `LINE` diagnostic to all errors
#[derive(Debug, PartialEq)]
pub enum InterpreterError {
    StackUnderflow,
    EndOfCodeSection,
    ReadingMoreThenCodeSection,
    InvalidOpcode(u8),
    InvalidType(String),
    OutOfBoundsAccess(usize, usize),
    InvalidByteSequence(usize),
    StringIndexOutOfBounds,
    InvalidStringPointer,
    InvalidUtf8String,
    InvalidCString,
    InvalidObjectPointer,
    InvalidJumpOffset(usize, i32, usize),
    NotEnoughArguments(&'static str),
    InvalidStoreIndex(ValueRel, i32, i64),
    InvalidLoadIndex(ValueRel, i32, i64),
    InvalidLengthForArray,
    ObjectError(ObjectError),
    Fail {
        line: usize,
        column: usize,
        obj: String,
    },
    InvalidValueRel,
    TooMuchMembers(usize, usize),
    TooManyCaptures(usize),
    FileDoesNotExist(String),
    FileIsTooLarge(String),
    FileTypeError(String),
    DivisionByZero,
    SexpTagTooLong(usize),
}

/// Convert a byte, that couldnt be incoded into an interpreter error.
impl From<u8> for InterpreterError {
    fn from(opcode: u8) -> Self {
        InterpreterError::InvalidOpcode(opcode)
    }
}

impl From<ObjectError> for InterpreterError {
    fn from(err: ObjectError) -> Self {
        InterpreterError::ObjectError(err)
    }
}

impl std::fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InterpreterError::StackUnderflow => write!(f, "Stack underflow"),
            InterpreterError::EndOfCodeSection => write!(f, "End of code section"),
            InterpreterError::ReadingMoreThenCodeSection => {
                write!(f, "Reading more bytes than code section currently has")
            }
            InterpreterError::InvalidOpcode(opcode) => write!(f, "Invalid opcode: {:#x}", opcode),
            InterpreterError::InvalidType(name) => write!(f, "Invalid type: {}", name),
            InterpreterError::OutOfBoundsAccess(index, length) => write!(
                f,
                "Out of bounds access at index {} with length {}",
                index, length
            ),
            InterpreterError::InvalidByteSequence(ip) => {
                write!(f, "Invalid byte sequence at index {}", ip)
            }
            InterpreterError::StringIndexOutOfBounds => {
                write!(f, "String index out of bounds")
            }
            InterpreterError::InvalidStringPointer => {
                write!(f, "Invalid string pointer")
            }
            InterpreterError::InvalidUtf8String => {
                write!(f, "Invalid UTF-8 string")
            }
            InterpreterError::InvalidCString => {
                write!(f, "Invalid C string")
            }
            InterpreterError::InvalidObjectPointer => {
                write!(f, "Invalid object pointer")
            }
            InterpreterError::InvalidJumpOffset(ip, offset, code_len) => {
                write!(
                    f,
                    "Invalid jump offset: current ip at {}, offset is {}, but code length is {}",
                    ip, offset, code_len
                )
            }
            InterpreterError::NotEnoughArguments(instr) => {
                write!(f, "Not enough arguments for instruction `{}`", instr)
            }
            InterpreterError::InvalidStoreIndex(rel, index, n) => {
                write!(f, "Invalid store index {}/{} for {}", index, n, rel)
            }
            InterpreterError::InvalidLoadIndex(rel, index, n) => {
                write!(f, "Invalid load index {}/{} for {}", index, n, rel)
            }
            InterpreterError::InvalidLengthForArray => {
                write!(f, "Invalid length for array")
            }
            InterpreterError::ObjectError(err) => {
                write!(f, "Object creation error: {}", err)
            }
            InterpreterError::Fail { line, column, obj } => {
                write!(
                    f,
                    "Failed matching at line {} column {}: {}",
                    line, column, obj
                )
            }
            InterpreterError::InvalidValueRel => {
                write!(
                    f,
                    "Invalid value relation, there is only: Global(0), Local(1), Argument(2) and Captured(3), encountered something else"
                )
            }
            InterpreterError::TooMuchMembers(n, max) => {
                write!(f, "Too much aggregate members: {}, max is {}", n, max)
            }
            InterpreterError::FileDoesNotExist(file) => {
                write!(f, "File does not exist: {}", file)
            }
            InterpreterError::FileIsTooLarge(file) => {
                write!(f, "File is too large: {}, max is 1GB", file)
            }
            InterpreterError::FileTypeError(file) => {
                write!(f, "File type error: {}, expected .bc", file)
            }
            InterpreterError::DivisionByZero => {
                write!(f, "Division by zero")
            }
            InterpreterError::TooManyCaptures(captured_len) => {
                write!(
                    f,
                    "Too many captured variables: {}, max is {}",
                    captured_len, MAX_CAPTURES
                )
            }
            InterpreterError::SexpTagTooLong(len) => {
                write!(f, "Sexp tag too long: {}, max is {}", len, MAX_SEXP_TAGLEN)
            }
        }
    }
}

impl std::error::Error for InterpreterError {}

#[derive(Debug, Clone)]
pub struct Function {
    label: i32,
    reachable: bool,
    blocks: Vec<Block>,
    target_offsets: HashSet<usize>,
}

impl Function {
    pub fn new(label: i32) -> Self {
        Function {
            label,
            reachable: false,
            blocks: Vec::new(),
            target_offsets: HashSet::new(),
        }
    }

    pub fn add_instruction(&mut self, instruction: InstructionTrace) {
        self.blocks
            .last_mut()
            .unwrap()
            .instructions
            .push(instruction);
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    offset: usize,
    offset_end: usize,
    label: i32,
    predecessors: Vec<i32>,
    instructions: Vec<InstructionTrace>,
}

impl Block {
    pub fn new(offset: usize, label: i32) -> Self {
        Block {
            offset,
            offset_end: 0,
            label,
            predecessors: Vec::new(),
            instructions: Vec::new(),
        }
    }
}

pub struct Analyzer {
    functions: Vec<Function>,
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer {
            functions: Vec::new(),
        }
    }

    pub fn build_cfg(&mut self, instructions: Vec<InstructionTrace>) {
        let mut functions: Vec<Function> = Vec::new();

        let mut counter = 0;
        let mut func_counter = 0;

        let mut current_func = Function::new(func_counter);

        let mut previous_instruction_is_jmp = false;
        let mut previous_instruction_is_end = false;

        // Push entry block
        current_func.blocks.push(Block::new(0, counter));

        // CALL targets
        let mut call_targets = Vec::new();

        // First pass:
        // 1. Split bytecode into functions
        // 2. Assign target offsets to each function
        for trace in instructions {
            if previous_instruction_is_jmp {
                current_func.target_offsets.insert(trace.offset);
                previous_instruction_is_jmp = false;
            }

            match trace.instruction {
                Instruction::BEGIN { .. } | Instruction::CBEGIN { .. } => {
                    // Push entry block
                    current_func
                        .blocks
                        .push(Block::new(trace.offset as usize, counter));

                    // If we create a function that is in the call targets we mark it reachable
                    if call_targets.contains(&trace.offset) || trace.offset == 0 {
                        current_func.reachable = true;
                    }

                    func_counter += 1;
                    counter = 0;
                }
                Instruction::END | Instruction::RET => {
                    previous_instruction_is_end = true;
                    current_func.blocks.last_mut().unwrap().offset_end = trace.offset as usize;
                    // functions.push(current_func.clone());
                }
                Instruction::JMP { offset } => {
                    current_func.target_offsets.insert(offset as usize);
                }
                Instruction::CJMP { offset, .. } => {
                    current_func.target_offsets.insert(offset as usize);

                    // the next instruction is also a possible target offset,
                    // if compare is false
                    previous_instruction_is_jmp = true;
                }
                Instruction::CALL {
                    ref offset,
                    ref n,
                    ref name,
                    ref builtin,
                } => {
                    // NOTE: We only record target offsets for calls inside a function

                    if *builtin {
                        continue;
                    }

                    let Some(offset) = offset else {
                        continue;
                    };

                    // If some function exists with this offset mark it as reachable
                    if let Some(func) = functions
                        .iter_mut()
                        .find(|f| f.blocks[0].offset == *offset as usize)
                    {
                        func.reachable = true;
                    }

                    call_targets.push(*offset as usize);
                }
                _ => {}
            }

            current_func.add_instruction(trace);

            if previous_instruction_is_end {
                previous_instruction_is_end = false;
                functions.push(current_func);
                current_func = Function::new(func_counter);
            }
        }

        // Remove unreachable functions
        functions.retain(|func| func.reachable);

        // Second pass:
        // Split each function into basic blocks
        // by iterating over target offsets from first step
        for mut func in functions {
            // After first pass we have only one block in each function
            // Entry block will be recreated, thus we move instructions to a new vector
            let instructions = func.blocks[0].instructions.drain(..).collect::<Vec<_>>();

            // Sort offsets
            // TODO: fix clone()
            let mut offsets = func
                .target_offsets
                .iter()
                .map(|off| *off)
                .collect::<Vec<usize>>();
            // .clone().into_iter().collect::<Vec<_>>();
            offsets.push(func.blocks[0].offset);
            offsets.push(func.blocks[0].offset_end);
            offsets.sort();

            // Create pairs, which are (begin_block_offset, end_block_offset) for each block
            let offset_pairs = offsets
                .iter()
                .copied()
                .zip(offsets.iter().skip(1).copied())
                .collect::<Vec<(usize, usize)>>();

            // If we have just (begin_func_offset, end_func_offset) =>
            // we have only one block in the function
            if offset_pairs.len() == 1 {
                self.functions.push(func);
                continue;
            }

            // Cut instructions from entry block, based on offsets
            let mut blocks = vec![];
            let mut label = 0;
            for (begin, end) in offset_pairs {
                let mut block = Block::new(begin, label);
                block.offset = begin;

                let first_instruction = instructions
                    .iter()
                    .position(|trace| trace.offset == begin)
                    .unwrap();
                let last_instruction = instructions
                    .iter()
                    .position(|trace| trace.offset == end)
                    .unwrap();

                block.instructions = instructions[first_instruction..last_instruction].to_vec();
                blocks.push(block);
                label += 1;
            }

            func.blocks = blocks;

            self.functions.push(func);
        }

        // map of {target_label: Vec<Predecessor_Label>}
        let mut label_to_predecessor = HashMap::new();
        // Third pass:
        // Assign predecessors for each block
        for func in &mut self.functions {
            for block in &func.blocks {
                for instruction in &block.instructions {
                    if let Instruction::JMP { offset } = instruction.instruction {
                        // if let Some(target_block) = func.blocks.iter().find(|b| b.label == offset) {
                        //     target_block.predecessors.push(block.label);
                        // }
                        func.blocks
                            .iter()
                            .find(|b| b.offset == offset as usize)
                            .map(|b| {
                                label_to_predecessor
                                    .entry(b.label)
                                    .or_insert(Vec::new())
                                    .push(block.label)
                            });
                    }

                    if let Instruction::CJMP { offset, .. } = instruction.instruction {
                        func.blocks
                            .iter()
                            .find(|b| b.offset == offset as usize)
                            .map(|b| {
                                label_to_predecessor
                                    .entry(b.label)
                                    .or_insert(Vec::new())
                                    .push(block.label)
                            });
                    }
                }
            }
        }

        for func in &mut self.functions {
            for block in &mut func.blocks {
                if let Some(preds) = label_to_predecessor.remove(&block.label) {
                    block.predecessors = preds;
                }
            }
        }
    }

    pub fn get_functions(&self) -> &Vec<Function> {
        &self.functions
    }

    pub fn get_frequency(&self) -> Frequency {
        let mut frequencies = Frequency::new();

        // Frequency analysis for a single opcode
        for func in &self.functions {
            for block in &func.blocks {
                let names = &mut block
                    .instructions
                    .iter()
                    .map(|instr| instr.instruction.get_opcode_name())
                    .collect::<Vec<String>>();

                names
                    .drain(..)
                    .into_iter()
                    .for_each(|name| frequencies.add_instruction(name));
            }
        }

        // Frequency analysis for a sequence of two opcodes
        for func in &self.functions {
            for block in &func.blocks {
                let names = &block
                    .instructions
                    .iter()
                    .map(|instr| instr.instruction.get_opcode_name())
                    .collect::<Vec<String>>();

                let instr_seq = names
                    .iter()
                    .zip(names.iter().skip(1))
                    .map(|(f, s)| format!("{}; {}", f, s))
                    .collect::<Vec<_>>();

                for seq in instr_seq {
                    frequencies.add_instruction(seq);
                }
            }
        }

        frequencies
    }

    /// Verify the input file before parsing as a bytecode file
    pub fn verify_input(input: &str) -> Result<(), InterpreterError> {
        // Check existance
        if !Path::new(input).exists() {
            return Err(InterpreterError::FileDoesNotExist(input.to_string()));
        }

        // Check file type (naive)
        // NOTE: we can use a hack: `file` command detects `bc` file type as a matlab file,
        // but im not sure its platform-independent
        let extension = Path::new(input).extension().unwrap_or_default();
        if extension != "bc" {
            return Err(InterpreterError::FileTypeError(input.to_string()));
        }

        // Check file size
        let metadata = std::fs::metadata(input)
            .map_err(|_| InterpreterError::FileIsTooLarge(input.to_string()))?;
        if metadata.len() >= MAX_FILE_SIZE {
            return Err(InterpreterError::FileIsTooLarge(input.to_string()))?;
        }

        Ok(())
    }

    /// Static verification of bytecode on built CFG on par with the bytefile.
    pub fn verify_bytecode(&self, bf: &Bytefile) -> Result<(), InterpreterError> {
        if self.functions.is_empty() {
            panic!("Please, run cfg builder first");
        }

        // Check code section
        if bf.code_section.is_empty() {
            return Err(InterpreterError::EndOfCodeSection);
        }

        let code_section_len = bf.code_section.len();
        let strintab_size = bf.string_table.len();

        for func in &self.functions {
            for block in &func.blocks {
                for instr in &block.instructions {
                    let ip = instr.offset;
                    let instruction = &instr.instruction;

                    match instruction {
                        Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                            let offset_at = *offset as usize;

                            if (*offset) < 0 || offset_at >= code_section_len {
                                return Err(InterpreterError::InvalidJumpOffset(
                                    ip,
                                    *offset,
                                    code_section_len,
                                ));
                            }
                        }
                        Instruction::CALL {
                            offset,
                            n,
                            name,
                            builtin,
                        } => {
                            // No offset means its a builtin call
                            let Some(offset) = offset else {
                                continue;
                            };

                            let offset_at = *offset as usize;

                            if (*offset) < 0 || offset_at >= code_section_len {
                                return Err(InterpreterError::InvalidJumpOffset(
                                    ip,
                                    *offset,
                                    code_section_len,
                                ));
                            }
                        }
                        Instruction::STRING { index } => {
                            let string_index = *index as usize;
                            if string_index >= strintab_size as usize {
                                return Err(InterpreterError::StringIndexOutOfBounds);
                            }
                        }
                        Instruction::SEXP { s_index, n_members } => {
                            let string_index = *s_index as usize;
                            if string_index >= strintab_size as usize {
                                return Err(InterpreterError::StringIndexOutOfBounds);
                            }

                            let mems = *n_members as usize;
                            if mems >= MAX_SEXP_MEMBERS {
                                return Err(InterpreterError::TooMuchMembers(
                                    mems,
                                    MAX_SEXP_MEMBERS,
                                ));
                            }

                            let tag = bf
                                .get_string_at_offset(string_index)
                                .map_err(|_| InterpreterError::StringIndexOutOfBounds)?;

                            if tag.len() > MAX_SEXP_TAGLEN {
                                return Err(InterpreterError::SexpTagTooLong(tag.len()));
                            }
                        }
                        Instruction::ARRAY { n } => {
                            let array_members = *n as usize;
                            if array_members >= MAX_ARRAY_MEMBERS {
                                return Err(InterpreterError::TooMuchMembers(
                                    array_members,
                                    MAX_ARRAY_MEMBERS,
                                ));
                            }
                        }
                        Instruction::STORE { rel, index } | Instruction::LOAD { rel, index } => {
                            if let ValueRel::Global = rel {
                                let el_index = *index as usize;

                                if el_index >= bf.global_area_size as usize {
                                    if let Instruction::STORE { .. } = instr.instruction {
                                        return Err(InterpreterError::InvalidStoreIndex(
                                            ValueRel::Global,
                                            *index,
                                            bf.global_area_size as i64,
                                        ));
                                    } else {
                                        return Err(InterpreterError::InvalidLoadIndex(
                                            ValueRel::Global,
                                            *index,
                                            bf.global_area_size as i64,
                                        ));
                                    }
                                }
                            }
                        }
                        Instruction::CLOSURE { offset, arity } => {
                            let arity = *arity as usize;

                            if arity >= MAX_CAPTURES {
                                return Err(InterpreterError::TooManyCaptures(arity));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    pub fn cfg_to_dot(&self) -> String {
        // 1 – start the digraph and set a few nice defaults.
        let mut dot = String::new();
        writeln!(&mut dot, "digraph CFG {{").unwrap();
        writeln!(&mut dot, "    rankdir=TB;").unwrap(); // top‑to‑bottom layout
        writeln!(&mut dot, "    node [shape=record, fontname=\"Courier\"];").unwrap();

        let funcs = &self.functions;

        for func in funcs {
            let blocks = &func.blocks;
            // 2 – create a map from label -> block index for fast look‑ups.
            let mut label_to_idx = std::collections::HashMap::<i32, usize>::new();
            for (i, b) in blocks.iter().enumerate() {
                label_to_idx.insert(b.label, i);
            }

            // 3 – emit a node for every block.
            for block in blocks {
                // escape double quotes and backslashes for dot
                let mut instrs = String::new();
                for ins in &block.instructions {
                    let instr_as_string = format!("{:#?}", ins);
                    let replace_escaped = instr_as_string.replace("\r\n", "").replace("\n", "");
                    let esacpe_curly_braces =
                        replace_escaped.replace("{", "\\{").replace("}", "\\}");
                    // `\l` forces a left‑justified line break inside a record label
                    write!(&mut instrs, "{}\\l", esacpe_curly_braces).unwrap();
                }

                // record label:  { offset | instructions }
                let node_label = format!(
                    "{{ B{} | offset: {} | {} }}",
                    block.label,
                    block.offset,
                    if instrs.is_empty() {
                        "<empty>"
                    } else {
                        &instrs
                    }
                );

                writeln!(
                    &mut dot,
                    "    B{} [label=\"{}\"];", // node name = B<label>
                    block.label, node_label
                )
                .unwrap();
            }

            // 4 – emit edges.
            for (i, block) in blocks.iter().enumerate() {
                // 4a – edges coming from the explicit predecessor list.
                for &pred_label in &block.predecessors {
                    // huard against malformed predecessor data.
                    if label_to_idx.contains_key(&pred_label) {
                        writeln!(&mut dot, "    B{} -> B{};", pred_label, block.label).unwrap();
                    }
                }

                // 4b – fall‑through edge (the “next” block in linear order)
                //     Only add it when the block does *not* end with an unconditional jump
                let ends_with_uncond_jmp = block
                    .instructions
                    .last()
                    .map(|ins| matches!(ins.instruction, Instruction::JMP { .. }))
                    .unwrap_or(false);

                if !ends_with_uncond_jmp {
                    // The next block is simply the one that appears after us in the
                    // vector
                    if i + 1 < blocks.len() {
                        let succ_label = blocks[i + 1].label;
                        writeln!(
                            &mut dot,
                            "    B{} -> B{} [style=dashed];",
                            block.label, succ_label
                        )
                        .unwrap();
                    }
                } else {
                    // Find block with offset corresponding to the JMP instruction
                    if let Some(last) = block.instructions.last() {
                        if let Instruction::JMP { offset, .. } = last.instruction {
                            // Find the block whose `offset` matches the target address.
                            // In the current Analyzer implementation a block starts at the
                            // *target* of the jump, so this lookup works
                            if let Some(target_block) =
                                blocks.iter().find(|b| b.offset == offset as usize)
                            {
                                writeln!(
                                    &mut dot,
                                    "    B{} -> B{} [label=\"JMP\", color=red];",
                                    block.label, target_block.label
                                )
                                .unwrap();
                            }
                        }
                    }
                }

                // 4c – for conditional jumps we also want an explicit edge to the
                //     jump target (if the target exists). This is optional – you can
                //     omit it if you rely on the predecessor list only.
                if let Some(last) = block.instructions.last() {
                    if let Instruction::CJMP { offset, .. } = last.instruction {
                        // Find the block whose `offset` matches the target address.
                        // In the current Analyzer implementation a block starts at the
                        // *target* of the jump, so this lookup works
                        if let Some(target_block) =
                            blocks.iter().find(|b| b.offset == offset as usize)
                        {
                            writeln!(
                                &mut dot,
                                "    B{} -> B{} [label=\"CJMP\", color=blue];",
                                block.label, target_block.label
                            )
                            .unwrap();
                        }

                        // Second edge goes to BLOCK if condition is fale, i.e. to the next block
                        writeln!(
                            &mut dot,
                            "    B{} -> B{} [label=\"CJMP\", color=red];",
                            block.label,
                            block.label + 1
                        )
                        .unwrap();
                    }
                }
            }
        }

        writeln!(&mut dot, "}}").unwrap();
        dot
    }
}

pub struct Frequency {
    frequency: HashMap<String, u32>,
}

impl Frequency {
    pub fn new() -> Self {
        Frequency {
            frequency: HashMap::new(),
        }
    }

    pub fn add_instruction(&mut self, opcode_name: String) {
        *self.frequency.entry(opcode_name).or_insert(0) += 1;
    }
}

impl Display for Frequency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut to_vec = self.frequency.iter().collect::<Vec<_>>();
        to_vec.sort_by(|a, b| b.1.cmp(&a.1));

        for (opcode, count) in &to_vec {
            writeln!(f, "{}: {}", opcode, count)?;
        }
        Ok(())
    }
}

impl Debug for Frequency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (opcode, count) in &self.frequency {
            writeln!(f, "{}: {}", opcode, count)?;
        }
        Ok(())
    }
}
