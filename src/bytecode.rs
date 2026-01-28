//! Descriptor of Lama bytecode

enum Op {
    ADD, // +
    SUB, // -
    MUL, // *
    DIV, // /
    MOD, // %
    LT, // <
    LEQ, // <=
    GT, // >
    GEQ, // >=
    EQ, // ==
    NEQ, // !=
    AND, // &&, Tests if both integer operands are non-zero
    OR, // !!, Tests if either of the operands is non-zero.
}

enum ValueRel {
    Global,
    Local,
    Arg, // Function argument
    Capture, // Captured by closure
}

enum CompareJumpKind {
    ISZERO, // jump if operand is zero
    ISNONZERO, // jump if operand is non-zero
}

enum Builtin {
    Lread,
    Lwrite,
    Llength,
    Lstring, // Load string from string table
    Barray,
}

enum Instruction {
    NOP,
    /// Marks the end of the procedure definition. When executed
    /// returns the top value to the caller of this procedure.
    END,
    /// See `Op` enum
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
    /// Marks the following bytecode as corresponding to line n
    /// in the source text. Only used for diagnostics.
    LINE {
        n: i32,
    },
    /// Removes the top value from the stack.
    DROP,
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
}