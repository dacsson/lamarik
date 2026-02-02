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
cargo test
```

# Project structure
```
TBD
```
