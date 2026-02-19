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

## Testing

```
=> cargo test
running 24 tests
test disasm::tests::parse_minimal_file ... ok
test interpreter::tests::test_arg_and_local_load ... ok
test interpreter::tests::test_array ... ok
test interpreter::tests::test_array_tag ... ok
test interpreter::tests::test_binops_eval ... ok
test interpreter::tests::test_builtin_functions ... ok
test interpreter::tests::test_closure_creation ... ok
test interpreter::tests::test_conditional_jump ... ok
test interpreter::tests::test_decoder_minimal ... ok
test interpreter::tests::test_drop ... ok
test interpreter::tests::test_dup ... ok
test interpreter::tests::test_elem ... ok
test interpreter::tests::test_frame_move_args_and_locals ... ok
test interpreter::tests::test_invalid_args_and_locals_assignment ... ok
test interpreter::tests::test_invalid_elem ... ok
test interpreter::tests::test_invalid_frame_move ... ok
test interpreter::tests::test_invalid_jump_offset ... ok
test interpreter::tests::test_sexp_cons_nil_eval ... ok
test interpreter::tests::test_sexp_tag ... ok
test interpreter::tests::test_string_eval ... ok
test interpreter::tests::test_swap ... ok
test object::tests::test_create_from_string ... ok
test object::tests::test_creation ... ok
test tests::runtime_link_smoke_test ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

# Regression

You can see the regression tests in the `doc` directory. Currently this interpreter passes all 75 tests, and 4 tests failed as expected.

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
