//! Implements static analysis of Lama VM bytecode, for frequency analysis of instructions.

use crate::{bytecode::Instruction, disasm::Bytefile, interpreter::InstructionTrace};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
};

#[derive(Debug, Clone)]
pub struct Function {
    label: i32,
    blocks: Vec<Block>,
    target_offsets: HashSet<usize>,
}

impl Function {
    pub fn new(label: i32) -> Self {
        Function {
            label,
            blocks: Vec::new(),
            target_offsets: HashSet::new(),
        }
    }

    pub fn add_instruction(&mut self, instruction: InstructionTrace) {
        self.blocks
            .last_mut()
            .unwrap()
            .instructions
            .push(instruction);
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    offset: usize,
    offset_end: usize,
    label: i32,
    predecessors: Vec<i32>,
    instructions: Vec<InstructionTrace>,
}

impl Block {
    pub fn new(offset: usize, label: i32) -> Self {
        Block {
            offset,
            offset_end: 0,
            label,
            predecessors: Vec::new(),
            instructions: Vec::new(),
        }
    }
}

pub struct Analyzer {
    functions: Vec<Function>,
}

impl Analyzer {
    pub fn new() -> Self {
        Analyzer {
            functions: Vec::new(),
        }
    }

    pub fn build_cfg(&mut self, instructions: Vec<InstructionTrace>) {
        let mut functions = Vec::new();

        let mut counter = 0;
        let mut func_counter = 0;

        let mut current_func = Function::new(func_counter);

        let mut previous_instruction_is_jmp = false;

        // Push entry block
        current_func.blocks.push(Block::new(0, counter));

        // First pass:
        // 1. Split bytecode into functions
        // 2. Assign target offsets to each function
        for trace in instructions {
            current_func.add_instruction(trace.clone());

            if previous_instruction_is_jmp {
                current_func.target_offsets.insert(trace.offset);
                previous_instruction_is_jmp = false;
            }

            match trace.instruction {
                Instruction::BEGIN { .. } | Instruction::CBEGIN { .. } => {
                    current_func = Function::new(func_counter);

                    // Push entry block
                    current_func
                        .blocks
                        .push(Block::new(trace.offset as usize, counter));

                    // Push `BEGIN` function
                    // TODO: fix
                    current_func.add_instruction(trace.clone());

                    func_counter += 1;
                    counter = 0;
                }
                Instruction::END | Instruction::RET => {
                    current_func.blocks.last_mut().unwrap().offset_end = trace.offset as usize;
                    functions.push(current_func.clone());
                }
                Instruction::JMP { offset } => {
                    current_func.target_offsets.insert(offset as usize);
                }
                Instruction::CJMP { offset, .. } => {
                    current_func.target_offsets.insert(offset as usize);

                    // the next instruction is also a possible target offset,
                    // if compare is false
                    previous_instruction_is_jmp = true;
                }
                _ => {}
            }
        }

        // Second pass:
        // Split each function into basic blocks
        // by iterating over target offsets from first step
        // let functions = self.functions.clone();
        for mut func in functions {
            // After first pass we have only one block in each function
            let instructions = func.blocks[0].instructions.clone();

            // Sort offsets
            // TODO: fix clone()
            let mut offsets = func.target_offsets.clone().into_iter().collect::<Vec<_>>();
            offsets.push(func.blocks[0].offset);
            offsets.push(func.blocks[0].offset_end);
            offsets.sort();

            // Create pairs, which are (begin_block_offset, end_block_offset) for each block
            let offset_pairs = offsets
                .iter()
                .copied()
                .zip(offsets.iter().skip(1).copied())
                .collect::<Vec<(usize, usize)>>();

            // If we have just (begin_func_offset, end_func_offset) =>
            // we have only one block in the function
            if offset_pairs.len() == 1 {
                self.functions.push(func);
                continue;
            }

            // Cut instructions from entry block, based on offsets
            let mut blocks = vec![];
            let mut label = 0;
            for (begin, end) in offset_pairs {
                let mut block = Block::new(begin, label);
                block.offset = begin;

                let first_instruction = instructions
                    .iter()
                    .position(|trace| trace.offset == begin)
                    .unwrap();
                let last_instruction = instructions
                    .iter()
                    .position(|trace| trace.offset == end)
                    .unwrap();

                block.instructions = instructions[first_instruction..last_instruction].to_vec();
                blocks.push(block);
                label += 1;
            }

            func.blocks = blocks;

            self.functions.push(func);
        }

        // map of {target_label: Vec<Predecessor_Label>}
        let mut label_to_predecessor = HashMap::new();
        // Third pass:
        // Assign predecessors for each block
        for func in &mut self.functions {
            for block in &func.blocks {
                for instruction in &block.instructions {
                    if let Instruction::JMP { offset } = instruction.instruction {
                        // if let Some(target_block) = func.blocks.iter().find(|b| b.label == offset) {
                        //     target_block.predecessors.push(block.label);
                        // }
                        func.blocks
                            .iter()
                            .find(|b| b.offset == offset as usize)
                            .map(|b| {
                                label_to_predecessor
                                    .entry(b.label)
                                    .or_insert(Vec::new())
                                    .push(block.label)
                            });
                    }

                    if let Instruction::CJMP { offset, .. } = instruction.instruction {
                        func.blocks
                            .iter()
                            .find(|b| b.offset == offset as usize)
                            .map(|b| {
                                label_to_predecessor
                                    .entry(b.label)
                                    .or_insert(Vec::new())
                                    .push(block.label)
                            });
                    }
                }
            }
        }

        for targets in label_to_predecessor {
            for func in &mut self.functions {
                func.blocks
                    .iter_mut()
                    .find(|b| b.label == targets.0)
                    .map(|b| b.predecessors = targets.1.clone());
            }
        }
    }

    pub fn get_functions(&self) -> &Vec<Function> {
        &self.functions
    }

    pub fn cfg_to_dot(&self) -> String {
        // 1 – start the digraph and set a few nice defaults.
        let mut dot = String::new();
        writeln!(&mut dot, "digraph CFG {{").unwrap();
        writeln!(&mut dot, "    rankdir=TB;").unwrap(); // top‑to‑bottom layout
        writeln!(&mut dot, "    node [shape=record, fontname=\"Courier\"];").unwrap();

        let funcs = &self.functions;

        for func in funcs {
            let blocks = &func.blocks;
            // 2 – create a map from label -> block index for fast look‑ups.
            let mut label_to_idx = std::collections::HashMap::<i32, usize>::new();
            for (i, b) in blocks.iter().enumerate() {
                label_to_idx.insert(b.label, i);
            }

            // 3 – emit a node for every block.
            for block in blocks {
                // escape double quotes and backslashes for dot
                let mut instrs = String::new();
                for ins in &block.instructions {
                    let instr_as_string = format!("{:#?}", ins);
                    let replace_escaped = instr_as_string.replace("\r\n", "").replace("\n", "");
                    let esacpe_curly_braces =
                        replace_escaped.replace("{", "\\{").replace("}", "\\}");
                    // `\l` forces a left‑justified line break inside a record label
                    write!(&mut instrs, "{}\\l", esacpe_curly_braces).unwrap();
                }

                // record label:  { offset | instructions }
                let node_label = format!(
                    "{{ B{} | offset: {} | {} }}",
                    block.label,
                    block.offset,
                    if instrs.is_empty() {
                        "<empty>"
                    } else {
                        &instrs
                    }
                );

                writeln!(
                    &mut dot,
                    "    B{} [label=\"{}\"];", // node name = B<label>
                    block.label, node_label
                )
                .unwrap();
            }

            // 4 – emit edges.
            for (i, block) in blocks.iter().enumerate() {
                // 4a – edges coming from the explicit predecessor list.
                for &pred_label in &block.predecessors {
                    // huard against malformed predecessor data.
                    if label_to_idx.contains_key(&pred_label) {
                        writeln!(&mut dot, "    B{} -> B{};", pred_label, block.label).unwrap();
                    }
                }

                // 4b – fall‑through edge (the “next” block in linear order)
                //     Only add it when the block does *not* end with an unconditional jump
                let ends_with_uncond_jmp = block
                    .instructions
                    .last()
                    .map(|ins| matches!(ins.instruction, Instruction::JMP { .. }))
                    .unwrap_or(false);

                if !ends_with_uncond_jmp {
                    // The next block is simply the one that appears after us in the
                    // vector
                    if i + 1 < blocks.len() {
                        let succ_label = blocks[i + 1].label;
                        writeln!(
                            &mut dot,
                            "    B{} -> B{} [style=dashed];",
                            block.label, succ_label
                        )
                        .unwrap();
                    }
                } else {
                    // Find block with offset corresponding to the JMP instruction
                    if let Some(last) = block.instructions.last() {
                        if let Instruction::JMP { offset, .. } = last.instruction {
                            // Find the block whose `offset` matches the target address.
                            // In the current Analyzer implementation a block starts at the
                            // *target* of the jump, so this lookup works
                            if let Some(target_block) =
                                blocks.iter().find(|b| b.offset == offset as usize)
                            {
                                writeln!(
                                    &mut dot,
                                    "    B{} -> B{} [label=\"JMP\", color=red];",
                                    block.label, target_block.label
                                )
                                .unwrap();
                            }
                        }
                    }
                }

                // 4c – for conditional jumps we also want an explicit edge to the
                //     jump target (if the target exists). This is optional – you can
                //     omit it if you rely on the predecessor list only.
                if let Some(last) = block.instructions.last() {
                    if let Instruction::CJMP { offset, .. } = last.instruction {
                        // Find the block whose `offset` matches the target address.
                        // In the current Analyzer implementation a block starts at the
                        // *target* of the jump, so this lookup works
                        if let Some(target_block) =
                            blocks.iter().find(|b| b.offset == offset as usize)
                        {
                            writeln!(
                                &mut dot,
                                "    B{} -> B{} [label=\"CJMP\", color=blue];",
                                block.label, target_block.label
                            )
                            .unwrap();
                        }

                        // Second edge goes to BLOCK if condition is fale, i.e. to the next block
                        writeln!(
                            &mut dot,
                            "    B{} -> B{} [label=\"CJMP\", color=red];",
                            block.label,
                            block.label + 1
                        )
                        .unwrap();
                    }
                }
            }
        }

        writeln!(&mut dot, "}}").unwrap();
        dot
    }
}
