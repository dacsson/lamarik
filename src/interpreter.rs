//! VM Interpreter

use crate::bytecode::{Builtin, CapturedVar, CompareJumpKind, Instruction, Op, PattKind, ValueRel};
use crate::disasm::Bytefile;
use crate::frame::FrameMetadata;
use crate::numeric::LeBytes;
use crate::object::{Object, ObjectError};
use crate::{
    __gc_init, __gc_stack_bottom, __gc_stack_top, Barray_tag_patt, Bboxed_patt, Bclosure_tag_patt,
    Bsexp_tag_patt, Bstring_patt, Bstring_tag_patt, Bunboxed_patt, CONS_TAG_HASH, Llength, Lread,
    Lstring, LtagHash, Lwrite, NIL_TAG_HASH, gc_set_bottom, gc_set_top, get_array_el,
    get_captured_variable, get_sexp_el, lama_type_ARRAY, lama_type_CLOSURE, lama_type_SEXP,
    lama_type_STRING, new_array, new_closure, new_sexp, new_string, rtBox, rtToData, rtToSexp,
    rtUnbox, set_array_el, set_captured_variable, set_sexp_el,
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
    Fail {
        line: usize,
        column: usize,
        obj: String,
    },
    InvalidValueRel,
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
        }
    }
}

impl std::error::Error for InterpreterError {}

#[derive(Debug, Clone)]
pub struct InstructionTrace {
    pub instruction: Instruction,
    pub offset: usize,
}

pub struct Interpreter {
    operand_stack: Vec<Object>,
    frame_pointer: usize,
    /// Decoded bytecode file with raw code section
    bf: Bytefile,
    /// Instruction pointer, moves along code section in `bf`
    ip: usize,
    /// Collect found instructions, only when `parse_only` is true
    instructions: Vec<InstructionTrace>,
    /// Global variables
    globals: Vec<Object>,
    /// Code section length
    code_section_len: usize,
}

const MAX_OPERAND_STACK_SIZE: usize = 0xffff;

impl Interpreter {
    /// Create a new interpreter with operand stack filled with
    /// emulated call to main
    pub fn new(bf: Bytefile) -> Self {
        let mut operand_stack = Vec::with_capacity(MAX_OPERAND_STACK_SIZE);

        unsafe {
            __gc_init();
        }

        // Emulating call to main
        operand_stack.push(Object::new_empty()); // FRAME_PTR
        operand_stack.push(Object::new_empty()); // CLOSURE_OBJ
        operand_stack.push(Object::new_unboxed(2)); // ARGS_COUNT
        operand_stack.push(Object::new_empty()); // LOCALS_COUNT
        operand_stack.push(Object::new_empty()); // OLD_FRAME_POINTER
        operand_stack.push(Object::new_empty()); // OLD_IP
        operand_stack.push(Object::new_empty()); // ARGV
        operand_stack.push(Object::new_empty()); // ARGC
        operand_stack.push(Object::new_empty()); // CURR_IP

        let global_areas_size = bf.global_area_size as usize;
        let code_section_len = bf.code_section.len();

        Interpreter {
            operand_stack,
            frame_pointer: 0,
            bf,
            ip: 0,
            instructions: Vec::new(),
            globals: vec![Object::new_empty(); global_areas_size],
            code_section_len,
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
        #[cfg(feature = "runtime_checks")]
        if self.ip + std::mem::size_of::<T>() > self.code_section_len {
            return Err(InterpreterError::ReadingMoreThenCodeSection);
        }

        let bit_size = std::mem::size_of::<T>();
        let bytes = &self.bf.code_section[self.ip..self.ip + bit_size];

        self.ip += bit_size;

        Ok(T::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub fn collect_instructions(&mut self) -> Result<&Vec<InstructionTrace>, InterpreterError> {
        while self.ip < self.code_section_len {
            let opcode_offset = self.ip;

            let encoding = self.next::<u8>()?;

            if encoding == 0xff {
                break;
            }

            let instr = self.decode(encoding)?;

            if cfg!(feature = "verbose") {
                println!("[LOG] IP {} BYTE {} INSTR {:?}", self.ip, encoding, instr);
            }

            self.instructions.push(InstructionTrace {
                instruction: instr,
                offset: opcode_offset,
            });
        }

        Ok(&self.instructions)
    }

    /// Main interpreter loop
    pub fn run(&mut self) -> Result<(), InterpreterError> {
        while self.ip < self.code_section_len {
            let encoding = self.next::<u8>()?;
            let instr = self.decode(encoding)?;

            if cfg!(feature = "verbose") {
                println!("[LOG] IP {} BYTE {} INSTR {:?}", self.ip, encoding, instr);
            }

            self.eval(&instr)?;

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
        let (opcode, subopcode) = (byte & 0xF0, byte & 0x0F);

        match (opcode, subopcode) {
            (0x00, 0x0) => Ok(Instruction::NOP),
            (0x00, _) if (0x1..=0xd).contains(&subopcode) => Ok(Instruction::BINOP {
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
                    captured.push(CapturedVar {
                        rel: ValueRel::try_from(self.next::<u8>()?)
                            .map_err(|_| InterpreterError::InvalidValueRel)?,
                        index: self.next::<i32>()?,
                    });
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
        if cfg!(feature = "verbose") {
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

                if cfg!(feature = "verbose") {
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

                if cfg!(feature = "verbose") {
                    println!("[LOG][STRING] string: {:?}", string);
                }

                let lama_string =
                    new_string(string).map_err(|_| InterpreterError::InvalidStringPointer)?;

                self.push(
                    Object::try_from(lama_string)
                        .map_err(|_| InterpreterError::InvalidStringPointer)?,
                )?;

                if cfg!(feature = "verbose") {
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

                if cfg!(feature = "verbose") {
                    println!(
                        "[LOG][Instruction::SEXP] tag_u8: {:#?}, index {}",
                        tag_u8, s_index
                    );
                }

                let c_string = CString::from_vec_with_nul(tag_u8)
                    .map_err(|_| InterpreterError::InvalidCString)?;

                if cfg!(feature = "verbose") {
                    println!(
                        "[LOG][Instruction::SEXP] c_string: {}",
                        c_string.to_str().unwrap()
                    );
                }

                let mut args = vec![0; *n_members as usize];
                //Vec::with_capacity(*n_members as usize);
                // args.resize(*n_members as usize, 0);

                for i in (0..*n_members).rev() {
                    // args.push(self.pop()?.raw());
                    args[i as usize] = self.pop()?.raw();
                }

                // Reverse the arguments to match the order of the SEXP constructor
                // args.reverse();

                let sexp = new_sexp(c_string, args);

                if cfg!(feature = "verbose") {
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
                #[cfg(feature = "runtime_checks")]
                if (*offset) < 0 || offset_at >= self.code_section_len {
                    return Err(InterpreterError::InvalidJumpOffset(
                        self.ip,
                        *offset,
                        self.code_section_len,
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
                #[cfg(feature = "runtime_checks")]
                if aggregate.lama_type().is_none() {
                    return Err(InterpreterError::InvalidType(String::from(
                        "Expected an aggregate type in STA instruction",
                    )));
                }

                unsafe {
                    let length = rtUnbox(Llength(aggregate.as_ptr_mut().unwrap())) as usize;

                    // check for out of bounds access
                    #[cfg(feature = "runtime_checks")]
                    if (index_obj.unwrap()) < 0 || index >= length {
                        return Err(InterpreterError::OutOfBoundsAccess(index, length));
                    }

                    let as_ptr = aggregate
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let lama_type = aggregate.lama_type().unwrap();

                    if lama_type == lama_type_SEXP {
                        let sexp = rtToSexp(as_ptr);
                        set_sexp_el(&mut *sexp, index, value);
                    } else if lama_type == lama_type_STRING {
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
            Instruction::BEGIN { args, locals } | Instruction::CBEGIN { args, locals } => {
                let mut closure_obj = Object::new_empty();
                let mut ret_ip = Object::new_empty();

                // Top object is either return_ip or a closure obj
                let mut obj = self
                    .pop()
                    .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?; // must be a closure

                // check for closure
                if let Some(lama_type) = obj.lama_type() {
                    // check for closure type
                    if lama_type == lama_type_CLOSURE {
                        closure_obj = obj;

                        // Save previous ip (provided by `CALL`)
                        ret_ip = self
                            .pop()
                            .map_err(|_| InterpreterError::NotEnoughArguments("BEGIN"))?;
                    }
                } else {
                    ret_ip = obj;
                }

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
                #[cfg(feature = "runtime_checks")]
                if self.operand_stack.is_empty() {
                    return Err(InterpreterError::NotEnoughArguments("BEGIN"));
                }
                self.frame_pointer = self.operand_stack.len() - 1;

                // Push closure object onto operand stack
                self.push(closure_obj)?;

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
                    closure_obj: _,
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

                // Pop closure object
                self.pop()?;
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
                    ValueRel::Capture => unsafe {
                        let closure = frame
                            .get_closure(&mut self.operand_stack, self.frame_pointer)
                            .map_err(|_| {
                                InterpreterError::InvalidStoreIndex(ValueRel::Capture, *index, 1)
                            })?;

                        let to_data = rtToData(
                            closure
                                .as_ptr_mut()
                                .ok_or(InterpreterError::InvalidObjectPointer)?,
                        );

                        set_captured_variable(&mut *to_data, *index as usize, value.raw());
                    },
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
                let mut frame =
                    FrameMetadata::get_from_stack(&self.operand_stack, self.frame_pointer)
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
                    ValueRel::Capture => unsafe {
                        let closure = frame
                            .get_closure(&mut self.operand_stack, self.frame_pointer)
                            .map_err(|_| {
                                InterpreterError::InvalidStoreIndex(ValueRel::Capture, *index, 1)
                            })?;

                        let to_data = rtToData(
                            closure
                                .as_ptr_mut()
                                .ok_or(InterpreterError::InvalidObjectPointer)?,
                        );

                        let element = get_captured_variable(&*to_data, *index as usize);

                        self.push(Object::Boxed(element))?;
                    },
                    ValueRel::Global => {
                        #[cfg(feature = "runtime_checks")]
                        if (*index as usize) >= self.globals.len() {
                            return Err(InterpreterError::InvalidStoreIndex(
                                ValueRel::Global,
                                *index,
                                self.globals.len() as i64,
                            ));
                        }

                        let value = self.globals[*index as usize].clone();
                        self.push(value)?;
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
                if cfg!(feature = "verbose") {
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

                                let mut elements = vec![0; length];
                                for i in (0..length).rev() {
                                    elements[i as usize] = self.pop()?.raw();
                                }

                                // elements.reverse();

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

                                    if cfg!(feature = "verbose") {
                                        let c_str = CStr::from_ptr(contents);
                                        let string = c_str
                                            .to_str()
                                            .map_err(|_| InterpreterError::InvalidStringPointer)?;
                                        println!(
                                            "[LOG][Lstring] Created string: {} from {}",
                                            string,
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
                #[cfg(feature = "runtime_checks")]
                if obj.lama_type().is_none() {
                    return Err(InterpreterError::InvalidType(String::from(
                        "indexing into a type that is not an aggregate",
                    )));
                }

                unsafe {
                    let length = rtUnbox(Llength(obj.as_ptr_mut().unwrap())) as usize;

                    // check for out of bounds access
                    #[cfg(feature = "runtime_checks")]
                    if (index_obj.unwrap()) < 0 || index >= length {
                        return Err(InterpreterError::OutOfBoundsAccess(index, length));
                    }

                    let as_ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let lama_type = obj.lama_type().unwrap();

                    if lama_type == lama_type_SEXP {
                        let sexp = rtToSexp(as_ptr);
                        let element = get_sexp_el(&*sexp, index);

                        // push the boxed element onto the stack
                        self.push(Object::Boxed(element))?;
                    } else if lama_type == lama_type_STRING {
                        let contents = (*rtToData(as_ptr)).contents.as_ptr();

                        let el = contents.add(index);

                        if cfg!(feature = "verbose") {
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
                        self.push(Object::Boxed(element))?;
                    }
                }
            }
            Instruction::ARRAY { n } => unsafe {
                let mut obj = self.pop()?;

                let Some(lama_type) = obj.lama_type() else {
                    self.push(Object::new_boxed(0))?;
                    return Ok(());
                };

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
            },
            Instruction::TAG { index, n } => unsafe {
                let mut obj = self.pop()?;

                let Some(lama_type) = obj.lama_type() else {
                    self.push(Object::new_boxed(0))?;
                    return Ok(());
                };

                // check aggregate type
                if lama_type != lama_type_SEXP {
                    self.push(Object::new_boxed(0))?;
                } else {
                    // get length of sexp
                    let length = Llength(
                        obj.as_ptr_mut()
                            .ok_or(InterpreterError::InvalidObjectPointer)?,
                    );

                    // check length
                    if rtUnbox(length) as i32 == *n {
                        let sexp = rtToSexp(
                            obj.as_ptr_mut()
                                .ok_or(InterpreterError::InvalidObjectPointer)?,
                        );

                        let tag_u8 = self
                            .bf
                            .get_string_at_offset(*index as usize)
                            .map_err(|_| InterpreterError::StringIndexOutOfBounds)?;

                        let c_string = CString::from_vec_with_nul(tag_u8)
                            .map_err(|_| InterpreterError::InvalidCString)?;

                        let hashed_string = if c_string.to_bytes() == "cons".as_bytes() {
                            CONS_TAG_HASH
                        } else if c_string.to_bytes() == "nil".as_bytes() {
                            NIL_TAG_HASH
                        } else {
                            LtagHash(c_string.into_raw())
                        };

                        if rtBox((*sexp).tag as i64) == hashed_string {
                            self.push(Object::new_boxed(1))?;
                        } else {
                            self.push(Object::new_boxed(0))?;
                        }
                    } else {
                        self.push(Object::new_boxed(0))?;
                    }
                }
            },
            Instruction::FAIL { line, column } => unsafe {
                let obj = self.pop()?;

                let ptr = Lstring(vec![obj.raw()].as_mut_ptr());
                let contents = (*rtToData(ptr)).contents.as_ptr();
                let c_str = CStr::from_ptr(contents);
                let string = c_str
                    .to_str()
                    .map_err(|_| InterpreterError::InvalidStringPointer)?;

                return Err(InterpreterError::Fail {
                    line: *line as usize,
                    column: *column as usize,
                    obj: String::from(string),
                });
            },
            Instruction::CLOSURE {
                offset,
                arity: _,
                captured,
            } => unsafe {
                let offset_at = *offset as usize;

                // verify offset is within bounds
                #[cfg(feature = "runtime_checks")]
                if (*offset) < 0 || offset_at >= self.code_section_len {
                    return Err(InterpreterError::InvalidJumpOffset(
                        self.ip,
                        *offset,
                        self.code_section_len,
                    ));
                }

                // Collect captured variables
                let mut args: Vec<i64> = captured
                    .iter()
                    .map(|desc| match desc.rel {
                        ValueRel::Arg => {
                            let frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;
                            let obj = frame
                                .get_arg_at(
                                    &self.operand_stack,
                                    self.frame_pointer,
                                    desc.index as usize,
                                )
                                .ok_or(InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ))?;
                            Ok::<i64, InterpreterError>(obj.raw())
                        }
                        ValueRel::Capture => {
                            let mut frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;

                            let closure = frame
                                .get_closure(&mut self.operand_stack, self.frame_pointer)
                                .map_err(|_| {
                                    InterpreterError::InvalidLoadIndex(
                                        ValueRel::Capture,
                                        desc.index,
                                        1,
                                    )
                                })?;

                            let to_data = rtToData(
                                closure
                                    .as_ptr_mut()
                                    .ok_or(InterpreterError::InvalidObjectPointer)?,
                            );

                            let element = get_captured_variable(&*to_data, desc.index as usize);

                            Ok(element)
                        }
                        ValueRel::Global => {
                            let value = self.globals.get(desc.index as usize).ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;
                            Ok(value.raw())
                        }
                        ValueRel::Local => {
                            let frame = FrameMetadata::get_from_stack(
                                &self.operand_stack,
                                self.frame_pointer,
                            )
                            .ok_or(
                                InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ),
                            )?;

                            let obj = frame
                                .get_local_at(
                                    &self.operand_stack,
                                    self.frame_pointer,
                                    desc.index as usize,
                                )
                                .ok_or(InterpreterError::NotEnoughArguments(
                                    "trying to create closure frame",
                                ))?;
                            Ok(obj.raw())
                        }
                    })
                    .collect::<Result<_, _>>()?;

                args.insert(0, *offset as i64);

                // Create a new closure object
                let closure = new_closure(args);

                self.push(
                    Object::try_from(closure)
                        .map_err(|_| InterpreterError::InvalidObjectPointer)?,
                )?;
            },
            Instruction::CALLC { arity } => {
                let mut obj = self.take(*arity as usize)?; // must be a closure

                // check for closure
                #[cfg(feature = "runtime_checks")]
                let Some(lama_type) = obj.lama_type() else {
                    return Err(InterpreterError::InvalidObjectPointer);
                };

                // check for closure type
                #[cfg(feature = "runtime_checks")]
                if lama_type != lama_type_CLOSURE {
                    return Err(InterpreterError::InvalidType(
                        "expected closure object at top of the stack to call a closure".into(),
                    ));
                }

                // Push old instruction pointer
                // `CBEGIN` instruction will collect it
                self.push(Object::new_unboxed(self.ip as i64))?;

                // Re-push closure object
                // `CBEGIN` instruction will collect it
                self.push(obj.clone())?;

                unsafe {
                    let to_data = rtToData(
                        obj.as_ptr_mut()
                            .ok_or(InterpreterError::InvalidObjectPointer)?,
                    );
                    // First element in closure object is the offset
                    self.ip = get_array_el(&*to_data, 0) as usize;
                }
            }
            Instruction::PATT { kind } => match kind {
                PattKind::BothAreStr => unsafe {
                    let x = self.pop()?;
                    let y = self.pop()?;

                    let x_ptr = x
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;
                    let y_ptr = y
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bstring_patt(x_ptr, y_ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsStr => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bstring_tag_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsArray => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Barray_tag_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsSExp => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bsexp_tag_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsRef => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bboxed_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsVal => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bunboxed_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
                PattKind::IsLambda => unsafe {
                    let obj = self.pop()?;
                    let ptr = obj
                        .as_ptr_mut()
                        .ok_or(InterpreterError::InvalidObjectPointer)?;

                    let res = Bclosure_tag_patt(ptr);

                    self.push(Object::Boxed(res))?;
                },
            },
            _ => panic!("Unimplemented instruction {:?}", instr),
        };

        Ok(())
    }

    unsafe fn gc_sync(&mut self) -> Result<(), InterpreterError> {
        #[cfg(feature = "runtime_checks")]
        if self.operand_stack.is_empty() {
            return Err(InterpreterError::StackUnderflow);
        }

        unsafe {
            __gc_stack_top =
            // gc_set_top(
                self.operand_stack
                    .as_ptr()
                    .add(self.operand_stack.len() - 1)
                    .addr();
            // );
            // gc_set_bottom(
            __gc_stack_bottom = self.operand_stack.as_ptr().addr()
            // );
        }

        Ok(())
    }

    /// Push to the operand stack
    #[inline(always)]
    fn push(&mut self, obj: Object) -> Result<(), InterpreterError> {
        self.operand_stack.push(obj);
        if cfg!(feature = "verbose") {
            println!("[LOG] STACK PUSH");
            self.print_stack();
        }

        unsafe {
            self.gc_sync()?;
        }

        Ok(())
    }

    /// Pop from the operand stack
    #[inline(always)]
    fn pop(&mut self) -> Result<Object, InterpreterError> {
        let obj = self
            .operand_stack
            .pop()
            .ok_or(InterpreterError::StackUnderflow);
        if cfg!(feature = "verbose") {
            println!("[LOG] STACK POP");
            self.print_stack();
        }

        unsafe {
            self.gc_sync()?;
        }

        obj
    }

    /// Take from the operand stack at `index`, relative to the top of the stack
    /// removes the element and returns it
    fn take(&mut self, index: usize) -> Result<Object, InterpreterError> {
        let relative_index = self.operand_stack.len() - index - 1;

        #[cfg(feature = "runtime_checks")]
        if relative_index >= self.operand_stack.len() {
            return Err(InterpreterError::StackUnderflow);
        }

        let obj = self.operand_stack.remove(relative_index);

        if cfg!(feature = "verbose") {
            println!("[LOG] STACK TAKE {}", index);
            self.print_stack();
        }

        unsafe {
            self.gc_sync()?;
        }

        Ok(obj)
    }

    fn print_stack(&self) {
        println!("---------------- STACK BEGIN --------------");
        for (i, obj) in self.operand_stack.iter().enumerate() {
            if i == self.frame_pointer {
                println!("[{}] {} <- frame_pointer", i, obj);
            } else if i == self.frame_pointer + 1 {
                println!("[{}] {} <- closure", i, obj);
            } else if i == self.frame_pointer + 2 {
                println!("[{}] {} <- argn", i, obj);
            } else if i == self.frame_pointer + 3 {
                println!("[{}] {} <- localn", i, obj);
            } else if i == self.frame_pointer + 4 {
                println!("[{}] {} <- old frame pointer", i, obj);
            } else if i == self.frame_pointer + 5 {
                println!("[{}] {} <- return ip", i, obj);
            } else {
                println!("[{}] {}", i, obj);
            }
        }
        println!("---------------- STACK END   --------------");
    }
}

#[cfg(test)]
mod tests;
