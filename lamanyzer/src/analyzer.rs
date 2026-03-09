use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};

use bitvec::array::BitArray;
use bitvec::vec::BitVec;
use bitvec::{BitArr, prelude as bv};
use lamacore::bytecode::Instruction;
use lamacore::bytecode::{Builtin, CompareJumpKind, Op, PattKind, ValueRel};
use lamacore::bytefile::Bytefile;
use lamacore::decoder::{Decoder, DecoderError};
use std::array;

// There is 60 opcodes in total in VM
const OPCODES_COUNT: usize = 60;

#[derive(Debug)]
pub enum AnalysisError {
    FileIsTooLarge(String, u64),
    DecoderError(DecoderError),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisError::FileIsTooLarge(file, size) => {
                write!(f, "File {} is too large: {}, max is 1GB", file, size)
            }
            AnalysisError::DecoderError(e) => {
                write!(f, "{}", e)
            }
        }
    }
}

impl std::error::Error for AnalysisError {}

pub struct Analyzer {
    decoder: Decoder,
}

impl Analyzer {
    pub fn new(decoder: Decoder) -> Self {
        Analyzer { decoder }
    }

    fn is_jump(instr: &Instruction) -> bool {
        match instr {
            Instruction::JMP { .. } | Instruction::CJMP { .. } => true,
            _ => false,
        }
    }

    fn is_terminal(instr: &Instruction) -> bool {
        match instr {
            Instruction::RET
            | Instruction::END
            | Instruction::FAIL { .. }
            | Instruction::JMP { .. } => true,
            _ => false,
        }
    }

    fn is_call(instr: &Instruction) -> bool {
        match instr {
            Instruction::CALL { .. } => true,
            _ => false,
        }
    }

    fn get_call_offset(instr: &Instruction) -> Option<i32> {
        match instr {
            Instruction::CALL { offset, .. } => Some(*offset),
            _ => None,
        }
    }

    fn is_split(instr: &Instruction) -> bool {
        match instr {
            Instruction::RET
            | Instruction::END
            | Instruction::FAIL { .. }
            | Instruction::JMP { .. }
            | Instruction::CALL { .. }
            | Instruction::CALLC { .. } => true,
            _ => false,
        }
    }

    /// Walk bytecode to find reachable offsets, starting from public symbols
    pub fn get_reachables(&mut self) -> Result<ReachableResult, AnalysisError> {
        // Initialize offsets in code section with all bits set to false
        let mut reachable_offsets = BitVec::new();
        reachable_offsets.resize(self.decoder.code_section_len, false);

        // Initialize jump targets
        let mut target_offsets = BitVec::new();
        target_offsets.resize(self.decoder.code_section_len, false);

        let mut visited_offsets = BitVec::new();
        visited_offsets.resize(self.decoder.code_section_len, false);

        // Walking queue
        let mut worklist = VecDeque::new();
        worklist.reserve(self.decoder.bf.public_symbols.len());

        let add_to_worklist = |offset: u32, list: &mut VecDeque<u32>, visited: &mut BitVec| {
            let offsetu = offset as usize;
            if !visited[offsetu] {
                visited.set(offsetu, true);
                list.push_back(offset);
            }
        };

        // Add public symbols to the worklist
        for (_, offset) in &self.decoder.bf.public_symbols {
            add_to_worklist(*offset, &mut worklist, &mut visited_offsets);
        }

        while !worklist.is_empty() {
            let offset = worklist.pop_front().unwrap();

            // Move to work element location (offset) in bytecode
            self.decoder.ip = offset as usize;

            let addr = offset as usize;

            // Skip if visited
            if reachable_offsets[addr] {
                continue;
            }

            // Mark visited
            reachable_offsets.set(addr, true);

            let encoding = self
                .decoder
                .next::<u8>()
                .map_err(|e| AnalysisError::DecoderError(e))?;

            let instr = self
                .decoder
                .decode(encoding)
                .map_err(|e| AnalysisError::DecoderError(e))?;

            // Enqueue functions that are called to process
            if Analyzer::is_call(&instr) {
                let Instruction::CALL { .. } = instr else {
                    unreachable!()
                };

                if let Some(offset) = Analyzer::get_call_offset(&instr) {
                    add_to_worklist(offset as u32, &mut worklist, &mut visited_offsets);
                }
            }

            // Enqueue jump targets
            if Analyzer::is_jump(&instr) {
                match instr {
                    Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                        add_to_worklist(offset as u32, &mut worklist, &mut visited_offsets);
                        target_offsets.set(offset as usize, true);
                    }
                    _ => {}
                }
            }

            // Push next instruction
            if !Analyzer::is_terminal(&instr) {
                add_to_worklist(self.decoder.ip as u32, &mut worklist, &mut visited_offsets);
            }
        }

        Ok(ReachableResult {
            reachables: reachable_offsets,
            targets: target_offsets,
        })
    }

    pub fn get_frequency(&mut self) -> Result<Frequency, AnalysisError> {
        let ReachableResult {
            reachables,
            targets,
        } = self.get_reachables()?;

        self.decoder.ip = reachables.first_one().unwrap() as usize;

        let mut occur_single = vec![0u32; self.decoder.code_section_len];
        let mut occur_double = vec![0u32; self.decoder.code_section_len];

        let mut singles = Vec::new();
        let mut doubles = Vec::new();

        for address in reachables.iter_ones() {
            self.decoder.ip = address;

            let encoding = self
                .decoder
                .next::<u8>()
                .map_err(|e| AnalysisError::DecoderError(e))?;

            let instr = self
                .decoder
                .decode(encoding)
                .map_err(|e| AnalysisError::DecoderError(e))?;

            let next_instr_start = self.decoder.ip;

            // Count sequences of 2 opcodes if not splittable
            if !targets[next_instr_start] && !Analyzer::is_split(&instr) {
                let next_encoding = self
                    .decoder
                    .next::<u8>()
                    .map_err(|e| AnalysisError::DecoderError(e))?;

                let next_instr = self
                    .decoder
                    .decode(next_encoding)
                    .map_err(|e| AnalysisError::DecoderError(e))?;

                // occur_double[address] += 1;
                doubles.push((instr.clone(), next_instr));

                self.decoder.ip = next_instr_start;
            }

            // Single opcode sequence always counts
            singles.push(instr);
        }

        let (freq_singles, freq_doubles) = self.count_dups(singles, doubles);

        Ok(Frequency::new(freq_singles, freq_doubles))
    }

    fn count_dups(
        &mut self,
        mut singles: Vec<Instruction>,
        mut doubles: Vec<(Instruction, Instruction)>,
    ) -> (
        Vec<(Instruction, usize)>,
        Vec<(Instruction, Instruction, usize)>,
    ) {
        singles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        doubles.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mut freq_singles = Vec::new();
        let mut freq_doubles = Vec::new();

        let mut singles_iter = singles.into_iter();

        if let Some(mut instr) = singles_iter.next() {
            let mut count = 1;
            for next in singles_iter {
                if next == instr {
                    count += 1;
                } else {
                    freq_singles.push((instr, count));
                    instr = next;
                    count = 1;
                }
            }
            freq_singles.push((instr, count));
        }

        let mut doubles_iter = doubles.into_iter();
        if let Some(mut pair) = doubles_iter.next() {
            let mut count = 1;
            for next in doubles_iter {
                if next == pair {
                    count += 1;
                } else {
                    freq_doubles.push((pair.0, pair.1, count));
                    pair = next;
                    count = 1;
                }
            }
            freq_doubles.push((pair.0, pair.1, count));
        }

        (freq_singles, freq_doubles)
    }
}

pub struct ReachableResult {
    reachables: BitVec,
    targets: BitVec,
}

pub struct Frequency {
    frequency_single: Vec<(Instruction, usize)>,
    frequency_double: Vec<(Instruction, Instruction, usize)>,
}

impl Frequency {
    pub fn new(
        frequency_single: Vec<(Instruction, usize)>,
        frequency_double: Vec<(Instruction, Instruction, usize)>,
    ) -> Self {
        Frequency {
            frequency_single,
            frequency_double,
        }
    }
}

impl Display for Frequency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut entries: Vec<(String, usize)> = Vec::new();

        // Single instructions
        for (instr, count) in &self.frequency_single {
            entries.push((instr.get_opcode_name(), *count));
        }

        // Instruction pairs
        for (instr1, instr2, count) in &self.frequency_double {
            entries.push((
                format!("{}; {}", instr1.get_opcode_name(), instr2.get_opcode_name()),
                *count,
            ));
        }

        // Sort descending by frequency
        entries.sort_unstable_by(|a, b| b.1.cmp(&a.1));

        for (name, count) in entries {
            writeln!(f, "{}: {}", name, count)?;
        }

        Ok(())
    }
}

// impl Debug for Frequency {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         let mut to_vec = self.frequency.iter().collect::<Vec<_>>();
//         to_vec.sort_by(|a, b| b.1.cmp(&a.1));

//         for (ids, count) in to_vec {
//             let id1 = (ids >> 8) as u8;
//             let id2 = (ids & 0xFF) as u8;

//             if id2 == 0 {
//                 writeln!(
//                     f,
//                     "{}: {}",
//                     self.instruction_to_id[id1 as usize].get_opcode_name(),
//                     count
//                 )?;
//             } else {
//                 writeln!(
//                     f,
//                     "{}; {}: {}",
//                     self.instruction_to_id[id1 as usize].get_opcode_name(),
//                     self.instruction_to_id[id2 as usize].get_opcode_name(),
//                     count
//                 )?;
//             }
//         }
//         Ok(())
//     }
// }
