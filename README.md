# What's this?

This is a [Lama](https://github.com/PLTools/Lama) bytecode interpreter for Virtual Machines course.

> [!NOTE]
> There is also a [Zig version](https://github.com/dacsson/LamaRpreter), which is 70-80% ready.
It was abandoned due to rapid changes in Zig language, which made distributing the project that much harder.
Also, the runtime has some quirks, which led to a number of bugs when translating and linking with Zig. Some functions 
and logic were used in this Rust version almost verbatim.

# Usage

## Building

You should build Lama runtime before the main build:

```
cd runtime-c 
make 
cd ..
cargo build --release
```

## Running

You can run a `*.bc` file with the following commands:
```
./target/release/lama-rs -l <path/to/file.bc> [-v]
```

## Options

```
Lama VM bytecode interpreter

Usage: lama-rs [OPTIONS] --lama-file <LAMA_FILE>

Options:
  -l, --lama-file <LAMA_FILE>  Source bytecode file
      --dump-bytefile          Dump parsed bytefile metadata
  -f, --frequency              Analyse opcodes frequency in the input file without execution
      --dump-cfg               Dump control flow graph of the bytecode file
  -h, --help                   Print help
```

## Testing

You can run internal tests, but be sure to enable the `runtime_checks` feature:

```
=> cargo test --features "runtime_checks" -- --test-threads=1
running 23 tests
...
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

# Regression Tests

You can see the regression tests in the `doc` directory. Currently this interpreter passes all 75 tests, and 4 tests failed as expected.

> [!WARNING]
> To run the regression tests, you need to set heap size to at least 128, instead of the default 64:
> ```
> // in `runtime-c/gc.h`
> #define MINIMUM_HEAP_CAPACITY (128)
> ```

Running regression tests:
```
./regression.py {LAMA_DIR}/regression/
```

# Performance

Performance has been tested on a `perfomance/Sort.lama` file with default heap size (see section above) using `time` linux command.

| Target   | Bytecode verifyer | Real time (s) | User time (s) | Sys time (s) |
|----------|-------------------|---------------|---------------|--------------|
| lamac -s | -                 | 2m32.840s     | 2m29.347s     | 0m3.345s     |
| lamac -i | -                 | 8m37.099s     | 8m30.830s     | 0m4.446s     |
| lama-rs  | None              | 2m58.577s     | 2m54.537s     | 0m4.031s     |
| lama-rs  | Runtime checks    | 3m22.140s     | 3m17.462s     | 0m3.669s     |
| lama-rs  | Static checks     | 3m19.732s     | 3m15.510s     | 0m4.076s     |

# Frequency Analysis

Analysis of frequency of instructions (1-2 parameterized opcodes) in the bytecode.

Example of frequency analysis of `Sort.bc`:
<details>
<summary>Output</summary>

```
=> lama-rs -l ../Lama/Sort.bc -f

DROP: 31
DUP: 28
ELEM: 21
CONST 1: 16
CONST 1; ELEM: 13
CONST 0: 11
DROP; DUP: 11
DUP; CONST 1: 11
DROP; DROP: 10
CONST 0; ELEM: 8
ELEM; DROP: 7
DUP; CONST 0: 7
LOAD function argument 0: 6
JMP 762: 5
DUP; DUP: 4
SEXP 0 2: 4
CALL 351 1: 3
ARRAY 2: 3
ELEM; STORE local variable 0: 3
LOAD local variable 0: 3
STORE local variable 0; DROP: 3
JMP 350: 3
STORE local variable 0: 3
DUP; ARRAY 2: 3
LOAD local variable 3: 3
CALL 351 1; DUP: 2
TAG 0 2: 2
DUP; TAG 0 2: 2
ELEM; CONST 0: 2
JMP 116: 2
ELEM; CONST 1: 2
BINOP EQ: 2
CALL 43 1: 2
SEXP 0 2; JMP 762: 2
LOAD local variable 1: 2
CJMP 191 ISZERO: 1
LINE 6; LOAD local variable 1: 1
LINE 5; LOAD local variable 3: 1
STORE local variable 5: 1
LINE 16: 1
CALL 43 1; SEXP 0 2: 1
STORE local variable 2; DROP: 1
LOAD local variable 1; BINOP GT: 1
BINOP EQ; CJMP 191 ISZERO: 1
STORE local variable 2: 1
ARRAY 2; CJMP 197 ISNONZERO: 1
CALL 117 1; END: 1
BINOP GT: 1
CALL 351 1; CONST 1: 1
LOAD local variable 0; SEXP 0 2: 1
LINE 16; LOAD local variable 0: 1
BINOP EQ; CJMP 274 ISZERO: 1
DROP; LOAD local variable 5: 1
DROP; LINE 5: 1
LOAD function argument 0; DUP: 1
ELEM; STORE local variable 4: 1
LINE 6: 1
DROP; JMP 715: 1
BINOP SUB: 1
LINE 27; CONST 10000: 1
CONST 10000; CALL 43 1: 1
BINOP SUB; CALL 43 1: 1
LOAD function argument 0; CJMP 106 ISZERO: 1
CJMP 637 ISNONZERO: 1
ELEM; STORE local variable 2: 1
ELEM; STORE local variable 1: 1
LINE 25; LINE 27: 1
LINE 7; LOAD local variable 2: 1
LINE 3: 1
LINE 3; LOAD function argument 0: 1
DROP; JMP 386: 1
LINE 24: 1
LOAD function argument 0; JMP 762: 1
CONST 1; BINOP EQ: 1
STORE local variable 4: 1
LOAD local variable 4: 1
LINE 9: 1
DROP; JMP 336: 1
JMP 262: 1
LOAD local variable 0; JMP 350: 1
STORE local variable 3; DROP: 1
DROP; JMP 734: 1
TAG 0 2; CJMP 428 ISNONZERO: 1
FAIL 7 17; JMP 762: 1
JMP 762; JMP 762: 1
DROP; CONST 0: 1
CONST 0; LINE 9: 1
CONST 1; LINE 6: 1
LINE 15; LOAD local variable 0: 1
BEGIN 2 0; LINE 25: 1
END: 1
JMP 715: 1
LOAD function argument 0; LOAD function argument 0: 1
FAIL 14 9; JMP 350: 1
BEGIN 1 0: 1
STORE local variable 3: 1
CJMP 106 ISZERO: 1
TAG 0 2; CJMP 392 ISNONZERO: 1
LOAD local variable 2; CALL 351 1: 1
CONST 0; JMP 116: 1
LINE 14; LOAD function argument 0: 1
DUP; DROP: 1
LOAD local variable 4; SEXP 0 2: 1
BEGIN 1 6; LINE 3: 1
LINE 14: 1
LOAD local variable 5; LOAD local variable 3: 1
ELEM; SEXP 0 2: 1
FAIL 14 9: 1
CJMP 392 ISNONZERO: 1
CALL 151 1; JMP 350: 1
CONST 0; BINOP EQ: 1
CJMP 428 ISNONZERO: 1
LINE 15: 1
LOAD local variable 5: 1
DROP; LINE 16: 1
LOAD local variable 2: 1
STORE local variable 4; DROP: 1
LINE 7: 1
JMP 336: 1
LOAD local variable 3; LOAD local variable 1: 1
CONST 10000: 1
LINE 25: 1
CONST 1; BINOP SUB: 1
JMP 734: 1
BEGIN 1 1; LINE 14: 1
LINE 5: 1
ARRAY 2; CJMP 280 ISNONZERO: 1
ELEM; STORE local variable 3: 1
BINOP GT; CJMP 600 ISZERO: 1
STORE local variable 5; DROP: 1
FAIL 7 17: 1
LOAD local variable 1; LOAD local variable 3: 1
STORE local variable 1: 1
LOAD function argument 0; CONST 1: 1
SEXP 0 2; JMP 116: 1
LOAD function argument 0; CALL 351 1: 1
LINE 9; LOAD function argument 0: 1
ELEM; STORE local variable 5: 1
CJMP 600 ISZERO: 1
LINE 24; LOAD function argument 0: 1
BEGIN 1 1: 1
CJMP 280 ISNONZERO: 1
STORE local variable 1; DROP: 1
DROP; JMP 262: 1
LINE 27: 1
ELEM; DUP: 1
LOAD local variable 0; CALL 151 1: 1
BEGIN 1 0; LINE 24: 1
CJMP 274 ISZERO: 1
JMP 386: 1
LOAD local variable 3; LOAD local variable 0: 1
BEGIN 2 0: 1
ARRAY 2; CJMP 637 ISNONZERO: 1
CALL 117 1: 1
CALL 43 1; CALL 117 1: 1
LOAD local variable 3; LOAD local variable 4: 1
CJMP 197 ISNONZERO: 1
SEXP 0 2; CALL 351 1: 1
CALL 151 1: 1
BEGIN 1 6: 1
DROP; LINE 15: 1
```
</details>

# Project structure
```
.
├── build.rs
├── Cargo.lock
├── Cargo.toml
├── doc
│   ├── failures.log     <- expected regressions failures
│   └── regression.txt   <- regression tests output
├── README.md            <- you are here
├── runtime-c            <- Lama runtime, from original repo
│   ├── ...
├── src
│   ├── analyzer.rs      <- CFG builder, frequency analysis
│   ├── bytecode.rs      <- Lama bytecode description
│   ├── disasm.rs        <- Lama disassembler
│   ├── frame.rs         <- Frame description
│   ├── interpreter      
│   │   └── tests.rs     <- interpreter internal tests
│   ├── interpreter.rs   <- main interpreter logic
│   ├── lib.rs           <- exposes runtime API
│   ├── main.rs
│   ├── numeric.rs       <- helper traits for numerics
│   └── object.rs        <- operand stack object description
```
