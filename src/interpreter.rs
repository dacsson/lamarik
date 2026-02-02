//! VM Interpreter

use crate::bytecode::{Builtin, Instruction, Op, PattKind, ValueRel};
use crate::disasm::Bytefile;
use crate::numeric::LeBytes;
use crate::object::Object;
use std::convert::TryFrom;
use std::io::{BufReader, Cursor, Read};
use std::panic;

#[derive(Debug)]
enum InterpreterError {
    StackUnderflow,
    EndOfCodeSection,
    ReadingMoreThenCodeSection,
    InvalidOpcode(u8),
    InvalidType(String),
    OutOfBoundsAccess,
    InvalidByteSequence(usize),
}

/// Convert a byte, that couldnt be incoded into an interpreter error.
impl From<u8> for InterpreterError {
    fn from(opcode: u8) -> Self {
        InterpreterError::InvalidOpcode(opcode)
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
            InterpreterError::OutOfBoundsAccess => write!(f, "Out of bounds access"),
            InterpreterError::InvalidByteSequence(ip) => {
                write!(f, "Invalid byte sequence at index {}", ip)
            }
        }
    }
}

impl std::error::Error for InterpreterError {}

/// Frame metadata for the interpreter.
/// Because we have only one stack, we keep index
/// of the frame pointer.
///
/// In operand stack out frame looks like this:
/// ```txt
/// ... <- frame points to this index
/// ARGS_COUNT
/// LOCALS_COUNT
/// OLD_FRAME_POINTER
/// OLD_IP
/// ARG1
/// ARG2
/// ...
/// ARGN
/// LOCAL1
/// LOCAL2
/// ...
/// LOCALN
/// ```
struct FrameMetadata {
    n_locals: i64,
    n_args: i64,
    ret_frame_pointer: usize,
    ret_ip: usize,
}

struct InterpreterOpts {
    parse_only: bool,
    verbose: bool,
}

struct Interpreter {
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

        // Emulating call to main
        operand_stack.push(Object::new_empty()); // FRAME_PTR
        operand_stack.push(Object::new_unboxed(2)); // ARGS_COUNT
        operand_stack.push(Object::new_empty()); // LOCALS_COUNT
        operand_stack.push(Object::new_empty()); // OLD_FRAME_POINTER
        operand_stack.push(Object::new_empty()); // OLD_IP
        operand_stack.push(Object::new_empty()); // ARGV
        operand_stack.push(Object::new_empty()); // ARGC
        operand_stack.push(Object::new_empty()); // CURR_IP

        Interpreter {
            operand_stack,
            frame_pointer: 0,
            bf,
            ip: 0,
            opts,
            instructions: Vec::new(),
            globals: Vec::new(),
        }
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

            if let Instruction::NOP = instr {
                if self.opts.verbose {
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
            } else {
                self.dbgs("Instruction: NOP\n");
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
                index: self.next::<i32>()?,
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
            (0x20, _) if subopcode >= 0x0 && subopcode <= 0x3 => Ok(Instruction::LOAD {
                rel: ValueRel::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
                index: self.next::<i32>()?,
            }),
            (0x30, _) if subopcode >= 0x0 && subopcode <= 0x3 => Ok(Instruction::LOADREF {
                rel: ValueRel::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
                index: self.next::<i32>()?,
            }),
            (0x40, _) if subopcode >= 0x0 && subopcode <= 0x3 => Ok(Instruction::STORE {
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
            (0x60, _) if subopcode >= 0x0 && subopcode <= 0x6 => Ok(Instruction::PATT {
                kind: PattKind::try_from(subopcode).map_err(|_| InterpreterError::from(byte))?,
            }),
            (0x70, _) if subopcode >= 0x0 && subopcode <= 0x3 => Ok(Instruction::CALL {
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
    fn eval(&mut self, instr: &Instruction) -> Result<Instruction, InterpreterError> {
        if self.opts.verbose {
            println!("[LOG] EVAL {:?}", instr);
        }

        match instr {
            _ => panic!("Unimplemented instruction"),
        }
    }

    /// Push to the operand stack
    fn push(&mut self, obj: Object) {
        self.operand_stack.push(obj);
        if self.opts.verbose {
            println!("[LOG] STACK PUSH");
            self.print_stack();
        }
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

    fn dbgs(&self, msg: &str) {
        if self.opts.verbose {
            println!("{}", msg);
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_push_pop() {
//         let mut interp = Interpreter::new(Options::default());
//         interp.push(Object::Integer(42));
//         assert_eq!(interp.pop().unwrap(), Object::Integer(42));
//     }
// }
