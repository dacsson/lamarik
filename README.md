# What's this?

This is a [Lama](https://github.com/PLTools/Lama) bytecode interpreter for Virtual Machines course.

> [!NOTE]
> There is also a [Zig version](https://github.com/dacsson/LamaRpreter), which is 70-80% ready.
It was abandoned due to rapid changes in Zig language, which made distributing the project that much harder.
Also, the runtime has some quirks, which led to a number of bugs when translating and linking with Zig. Some functions 
and logic were used in this Rust version almost verbatim.

> [!WARNING]
> If you are a student in this course (Virtualization and Virtual Machines) and you want to do these assignments in Rust - _don't_. It is just much much more painfull to do/get credits. You have been warned!

# Usage

## Building

You should build Lama runtime before the main build:

```
cd runtime-c 
make 
cd ..
cargo build --release // builds all tools
```

To turn on bytecode verification:
```
cargo build --release --features="runtime_checks" // for on-the-fly checking, which is more verbose
cargo build --release --features="static_checks" // for static check of bytecode file before evaluation
```

## Running

This project is split into separate tools:
- `lamacore` - a library for shared function on Lama bytecode files and it's descriptions, not an executable
- `lamanyzer` - analysis of bytefile for instruction frequencys
- `lamarik` - interpreter with runtime checks
- `lamarifyer` - static analysis of bytefile before interpretation

### Lamarik

You can run a `*.bc` file with the following commands:
```
./target/release/lamarik -l <path/to/file.bc> 
```

### Lamanyzer

You can run a `*.bc` file with the following commands:
```
./target/release/lamanyzer -l <path/to/file.bc> 
```

### Lamanyzer

You can run a `*.bc` file with the following commands:
```
./target/release/lamarifyer -l <path/to/file.bc> 
```

## Testing

You can run internal tests, but be sure to enable the `runtime_checks` feature and disable `no_std` in `lib.rs`:

```
=> cargo test --features "runtime_checks" -- --test-threads=1
running 23 tests
...
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
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
| lama-rs  | Runtime checks    | 3m7.228ss     | 3m3.093s     | 0m3.201s     |
| lama-rs  | Static checks     | 3m40.362s     | 3m35.054s     | 0m5.210s     |

