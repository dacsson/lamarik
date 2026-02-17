//! VM Interpreter

use crate::bytecode::{Builtin, CompareJumpKind, Instruction, Op, PattKind, ValueRel};
use crate::disasm::Bytefile;
use crate::frame::FrameMetadata;
use crate::numeric::LeBytes;
use crate::object::{Object, ObjectError};
use crate::{
    __gc_init, Llength, Lread, Lstring, Lwrite, gc_set_bottom, gc_set_top, get_array_el,
    get_object_content_ptr, get_sexp_el, isUnboxed, lama_type_ARRAY, lama_type_SEXP,
    lama_type_STRING, new_array, new_sexp, new_string, rtBox, rtLen, rtToData, rtToSexp, rtUnbox,
    set_array_el, set_sexp_el,
};
use std::convert::TryFrom;
use std::ffi::{CStr, CString};
use std::panic;

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
        }
    }
}

impl std::error::Error for InterpreterError {}

pub struct InterpreterOpts {
    parse_only: bool,
    verbose: bool,
}

impl InterpreterOpts {
    pub fn new(parse_only: bool, verbose: bool) -> Self {
        Self {
            parse_only,
            verbose,
        }
    }
}

pub struct Interpreter {
    operand_stack: Vec<Object>,
    frame_pointer: usize,
    /// Decoded bytecode file with raw code section
    bf: Bytefile,
    /// Instruction pointer, moves along code section in `bf`
    ip: usize,
    opts: InterpreterOpts,
    /// Collect found instructions, only when `parse_only` is true
    instructions: Vec<Instruction>,
    /// Global variables
    globals: Vec<Object>,
}

impl Interpreter {
    /// Create a new interpreter with operand stack filled with
    /// emulated call to main
    pub fn new(bf: Bytefile, opts: InterpreterOpts) -> Self {
        let mut operand_stack = Vec::new();

        unsafe {
            __gc_init();
            // gc_set_top(0);
            // gc_set_bottom(0);
        }

        // Emulating call to main
        operand_stack.push(Object::new_empty()); // FRAME_PTR
        operand_stack.push(Object::new_unboxed(2)); // ARGS_COUNT
        operand_stack.push(Object::new_empty()); // LOCALS_COUNT
        operand_stack.push(Object::new_empty()); // OLD_FRAME_POINTER
        operand_stack.push(Object::new_empty()); // OLD_IP
        operand_stack.push(Object::new_empty()); // ARGV
        operand_stack.push(Object::new_empty()); // ARGC
        operand_stack.push(Object::new_empty()); // CURR_IP

        let global_areas_size = bf.global_area_size as usize;

        Interpreter {
            operand_stack,
            frame_pointer: 0,
            bf,
            ip: 0,
            opts,
            instructions: Vec::new(),
            globals: vec![Object::new_empty(); global_areas_size],
        }
    }

    /// Run the interpreter on a given program, without bytecode
    /// Useful for testing
    #[cfg(test)]
    pub fn run_on_program(&mut self, program: Vec<Instruction>) -> Result<(), InterpreterError> {
        for instr in program {
            self.eval(&instr)?;
        }

        Ok(())
    }

    /// Reads the next n bytes from the code section,
    /// where n is the size of type `T`.
    /// Returns the value read as type `T`, where `T` is an integer type.
    fn next<T: LeBytes>(&mut self) -> Result<T, InterpreterError> {
        if self.ip + std::mem::size_of::<T>() > self.bf.code_section.len() {
            return Err(InterpreterError::ReadingMoreThenCodeSection);
        }

        if self.ip > self.bf.code_section.len() {
            return Err(InterpreterError::EndOfCodeSection);
        }

        let bit_size = std::mem::size_of::<T>();
        let bytes = self
            .bf
            .code_section
            .get(self.ip..self.ip + bit_size)
            .ok_or(InterpreterError::EndOfCodeSection)?;

        self.ip += bit_size;

        Ok(T::from_le_bytes(bytes.try_into().unwrap()))
    }

    /// Main interpreter loop
    pub fn run(&mut self) -> Result<(), InterpreterError> {
        while self.ip < self.bf.code_section.len() {
            let encoding = self.next::<u8>()?;
            let instr = self.decode(encoding)?;

            if self.opts.verbose {
                println!("[LOG] IP {} BYTE {} INSTR {:?}", self.ip, encoding, instr);
            }

            if self.opts.parse_only {
                self.instructions.push(instr.clone());
            } else {
                self.eval(&instr)?;
            }

            // HACK: if we encounter END instruction, while in frame 0
            //       (a.k.a main function) we exit the interpreter
            if let Instruction::END = instr
                && self.frame_pointer == 0
            {
                break;
            }
        }

        Ok(())
    }

    /// Decode a byte into an instruction
    fn decode(&mut self, byte: u8) -> Result<Instruction, InterpreterError> {
        if byte == 0xff {
            return Ok(Instruction::HALT);
        }

        let (opcode, subopcode) = (byte & 0xF0, byte & 0x0F);

        match (opcode, subopcode) {
            (0x00, 0x0) => Ok(Instruction::NOP),
            (0x00, _) if subopcode >= 0x1 && subopcode <= 0xd => Ok(Instruction::BINOP {
                op: Op::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
            }),
            (0x00, _) => Err(InterpreterError::from(byte)),
            (0x10, 0x0) => Ok(Instruction::CONST {
                value: self.next::<i32>()?,
            }),
            (0x10, 0x1) => Ok(Instruction::STRING {
                index: self.next::<i32>()?,
            }),
            (0x10, 0x2) => Ok(Instruction::SEXP {
                s_index: self.next::<i32>()?,
                n_members: self.next::<i32>()?,
            }),
            (0x10, 0x3) => Ok(Instruction::STI),
            (0x10, 0x4) => Ok(Instruction::STA),
            (0x10, 0x5) => Ok(Instruction::JMP {
                offset: self.next::<i32>()?,
            }),
            (0x10, 0x6) => Ok(Instruction::END),
            (0x10, 0x7) => Ok(Instruction::RET),
            (0x10, 0x8) => Ok(Instruction::DROP),
            (0x10, 0x9) => Ok(Instruction::DUP),
            (0x10, 0xa) => Ok(Instruction::SWAP),
            (0x10, 0xb) => Ok(Instruction::ELEM),
            (0x20, _) if subopcode <= 0x3 => Ok(Instruction::LOAD {
                rel: ValueRel::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
                index: self.next::<i32>()?,
            }),
            (0x30, _) if subopcode <= 0x3 => Ok(Instruction::LOADREF {
                rel: ValueRel::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
                index: self.next::<i32>()?,
            }),
            (0x40, _) if subopcode <= 0x3 => Ok(Instruction::STORE {
                rel: ValueRel::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
                index: self.next::<i32>()?,
            }),
            (0x50, 0x0) => Ok(Instruction::CJMP {
                offset: self.next::<i32>()?,
                kind: crate::bytecode::CompareJumpKind::ISZERO,
            }),
            (0x50, 0x1) => Ok(Instruction::CJMP {
                offset: self.next::<i32>()?,
                kind: crate::bytecode::CompareJumpKind::ISNONZERO,
            }),
            (0x50, 0x2) => Ok(Instruction::BEGIN {
                args: self.next::<i32>()?,
                locals: self.next::<i32>()?,
            }),
            (0x50, 0x3) => Ok(Instruction::CBEGIN {
                args: self.next::<i32>()?,
                locals: self.next::<i32>()?,
            }),
            (0x50, 0x4) => {
                let offset = self.next::<i32>()?;
                let arity = self.next::<i32>()?;

                let mut captured = Vec::with_capacity(arity as usize);
                for _ in 0..arity {
                    captured.push(self.next::<i32>()?);
                }

                Ok(Instruction::CLOSURE {
                    offset,
                    arity,
                    captured,
                })
            }
            (0x50, 0x5) => Ok(Instruction::CALLC {
                arity: self.next::<i32>()?,
            }),
            (0x50, 0x6) => Ok(Instruction::CALL {
                offset: Some(self.next::<i32>()?),
                n: Some(self.next::<i32>()?),
                name: None,
                builtin: false,
            }),
            (0x50, 0x7) => Ok(Instruction::TAG {
                index: self.next::<i32>()?,
                n: self.next::<i32>()?,
            }),
            (0x50, 0x8) => Ok(Instruction::ARRAY {
                n: self.next::<i32>()?,
            }),
            (0x50, 0x9) => Ok(Instruction::FAIL {
                line: self.next::<i32>()?,
                column: self.next::<i32>()?,
            }),
            (0x50, 0xa) => Ok(Instruction::LINE {
                n: self.next::<i32>()?,
            }),
            (0x60, _) if subopcode <= 0x6 => Ok(Instruction::PATT {
                kind: PattKind::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
            }),
            (0x70, _) if subopcode <= 0x3 => Ok(Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?),
                builtin: true,
            }),
            (0x70, 0x4) => Ok(Instruction::CALL {
                offset: None,
                n: Some(self.next::<i32>()?),
                name: Some(Builtin::Barray),
                builtin: true,
            }),
            _ => Err(InterpreterError::InvalidOpcode(byte)),
        }
    }

    /// Evaluate a decoded instruction
    fn eval(&mut self, instr: &Instruction) -> Result<(), InterpreterError> {
        if self.opts.verbose {
            println!("[LOG] EVAL {:?}", instr);
        }

        match instr {
            Instruction::NOP => (),
            Instruction::BINOP { op } => {
                let right = self.pop()?.unwrap();
                let left = self.pop()?.unwrap();
                let result = match op {
                    Op::ADD => left + right,
                    Op::SUB => left - right,
                    Op::MUL => left * right,
                    Op::DIV => left / right,
                    Op::MOD => left % right,
                    Op::EQ => {
                        if left == right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::NEQ => {
                        if left != right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::LT => {
                        if left < right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::LEQ => {
                        if left <= right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::GT => {
                        if left > right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::GEQ => {
                        if left >= right {
                            1
                        } else {
                            0
                        }
                    }
                    Op::AND => {
                        if left != 0 && right != 0 {
                            1
                        } else {
                            0
                        }
                    }
                    Op::OR => {
                        if left != 0 || right != 0 {
                            1
                        } else {
                            0
                        }
                    }
                };

                if self.opts.verbose {
                    println!("[LOG] {} {:?} {} = {}", right, op, left, result);
                }

                self.push(Object::new_boxed(result))?;
            }
            Instruction::CONST { value: index } => self.push(Object::new_boxed(*index as i64))?,
            Instruction::STRING { index } => {
                let string = self
                    .bf
                    .get_string_at_offset(*index as usize)
                    .map_err(|_| InterpreterError::StringIndexOutOfBounds)?;

                if self.opts.verbose {
                    println!("[LOG][STRING] string: {:?}", string);
                }

                let lama_string =
                    new_string(string).map_err(|_| InterpreterError::InvalidStringPointer)?;

                self.push(
                    Object::try_from(lama_string)
                        .map_err(|_| InterpreterError::InvalidStringPointer)?,
                )?;

                if self.opts.verbose {
                    println!(
                        "[LOG] as_ptr {:?}; Object {}",
                        lama_string,
                        self.operand_stack[self.operand_stack.len() - 1]
                    )
                };
            }
            Instruction::SEXP { s_index, n_members } => {
                let tag_u8 = self
                    .bf
                    .get_string_at_offset(*s_index as usize)
                    .map_err(|_| InterpreterError::StringIndexOutOfBounds)?;

                if self.opts.verbose {
                    println!(
                        "[LOG][Instruction::SEXP] tag_u8: {:#?}, index {}",
                        tag_u8, s_index
                    );
                }

                let c_string = CString::from_vec_with_nul(tag_u8)
                    .map_err(|_| InterpreterError::InvalidCString)?;

                if self.opts.verbose {
                    println!(
                        "[LOG][Instruction::SEXP] c_string: {}",
                        c_string.to_str().unwrap()
                    );
                }

                let mut args = Vec::with_capacity(*n_members as usize);
                for _ in 0..*n_members {
                    args.push(self.pop()?.raw());
                }

                // Reverse the arguments to match the order of the SEXP constructor
                args.reverse();

                let sexp = new_sexp(c_string, args);

                if self.opts.verbose {
                    unsafe {
                        println!("[Log][SEXP] {:#?}", *rtToSexp(sexp));
                    }
                }

                self.push(
                    Object::try_from(sexp).map_err(|_| InterpreterError::InvalidObjectPointer)?,
                )?;
            }
            Instruction::JMP { offset } => {
                // NOTE: Frame shifting is delegated to `BEGIN` instruction

                let offset_at = *offset as usize;

                // verify offset is within bounds
                if (*offset) < 0 || offset_at >= self.bf.code_section.len() {
                    return Err(InterpreterError::InvalidJumpOffset(
                        self.ip,
                        *offset,
                        self.bf.code_section.len(),
                    ));
                }

                self.ip = offset_at;
            }
            Instruction::STA => {
                let value_obj = self.pop()?;
                let index_obj = self.pop()?;
                let mut aggregate = self.pop()?;

                let index = index_obj.unwrap() as usize;
                let value = value_obj.unwrap();

                // check for aggregate
                if aggregate.lama_type().is_none() {
                    return Err(InterpreterError::InvalidType(String::from(
                        "Expected an aggregate type in STA instruction",
                    )));
                }

                unsafe {
                    let length = rtUnbox(Llength(aggregate.as_ptr_mut().unwrap())) as usize;

                    // check for out of bounds access
                    if (index_obj.unwrap()) < 0 || index >= length {
                        return Err(InterpreterError::OutOfBoundsAccess(index, length));
                    }

                    let as_ptr = aggregate
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    if aggregate.lama_type().unwrap() == lama_type_SEXP {
                        let sexp = rtToSexp(as_ptr);
                        set_sexp_el(&mut *sexp, index, value);
                    } else if aggregate.lama_type().unwrap() == lama_type_STRING {
                        let contents = (*rtToData(as_ptr)).contents.as_mut_ptr();

                        contents.add(index).write(value as i8);
                    } else {
                        let array = rtToData(as_ptr);
                        // NOTE: array stores raw values, no need to unwrap object (i.e. unbox)
                        set_array_el(&mut *array, index, value_obj.raw());
                    }
                }

                self.push(aggregate)?;
            }
            Instruction::STI => panic!(
                "Congratulations! Somehow, you emitted a STI instruction, while the compiler itself never should have"
            ),
            Instruction::BEGIN { args, locals } => {
                // Save previous ip (provided by `CALL`)
                let ret_ip = self
                    .pop()
                    .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?;

                // Collect callee provided arguments
                let mut arguments = Vec::new();
                for _ in 0..*args {
                    arguments.push(
                        self.pop()
                            .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?,
                    );
                }

                // Save previous frame pointer
                let ret_frame_pointer = self.frame_pointer;

                // Set new frame pointer as index into operand stack
                if self.operand_stack.is_empty() {
                    return Err(InterpreterError::NotEnoughArguments("BEGIN"));
                }
                self.frame_pointer = self.operand_stack.len() - 1;

                // Push arg and local count
                self.push(Object::new_unboxed(*args as i64))?;
                self.push(Object::new_unboxed(*locals as i64))?;

                // Push return frame pointer and ip
                // 1. Where to return in sack operand
                self.push(Object::new_unboxed(ret_frame_pointer as i64))?;
                // 2. Where to return in the bytecode after this call
                self.push(ret_ip)?;

                // Re-push arguments
                for arg in arguments.into_iter().rev() {
                    self.push(arg)?;
                }

                // Initialize local variables with 0
                // We create them as boxed objects
                for _ in 0..*locals {
                    self.push(Object::new_boxed(0))?;
                }
            }
            Instruction::END | Instruction::RET => {
                // Get procedures return value
                let return_value = self
                    .pop()
                    .map_err(|_| InterpreterError::NotEnoughArguments("END"))?;

                let FrameMetadata {
                    n_locals,
                    n_args,
                    ret_frame_pointer,
                    ret_ip,
                } = FrameMetadata::get_from_stack(&self.operand_stack, self.frame_pointer)
                    .ok_or(InterpreterError::NotEnoughArguments("END"))?;

                // Return to callee's frame pointer
                self.frame_pointer = ret_frame_pointer;

                // Return to caller's instruction pointer
                // NOTE: returning from main is not possible in this implementation
                //       the program will exit after the main function returns
                self.ip = ret_ip;

                // Pop return ip
                self.pop()?;
                // Pop old frame pointer
                self.pop()?;
                // Pop local count
                self.pop()?;
                // Pop argument count
                self.pop()?;

                for _ in 0..n_args {
                    self.pop()?;
                }

                for _ in 0..n_locals {
                    self.pop()?;
                }

                // After removing current frames metadata,
                // we can re-push the return value to send it back to the caller
                self.push(return_value)?;
            }
            Instruction::STORE { rel, index } => {
                let mut frame =
                    FrameMetadata::get_from_stack(&self.operand_stack, self.frame_pointer)
                        .ok_or(InterpreterError::NotEnoughArguments("STORE"))?;

                let value = self.pop()?;

                match rel {
                    ValueRel::Arg => {
                        frame
                            .set_arg_at(
                                &mut self.operand_stack,
                                self.frame_pointer,
                                *index as usize,
                                value.clone(),
                            )
                            .map_err(|_| {
                                InterpreterError::InvalidStoreIndex(
                                    ValueRel::Arg,
                                    *index,
                                    frame.n_args,
                                )
                            })?;
                    }
                    ValueRel::Capture => panic!("Not implemented"),
                    ValueRel::Global => {
                        if (*index as usize) >= self.globals.len() {
                            return Err(InterpreterError::InvalidStoreIndex(
                                ValueRel::Global,
                                *index,
                                self.globals.len() as i64,
                            ));
                        } else {
                            self.globals[*index as usize] = value.clone();
                        }
                    }
                    ValueRel::Local => frame
                        .set_local_at(
                            &mut self.operand_stack,
                            self.frame_pointer,
                            *index as usize,
                            value.clone(),
                        )
                        .map_err(|_| {
                            InterpreterError::InvalidStoreIndex(
                                ValueRel::Local,
                                *index,
                                frame.n_locals,
                            )
                        })?,
                }

                self.push(value)?;
            }
            Instruction::LOAD { rel, index } => {
                let frame = FrameMetadata::get_from_stack(&self.operand_stack, self.frame_pointer)
                    .ok_or(InterpreterError::NotEnoughArguments("STORE"))?;

                match rel {
                    ValueRel::Arg => {
                        let value = frame
                            .get_arg_at(&self.operand_stack, self.frame_pointer, *index as usize)
                            .ok_or(InterpreterError::InvalidStoreIndex(
                                ValueRel::Arg,
                                *index,
                                frame.n_args,
                            ))?;

                        self.push(value.clone())?;
                    }
                    ValueRel::Capture => panic!("Not implemented"),
                    ValueRel::Global => {
                        if (*index as usize) >= self.globals.len() {
                            return Err(InterpreterError::InvalidStoreIndex(
                                ValueRel::Global,
                                *index,
                                self.globals.len() as i64,
                            ));
                        } else {
                            let value = self.globals[*index as usize].clone();
                            self.push(value)?;
                        }
                    }
                    ValueRel::Local => {
                        let value = frame
                            .get_local_at(&self.operand_stack, self.frame_pointer, *index as usize)
                            .ok_or(InterpreterError::InvalidStoreIndex(
                                ValueRel::Local,
                                *index,
                                frame.n_locals,
                            ))?;

                        self.push(value.clone())?;
                    }
                }
            }
            Instruction::DROP => {
                self.pop()?;
            }
            Instruction::DUP => {
                let value = self.pop()?;
                self.push(value.clone())?;
                self.push(value)?;
            }
            Instruction::SWAP => {
                let value1 = self.pop()?;
                let value2 = self.pop()?;
                self.push(value1)?;
                self.push(value2)?;
            }
            Instruction::LINE { n } => {
                if self.opts.verbose {
                    println!("[LOG][DEBUG] Line {}", n);
                }
            }
            Instruction::CALL {
                offset,
                n,
                name,
                builtin,
            } => {
                if !builtin {
                    // Push old instruction pointer
                    // `BEGIN` instruction will collect it
                    self.push(Object::new_unboxed(self.ip as i64))?;

                    if let Some(offset) = offset {
                        self.ip = *offset as usize;
                    } else {
                        panic!(
                            "Calling user-provided function without offset, this should never be possible"
                        );
                    }
                } else {
                    if let Some(name) = name {
                        match name {
                            Builtin::Barray => {
                                let length =
                                    n.ok_or(InterpreterError::InvalidLengthForArray)? as usize;

                                let mut elements = Vec::with_capacity(length);
                                for _ in 0..length {
                                    let element = self.pop()?;
                                    elements.push(element.raw());
                                }

                                elements.reverse();

                                let array = new_array(elements);

                                self.push(
                                    Object::try_from(array)
                                        .map_err(|_| InterpreterError::InvalidObjectPointer)?,
                                )?;
                            }
                            Builtin::Llength => {
                                let obj = self.pop()?;
                                let as_ptr = obj
                                    .as_ptr_mut()
                                    .ok_or(InterpreterError::InvalidObjectPointer)?;

                                unsafe {
                                    // Llength returns a boxed length value
                                    let length = Llength(as_ptr);
                                    self.push(Object::new_boxed(rtUnbox(length)))?;
                                }
                            }
                            Builtin::Lread => unsafe {
                                let val = Lread();

                                // Returns BOXED value
                                self.push(Object::new_boxed(rtUnbox(val)))?;
                            },
                            Builtin::Lwrite => {
                                let obj = self.pop()?;

                                unsafe {
                                    // Lwrite takes a boxed value
                                    Lwrite(rtBox(obj.unwrap()));
                                }

                                self.push(obj)?;
                            }
                            Builtin::Lstring => {
                                let obj = self.pop()?;

                                let mut slice = vec![obj.raw()];

                                unsafe {
                                    let ptr = Lstring(slice.as_mut_ptr());
                                    let contents = (*rtToData(ptr)).contents.as_ptr();

                                    if self.opts.verbose {
                                        let c_str = CStr::from_ptr(contents);
                                        let string = c_str
                                            .to_str()
                                            .map_err(|_| InterpreterError::InvalidStringPointer)?;
                                        println!(
                                            "[LOG][Lstring] Created string: {} from {}",
                                            string.to_string(),
                                            obj.unwrap()
                                        );
                                    }

                                    self.push(
                                        Object::try_from(contents)
                                            .map_err(|_| InterpreterError::InvalidStringPointer)?,
                                    )?;
                                }
                            }
                        }
                    } else {
                        panic!(
                            "Calling builtin function without name, this should never be possible"
                        );
                    }
                }
            }
            Instruction::CJMP { offset, kind } => match kind {
                CompareJumpKind::ISNONZERO => {
                    let obj = self.pop()?;
                    let value = obj.unwrap();

                    if value != 0 {
                        self.ip = *offset as usize;
                    }
                }
                CompareJumpKind::ISZERO => {
                    let obj = self.pop()?;
                    let value = obj.unwrap();

                    if value == 0 {
                        self.ip = *offset as usize;
                    }
                }
            },
            Instruction::ELEM => {
                let index_obj = self.pop()?;
                let mut obj = self.pop()?;

                let index = index_obj.unwrap() as usize;

                // check for aggregate
                if obj.lama_type().is_none() {
                    return Err(InterpreterError::InvalidType(String::from(
                        "indexing into a type that is not an aggregate",
                    )));
                }

                unsafe {
                    let length = rtUnbox(Llength(obj.as_ptr_mut().unwrap())) as usize;

                    // check for out of bounds access
                    if (index_obj.unwrap()) < 0 || index >= length {
                        return Err(InterpreterError::OutOfBoundsAccess(index, length));
                    }

                    let as_ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    if obj.lama_type().unwrap() == lama_type_SEXP {
                        let sexp = rtToSexp(as_ptr);
                        let element = get_sexp_el(&*sexp, index);

                        // push the boxed element onto the stack
                        self.push(Object::new_boxed(element))?;
                    } else if obj.lama_type().unwrap() == lama_type_STRING {
                        let contents = (*rtToData(as_ptr)).contents.as_ptr();

                        let el = contents.add(index);

                        if self.opts.verbose {
                            println!(
                                "[LOG][ELEM] Accessing string element at index {}: {}",
                                index, *el
                            );
                        }

                        self.push(Object::new_boxed(*el as i64))?;
                    } else {
                        let array = rtToData(as_ptr);
                        let element = get_array_el(&*array, index);

                        // push the boxed element onto the stack
                        self.push(Object::new_boxed(rtUnbox(element)))?;
                    }
                }
            }
            Instruction::ARRAY { n } => unsafe {
                let mut obj = self.pop()?;

                if let Some(lama_type) = obj.lama_type() {
                    // check aggregate type
                    if lama_type != lama_type_ARRAY {
                        self.push(Object::new_boxed(0))?;
                    } else {
                        // get length of array
                        let length = Llength(
                            obj.as_ptr_mut()
                                .ok_or(InterpreterError::InvalidObjectPointer)?,
                        );

                        // check length
                        if rtUnbox(length) as i32 == *n {
                            self.push(Object::new_boxed(1))?;
                        } else {
                            self.push(Object::new_boxed(0))?;
                        }
                    }
                } else {
                    self.push(Object::new_boxed(0))?;
                }
            },
            _ => panic!("Unimplemented instruction {:?}", instr),
        };

        Ok(())
    }

    unsafe fn gc_sync(&mut self) -> Result<(), InterpreterError> {
        if self.operand_stack.is_empty() {
            return Err(InterpreterError::StackUnderflow);
        }

        unsafe {
            gc_set_top(
                self.operand_stack
                    .as_ptr()
                    .add(self.operand_stack.len() - 1)
                    .addr(),
            );
            gc_set_bottom(self.operand_stack.as_ptr().addr());
        }

        Ok(())
    }

    /// Push to the operand stack
    fn push(&mut self, obj: Object) -> Result<(), InterpreterError> {
        self.operand_stack.push(obj);
        if self.opts.verbose {
            println!("[LOG] STACK PUSH");
            self.print_stack();
        }

        unsafe {
            self.gc_sync()?;
        }

        Ok(())
    }

    /// Pop from the operand stack
    fn pop(&mut self) -> Result<Object, InterpreterError> {
        let obj = self
            .operand_stack
            .pop()
            .ok_or(InterpreterError::StackUnderflow);
        if self.opts.verbose {
            println!("[LOG] STACK POP");
            self.print_stack();
        }

        unsafe {
            self.gc_sync()?;
        }

        obj
    }

    fn print_stack(&self) {
        println!("---------------- STACK BEGIN --------------");
        for (i, obj) in self.operand_stack.iter().enumerate() {
            if i == self.frame_pointer {
                println!("[{}] {} <- frame_pointer", i, obj);
            } else if i == self.frame_pointer + 1 {
                println!("[{}] {} <- argn", i, obj);
            } else if i == self.frame_pointer + 2 {
                println!("[{}] {} <- localn", i, obj);
            } else if i == self.frame_pointer + 3 {
                println!("[{}] {} <- old frame pointer", i, obj);
            } else if i == self.frame_pointer + 4 {
                println!("[{}] {} <- return ip", i, obj);
            } else {
                println!("[{}] {}", i, obj);
            }
        }
        println!("---------------- STACK END   --------------");
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        __gc_init, Bsexp, LtagHash, alloc_sexp, get_array_el, get_object_content_ptr, get_sexp_el,
        isUnboxed, lama_type_SEXP, lama_type_STRING, new_array, new_sexp, rtBox, rtLen, rtSexpEl,
        rtTag, rtToData, rtToSexp, sexp,
    };

    use super::*;
    use std::{ffi::CStr, os::raw::c_void, ptr};

    /// Test minimal decoder functionality of translating bytecode to instructions
    #[test]
    fn test_decoder_minimal() -> Result<(), Box<dyn std::error::Error>> {
        // ~ =>  xxd dump/test1.bc
        // 00000000: 0500 0000 0100 0000 0100 0000 0000 0000  ................
        // 00000010: 0000 0000 6d61 696e 0052 0200 0000 0000  ....main.R......
        // 00000020: 0000 1002 0000 0010 0300 0000 015a 0100  .............Z..
        // 00000030: 0000 4000 0000 0018 5a02 0000 005a 0400  ..@.....Z....Z..
        // 00000040: 0000 2000 0000 0071 16ff                 .. ....q..
        let data: Vec<u8> = vec![
            0x05, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x52, 0x02, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x00, 0x00, 0x10, 0x03, 0x00,
            0x00, 0x00, 0x01, 0x5a, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x18,
            0x5a, 0x02, 0x00, 0x00, 0x00, 0x5a, 0x04, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
            0x00, 0x71, 0x16, 0xff,
        ];

        let bytefile: Bytefile = Bytefile::parse(data)?;

        let mut interp = Interpreter::new(
            bytefile,
            InterpreterOpts {
                parse_only: true,
                verbose: true,
            },
        );

        interp.run()?;

        // We expect the following instructions to be generated:
        // /LABEL ("main")
        // BEGIN ("main", 2, 0, [], [], [])
        // /SLABEL ("L1")
        // CONST (2)
        // CONST (3)
        // BINOP ("+")
        // /LINE (1)
        // ST (Global ("z"))
        // DROP
        // /LINE (2)
        // /LINE (4)
        // LD (Global ("z"))
        // CALL ("Lwrite", 1, false)
        // /SLABEL ("L2")
        // END

        assert_eq!(
            interp.instructions[0],
            Instruction::BEGIN { args: 2, locals: 0 }
        );
        assert_eq!(interp.instructions[1], Instruction::CONST { value: 2 });
        assert_eq!(interp.instructions[2], Instruction::CONST { value: 3 });
        assert_eq!(interp.instructions[3], Instruction::BINOP { op: Op::ADD });
        assert_eq!(
            interp.instructions[5],
            Instruction::STORE {
                rel: ValueRel::Global,
                index: 0
            }
        );
        assert_eq!(interp.instructions[6], Instruction::DROP);
        assert_eq!(
            interp.instructions[9],
            Instruction::LOAD {
                rel: ValueRel::Global,
                index: 0
            }
        );
        assert_eq!(
            interp.instructions[10],
            Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::Lwrite),
                builtin: true
            }
        );
        assert_eq!(interp.instructions[11], Instruction::END);

        Ok(())
    }

    /// Test minimal evaluation functionality of the interpreter
    #[test]
    fn test_binops_eval() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();
        for i in 1u8..=13u8 {
            let program = vec![
                Instruction::CONST { value: 2 },
                Instruction::CONST { value: 3 },
                Instruction::BINOP {
                    op: Op::try_from(i).unwrap(),
                },
            ];
            programs.push(program);
        }

        // tests on 0
        programs.push(vec![
            Instruction::CONST { value: 0 },
            Instruction::CONST { value: 0 },
            Instruction::BINOP { op: Op::AND },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 0 },
            Instruction::CONST { value: 1 },
            Instruction::BINOP { op: Op::OR },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 0 },
            Instruction::CONST { value: 0 },
            Instruction::BINOP { op: Op::OR },
        ]);

        // equality
        programs.push(vec![
            Instruction::CONST { value: 1 },
            Instruction::CONST { value: 1 },
            Instruction::BINOP { op: Op::EQ },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 1 },
            Instruction::CONST { value: 1 },
            Instruction::BINOP { op: Op::NEQ },
        ]);

        let expected_results = vec![
            5,  // 2 + 3 = 5
            -1, // 2 - 3 = 5
            6,  // 2 * 3 = 6
            0,  // 2 / 3 = 0
            2,  // 2 % 3 = 2
            1,  // 2 < 3 = 1
            1,  // 2 <= 3 = 1
            0,  // 2 > 3 = 0
            0,  // 2 >= 3 = 0
            0,  // 2 == 3 = 0
            1,  // 2 != 3 = 1
            1,  // 2 && 3 = 1
            1,  // 2 != 3 = 1
            0,  // 0 && 0 = 0
            1,  // 0 || 1 = 1
            0,  // 0 || 0 = 0
            1,  // 1 == 1 = 1
            0,  // 1 != 1 = 0
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
            interp.run_on_program(program)?;

            let top = interp.pop().unwrap();

            assert_eq!(top.unwrap(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_string_eval() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // "main"
        programs.push(vec![Instruction::STRING { index: 0 }]);

        // "Hello"
        programs.push(vec![Instruction::STRING { index: 5 }]);

        let expected_results = vec![CString::new("main")?, CString::new("Hello, world!")?];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("main")?);
            interp.bf.put_string(CString::new("Hello, world!")?);

            println!("{}", interp.bf);

            interp.run_on_program(program)?;

            let mut obj = interp.pop().unwrap();

            assert_eq!(obj.lama_type(), Some(lama_type_STRING));

            unsafe {
                let as_ptr = obj.as_ptr_mut().ok_or("Failed to get pointer")?;
                let contents = (*rtToData(as_ptr)).contents.as_ptr();
                let c_string_again = CStr::from_ptr(contents);

                assert_eq!(*c_string_again, expected);
            }
        }

        Ok(())
    }

    #[test]
    fn test_sexp_cons_nil_eval() -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            __gc_init();
        }

        let mut programs = Vec::new();

        programs.push(vec![
            // Nil()
            Instruction::SEXP {
                s_index: 5,
                n_members: 0,
            },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 1 },
            // Nil()
            Instruction::SEXP {
                s_index: 5,
                n_members: 0,
            },
            // Cons(1, Nil())
            Instruction::SEXP {
                s_index: 0,
                n_members: 2,
            },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 1 },
            Instruction::CONST { value: 2 },
            // Nil()
            Instruction::SEXP {
                s_index: 5,
                n_members: 0,
            },
            // Cons(1, Nil())
            Instruction::SEXP {
                s_index: 0,
                n_members: 2,
            },
            // Cons(2, Cons(1, Nil()))
            Instruction::SEXP {
                s_index: 0,
                n_members: 2,
            },
        ]);

        struct Expect {
            tag: i64,
            contents: Vec<i64>,
        }

        // checking tags and contents
        let expected_results = vec![
            Expect {
                tag: unsafe { rtUnbox(LtagHash(CString::new("Nil")?.into_raw())) },
                contents: vec![],
            },
            Expect {
                tag: unsafe { rtUnbox(LtagHash(CString::new("Cons")?.into_raw())) },
                contents: vec![1],
            },
            Expect {
                tag: unsafe { rtUnbox(LtagHash(CString::new("Cons")?.into_raw())) },
                contents: vec![1, 2],
            },
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("Cons")?);
            interp.bf.put_string(CString::new("Nil")?);

            println!("{}", interp.bf);

            interp.run_on_program(program)?;

            let mut obj = interp.pop().unwrap();
            let sexp = obj.as_ptr_mut::<c_void>().unwrap();

            assert_eq!(obj.lama_type(), Some(lama_type_SEXP));

            unsafe {
                assert_eq!((*rtToSexp(sexp)).tag, expected.tag as u64);

                for (i, el) in expected.contents.iter().enumerate() {
                    // skip tags
                    if i % 2 != 0 {
                        continue;
                    }
                    assert_eq!(rtUnbox(get_sexp_el(&*rtToSexp(sexp), i)), *el);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_invalid_jump_offset() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // negative jump offset
        programs.push(vec![Instruction::JMP { offset: -1 }]);

        // out of bounds jump offset
        programs.push(vec![Instruction::JMP { offset: 10000 }]);

        let expected_results = vec![
            Err(InterpreterError::InvalidJumpOffset(0, -1, 100)),
            Err(InterpreterError::InvalidJumpOffset(0, 10000, 100)),
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
            let result = interp.run_on_program(program);

            assert!(result.is_err());
            assert_eq!(result, expected);
        }

        Ok(())
    }

    #[test]
    fn test_invalid_frame_move() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // End without begin
        programs.push(vec![Instruction::CONST { value: 1 }, Instruction::END]);

        // Not enough arguments for frame metadata
        programs.push(vec![Instruction::BEGIN {
            args: 10,
            locals: 0,
        }]);

        let expected_results = vec![
            Err(InterpreterError::NotEnoughArguments("END")),
            Err(InterpreterError::NotEnoughArguments("BEGIN")),
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
            interp.operand_stack.clear();
            let result = interp.run_on_program(program);

            assert!(result.is_err());
            assert_eq!(result, expected);
        }

        Ok(())
    }

    #[test]
    fn test_frame_move_args_and_locals() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 }, // return ip
            Instruction::BEGIN { args: 2, locals: 2 },
        ]);

        // Local variables assignment
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 }, // return ip
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Local,
                index: 0,
            }, // store `3` in local variable at index 0
            Instruction::CONST { value: 4 },
            Instruction::STORE {
                rel: ValueRel::Local,
                index: 1,
            }, // store `4` in local variable at index 1
        ]);

        // Arguments assignment
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Arg,
                index: 0,
            }, // store `3` in function argument at index 0
            Instruction::CONST { value: 4 },
            Instruction::STORE {
                rel: ValueRel::Arg,
                index: 1,
            }, // store `4` in function argument at index 1
        ]);

        struct ExpectValue {
            metadata: FrameMetadata,
            args: Vec<Object>,
            locals: Vec<Object>,
        }

        let expected_results = vec![
            ExpectValue {
                metadata: FrameMetadata::new(2, 2, 0, 2),
                args: vec![Object::new_boxed(2), Object::new_boxed(2)],
                // un-initialized locals
                locals: vec![Object::new_boxed(0), Object::new_boxed(0)],
            },
            ExpectValue {
                metadata: FrameMetadata::new(2, 2, 0, 2),
                args: vec![Object::new_boxed(2), Object::new_boxed(2)],
                locals: vec![Object::new_boxed(3), Object::new_boxed(4)],
            },
            ExpectValue {
                metadata: FrameMetadata::new(2, 2, 0, 2),
                args: vec![Object::new_boxed(3), Object::new_boxed(4)],
                locals: vec![Object::new_boxed(0), Object::new_boxed(0)],
            },
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            interp.run_on_program(program)?;

            let frame = FrameMetadata::get_from_stack(&interp.operand_stack, interp.frame_pointer)
                .ok_or(InterpreterError::StackUnderflow)?;

            assert_eq!(frame, expected.metadata);
            assert_eq!(
                frame
                    .get_local_at(&interp.operand_stack, interp.frame_pointer, 0)
                    .unwrap()
                    .unwrap(),
                expected.locals[0].unwrap()
            );
            assert_eq!(
                frame
                    .get_local_at(&interp.operand_stack, interp.frame_pointer, 1)
                    .unwrap()
                    .unwrap(),
                expected.locals[1].unwrap()
            );
            assert_eq!(
                frame
                    .get_arg_at(&interp.operand_stack, interp.frame_pointer, 0)
                    .unwrap()
                    .unwrap(),
                expected.args[0].unwrap()
            );
            assert_eq!(
                frame
                    .get_arg_at(&interp.operand_stack, interp.frame_pointer, 1)
                    .unwrap()
                    .unwrap(),
                expected.args[1].unwrap()
            );
        }

        Ok(())
    }

    #[test]
    fn test_invalid_args_and_locals_assignment() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // Local variables assignment
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 }, // return ip
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Local,
                index: 3,
            }, // invalid store `3` in local variable at index 3
        ]);

        // Arguments assignment
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Arg,
                index: 3,
            }, // invalid store `3` in function argument at index 0
        ]);

        let expected_results = vec![
            Err(InterpreterError::InvalidStoreIndex(ValueRel::Local, 3, 2)),
            Err(InterpreterError::InvalidStoreIndex(ValueRel::Arg, 3, 2)),
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            let result = interp.run_on_program(program);

            assert!(result.is_err());
            assert_eq!(result, expected);
        }

        Ok(())
    }

    #[test]
    fn test_arg_and_local_load() -> Result<(), InterpreterError> {
        let mut programs = Vec::new();

        // Loading uninitialized local variable
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 }, // return ip
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::LOAD {
                rel: ValueRel::Local,
                index: 0,
            }, // load local variable at index 0
        ]);

        // Arguments load
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CONST { value: 4 },
            Instruction::CONST { value: 5 },
            Instruction::CONST { value: 2 }, // ip
            Instruction::BEGIN { args: 4, locals: 2 },
            Instruction::LOAD {
                rel: ValueRel::Arg,
                index: 3,
            }, // load function argument at index 3
        ]);

        // Load mutated argument
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 }, // ip
            Instruction::BEGIN { args: 4, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Arg,
                index: 0,
            }, // store `3` in function argument at index 0
            Instruction::LOAD {
                rel: ValueRel::Arg,
                index: 0,
            }, // load function argument at index 0
        ]);

        // Load mutated local
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 2 },
            Instruction::BEGIN { args: 2, locals: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STORE {
                rel: ValueRel::Local,
                index: 0,
            }, // invalid store `3` in function argument at index 0
            Instruction::LOAD {
                rel: ValueRel::Local,
                index: 0,
            }, // load function argument at index 0
        ]);

        let expected_results = vec![0, 5, 3, 3];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            interp.run_on_program(program)?;

            let obj = interp.pop()?;

            assert_eq!(obj.unwrap(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_drop() -> Result<(), InterpreterError> {
        let mut programs = Vec::new();

        // Loading uninitialized local variable
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::DROP,
        ]);

        let expected_results = vec![2];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            interp.run_on_program(program)?;

            let obj = interp.pop()?;

            assert_eq!(obj.unwrap(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_dup() -> Result<(), InterpreterError> {
        let mut programs = Vec::new();

        // Loading uninitialized local variable
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::DUP,
        ]);

        let expected_results = vec![(3, 3)];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            interp.run_on_program(program)?;

            let (obj1, obj2) = (interp.pop()?, interp.pop()?);

            assert_eq!(obj1.unwrap(), expected.0);
            assert_eq!(obj2.unwrap(), expected.1);
        }

        Ok(())
    }

    #[test]
    fn test_swap() -> Result<(), InterpreterError> {
        let mut programs = Vec::new();

        // Loading uninitialized local variable
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::SWAP,
        ]);

        let expected_results = vec![(2, 3)];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, false));

            interp.run_on_program(program)?;

            let (obj1, obj2) = (interp.pop()?, interp.pop()?);

            assert_eq!(obj1.unwrap(), expected.0);
            assert_eq!(obj2.unwrap(), expected.1);
        }

        Ok(())
    }

    #[test]
    fn test_array() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // 0 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(0),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::Llength),
                builtin: true,
            },
        ]);

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::Llength),
                builtin: true,
            },
        ]);

        // TODO: Array of different types of elements (this is allowed in Lama)
        // programs.push(vec![
        //     Instruction::CONST { value: 1 },
        //     Instruction::CONST { value: 2 },
        //     Instruction::STRING { index: 0 }, // "main"
        //     Instruction::CALL {
        //         offset: None,
        //         n: Some(3),
        //         name: Some(Builtin::Barray),
        //         builtin: true,
        //     },
        //     Instruction::DUP,
        //     Instruction::CALL {
        //         offset: None,
        //         n: None,
        //         name: Some(Builtin::Llength),
        //         builtin: true,
        //     },
        // ]);

        let c_string = CString::new("main")?;
        let raw_ptr = c_string.as_ptr();
        let object = Object::try_from(raw_ptr).unwrap();

        let expected_results = vec![vec![], vec![2, 3]];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("main")?);

            interp.run_on_program(program)?;

            let (len, obj) = (interp.pop()?, interp.pop()?);

            unsafe {
                assert_eq!(len.unwrap() as usize, expected.len());
                for (i, &value) in expected.iter().enumerate() {
                    assert_eq!(
                        rtUnbox(get_array_el(
                            &*rtToData(
                                obj.as_ptr_mut()
                                    .ok_or(InterpreterError::InvalidLengthForArray)?
                            ),
                            i
                        )),
                        value
                    );
                }
            }
        }

        Ok(())
    }

    // Note: no Lwrite/Lread testing
    #[test]
    fn test_builtin_functions() -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            __gc_init();
        }

        let mut programs = Vec::new();

        // Llength of SEXP
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CONST { value: 1 },
            // Nil()
            Instruction::SEXP {
                s_index: 5,
                n_members: 0,
            },
            // Cons(1, Nil())
            Instruction::SEXP {
                s_index: 0,
                n_members: 2,
            },
            // Cons(3, Cons(1, Nil()))
            Instruction::SEXP {
                s_index: 0,
                n_members: 2,
            },
            Instruction::DUP,
            Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::Llength),
                builtin: true,
            },
        ]);

        // Llength of string
        programs.push(vec![
            Instruction::STRING {
                index: 0, // "Cons"
            },
            Instruction::CALL {
                offset: None,
                n: None,
                name: Some(Builtin::Llength),
                builtin: true,
            },
        ]);

        // TODO: Llength of closure

        let expected_results = vec![2, 4];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("Cons")?);
            interp.bf.put_string(CString::new("Nil")?);

            interp.run_on_program(program)?;

            let obj = interp.pop()?;

            assert_eq!(obj.unwrap(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_conditional_jump() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        programs.push(vec![
            Instruction::CONST { value: 1 },
            Instruction::CJMP {
                offset: 10,
                kind: CompareJumpKind::ISNONZERO,
            },
            Instruction::CONST { value: 1 },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 0 },
            Instruction::CJMP {
                offset: 10,
                kind: CompareJumpKind::ISNONZERO,
            },
            Instruction::CONST { value: 1 },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 1 },
            Instruction::CJMP {
                offset: 10,
                kind: CompareJumpKind::ISZERO,
            },
            Instruction::CONST { value: 1 },
        ]);

        programs.push(vec![
            Instruction::CONST { value: 0 },
            Instruction::CJMP {
                offset: 10,
                kind: CompareJumpKind::ISZERO,
            },
            Instruction::CONST { value: 1 },
        ]);

        // expected ip
        let expected_results = vec![10, 0, 0, 10];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));
            interp.run_on_program(program)?;

            let ip = interp.ip;

            assert_eq!(ip, expected);
        }

        Ok(())
    }

    #[test]
    fn test_elem() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CONST { value: 1 }, // array[1] => 3
            Instruction::ELEM,
        ]);

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CONST { value: 0 }, // array[0] => 2
            Instruction::ELEM,
        ]);

        // string
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::STRING { index: 0 }, // "main"
            Instruction::DUP,
            Instruction::CONST { value: 0 }, // 'm'
            Instruction::ELEM,
        ]);

        let expected_results = vec![3, 2, 109];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("main")?);

            interp.run_on_program(program)?;

            let obj = interp.pop()?;
            assert_eq!(obj.unwrap(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_invalid_elem() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // 0 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(0),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CONST { value: 1 },
            Instruction::ELEM,
        ]);

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::CONST { value: 3 }, // array[3]
            Instruction::ELEM,
        ]);

        let expected_results = vec![
            InterpreterError::OutOfBoundsAccess(1, 0),
            InterpreterError::OutOfBoundsAccess(3, 2),
        ];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("main")?);

            let result = interp.run_on_program(program);

            assert!(result.is_err());
            assert_eq!(result.unwrap_err(), expected);
        }

        Ok(())
    }

    #[test]
    fn test_array_tag() -> Result<(), Box<dyn std::error::Error>> {
        let mut programs = Vec::new();

        // 0 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(0),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::ARRAY { n: 0 }, // => 1
        ]);

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::ARRAY { n: 2 }, // => 1
        ]);

        // 2 length array
        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::CALL {
                offset: None,
                n: Some(2),
                name: Some(Builtin::Barray),
                builtin: true,
            },
            Instruction::DUP,
            Instruction::ARRAY { n: 3 }, // => 0
        ]);

        programs.push(vec![
            Instruction::CONST { value: 2 },
            Instruction::CONST { value: 3 },
            Instruction::ARRAY { n: 0 }, // => 0
        ]);

        let expected_results = vec![1, 1, 0, 0];

        assert_eq!(programs.len(), expected_results.len());

        for (program, expected) in programs.into_iter().zip(expected_results) {
            let mut interp =
                Interpreter::new(Bytefile::new_dummy(), InterpreterOpts::new(false, true));

            interp.bf.put_string(CString::new("main")?);

            interp.run_on_program(program)?;

            let obj = interp.pop()?;

            assert_eq!(obj.unwrap(), expected);
        }

        Ok(())
    }
}
