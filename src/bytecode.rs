//! Descriptor of Lama bytecode
use std::convert::TryFrom;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    ADD, // +
    SUB, // -
    MUL, // *
    DIV, // /
    MOD, // %
    LT,  // <
    LEQ, // <=
    GT,  // >
    GEQ, // >=
    EQ,  // ==
    NEQ, // !=
    AND, // &&, Tests if both integer operands are non-zero
    OR,  // !!, Tests if either of the operands is non-zero.
}

/// Scoping rule for a value
#[derive(Debug, Clone, PartialEq)]
pub enum ValueRel {
    Global,
    Local,
    Arg,     // Function argument
    Capture, // Captured by closure
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompareJumpKind {
    ISZERO,    // jump if operand is zero
    ISNONZERO, // jump if operand is non-zero
}

/// Builtin functions
#[derive(Debug, Clone, PartialEq)]
pub enum Builtin {
    Lread,
    Lwrite,
    Llength,
    Lstring, // Load string from string table
    Barray,
}

/// Pattern matching kind
#[derive(Debug, Clone, PartialEq)]
pub enum PattKind {
    /// Tests whether the two operands are both strings and
    /// store the same bytes.
    BothAreStr, // "=str"
    /// Tests whether the operand is a string.
    IsStr, // "#string"
    /// Tests whether the operand is an array.
    IsArray, // "#array"
    /// Tests whether the operand is an S-expression.
    IsSExp, // "#sexp"
    /// Tests whether the operand has a boxed representation.
    IsRef, // "#ref"
    /// Tests whether the operand has an unboxed representation.
    IsVal, // "#val"
    /// Tests whether the operand is a closure.
    IsLambda, // "#fun"
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    NOP,
    /// Marks the end of the procedure definition. When executed
    /// returns the top value to the caller of this procedure.
    END,
    /// Returns the top value to the caller of this procedure.
    RET,
    /// See [`Op`]
    ///
    /// Example: BINOP ("*")
    BINOP {
        op: Op,
    },
    /// Pushes the 𝑘th constant from the constant pool.
    CONST {
        index: i32,
    },
    /// Pushes the 𝑠th string from the string table.
    STRING {
        index: i32,
    },
    /// Marks the start of a procedure definition with
    /// 𝑎 arguments and 𝑛 locals.
    /// When executed, initializes locals to an empty
    /// value. Unlike CBEGIN, the defined procedure
    /// cannot use captured variables.
    ///
    /// Example: BEGIN ("main", 2, 0, [], [], [])
    BEGIN {
        args: i32,
        locals: i32,
    },
    /// Marks the start of a closure definition with 𝑎 arguments
    /// and 𝑛 locals. When executed, initializes locals to an empty value.
    ///
    /// Unlike BEGIN, the defined closure may use captured variables.
    CBEGIN {
        args: i32,
        locals: i32,
    },
    /// Pushes a new closure with 𝑛 captured variables onto the
    /// stack. The bytecode for the closure begins at 𝑙 (given as an offset
    /// from the start of the bytecode).
    ///
    /// The instruction has a variable-length encoding; the description of
    /// each captured variable is specified as a 5-byte immediate.
    CLOSURE {
        offset: i32,
        arity: i32,
        /// A description of each captured variable.
        captured: Vec<i32>,
    },
    /// Store a value somewhere, depending on ValueRel
    ///
    /// Example: ST (Global ("z"))
    STORE {
        rel: ValueRel,
        index: i32,
    },
    /// Load a value from somewhere, depending on ValueRel
    ///
    /// Example: LD (Global ("z"))
    LOAD {
        rel: ValueRel,
        index: i32,
    },
    /// Load a reference to a value from somewhere, depending on ValueRel
    ///
    /// Example: LDA (Global ("z"))
    LOADREF {
        rel: ValueRel,
        index: i32,
    },
    /// Calls a function with 𝑛 arguments. The bytecode for the
    /// function begins at 𝑙 (given as an offset from the start of the byte
    /// code). Pushes the returned value onto the stack.
    /// OR calls a builtin function.
    CALL {
        offset: Option<i32>,
        n: Option<i32>,
        /// Name of the builtin function
        /// "Lread", "Lwrite", "Llength", "Lstring"
        name: Option<Builtin>,
        builtin: bool,
    },
    /// Calls a closure with 𝑛 arguments. The first
    /// operand must be the closure, followed by the arguments. Pushes
    /// the returned value onto the stack.
    CALLC {
        arity: i32,
    },
    /// Raises an error, reporting an error at the given line and column.
    /// The operand is the value being matched.
    FAIL {
        line: i32,
        column: i32,
    },
    /// Marks the following bytecode as corresponding to line n
    /// in the source text. Only used for diagnostics.
    LINE {
        n: i32,
    },
    /// Removes the top value from the stack.
    DROP,
    /// Duplicates the top value of the stack.
    DUP,
    /// Swaps the two top values on the stack.
    SWAP,
    /// Jumps to the given offset
    JMP {
        offset: i32,
    },
    /// Set instruction pointer to offset if operand is zero/non-zero
    CJMP {
        offset: i32,
        kind: CompareJumpKind,
    },
    /// Look up an element of some array/string/sexp
    /// NOTE: takes an operand and index from top of stack
    ELEM,
    /// Indirect store to variable
    /// Pop the reference to the variable and the value to store
    STI,
    /// Indirect store to a variable or an agregate
    /// If we store to a variable -> equivalent to STI
    /// Otherwise -> pop agregate, pop index, pop operand (result) that we assign to
    STA,
    /// Construct a sexp from, where s_index is the index of
    /// string in string table, used as tag
    SEXP {
        s_index: i32,
        n_members: i32,
    },
    /// Tests whether the operand is an S-expression with a specific
    /// tag (the 𝑠th string in the string table) and number of elements (𝑛).
    /// If the operand is not an S-expression, pushes 0.
    TAG {
        /// Index of the string in the string table
        index: i32,
        /// Number of elements in the S-expression
        n: i32,
    },
    /// Pattern matching instruction
    PATT {
        kind: PattKind,
    },
    /// Tests whether the operand is an array of 𝑛 elements.
    ARRAY {
        /// Number of elements in the array
        n: i32,
    },
    /// Imaginary instruction to mark the end of the bytecode file
    HALT,
}

/// Usefull feature to convert subopcode of
/// binary operation encoding into a variant of Op
impl TryFrom<u8> for Op {
    type Error = ();

    fn try_from(subopcode: u8) -> Result<Self, Self::Error> {
        match subopcode {
            0x1 => Ok(Op::ADD),
            0x2 => Ok(Op::SUB),
            0x3 => Ok(Op::MUL),
            0x4 => Ok(Op::DIV),
            0x5 => Ok(Op::MOD),
            0x6 => Ok(Op::LT),
            0x7 => Ok(Op::LEQ),
            0x8 => Ok(Op::GT),
            0x9 => Ok(Op::GEQ),
            0xa => Ok(Op::EQ),
            0xb => Ok(Op::NEQ),
            0xc => Ok(Op::AND),
            0xd => Ok(Op::OR),
            _ => Err(()),
        }
    }
}

/// Usefull feature to convert subopcode of
/// load/store/... [`ValueRel`] into a variant of ValueRel
impl TryFrom<u8> for ValueRel {
    type Error = ();

    fn try_from(subopcode: u8) -> Result<Self, Self::Error> {
        match subopcode {
            0x0 => Ok(ValueRel::Global),
            0x1 => Ok(ValueRel::Local),
            0x2 => Ok(ValueRel::Arg),
            0x3 => Ok(ValueRel::Capture),
            _ => Err(()),
        }
    }
}

/// Usefull feature to convert subopcode of
/// pattern matching into a variant of [`PattKind`]
impl TryFrom<u8> for PattKind {
    type Error = ();

    fn try_from(subopcode: u8) -> Result<Self, Self::Error> {
        match subopcode {
            0x0 => Ok(PattKind::BothAreStr),
            0x1 => Ok(PattKind::IsStr),
            0x2 => Ok(PattKind::IsArray),
            0x3 => Ok(PattKind::IsSExp),
            0x4 => Ok(PattKind::IsRef),
            0x5 => Ok(PattKind::IsVal),
            0x6 => Ok(PattKind::IsLambda),
            _ => Err(()),
        }
    }
}

impl TryFrom<u8> for Builtin {
    type Error = ();

    fn try_from(subopcode: u8) -> Result<Self, Self::Error> {
        match subopcode {
            0x0 => Ok(Builtin::Lread),
            0x1 => Ok(Builtin::Lwrite),
            0x2 => Ok(Builtin::Llength),
            0x3 => Ok(Builtin::Lstring),
            0x4 => Ok(Builtin::Barray),
            _ => Err(()),
        }
    }
}
