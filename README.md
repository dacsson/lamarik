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
| lama-rs  | Runtime checks    | 3m18.140s     | 3m14.462s     | 0m3.669s     |

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
