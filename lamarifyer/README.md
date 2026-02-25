# What's this?

Interpreter with static checks before evalutaion loop.

## Diff 

New file: `src/verifyer.rs` with static checks

Stack depth memorized for begin opcode:
```rust
for (offset, depth) in res.0.stack_depths.iter().enumerate() {
    if *depth != 0 {
        // println!("Stack depth: {}", depth);

        let begin_instr_bytes = &new_bytefile.code_section[offset - 8..offset - 4];
        let mut payload = u32::from_le_bytes(begin_instr_bytes.try_into().unwrap());
        payload |= (depth.to_le() as u32) << 16;
        new_bytefile.code_section[offset - 8..offset - 4]
            .copy_from_slice(&payload.to_le_bytes());
    }
}
```

Only reachable instructions are traversed.
