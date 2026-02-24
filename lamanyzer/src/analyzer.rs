use std::collections::{HashMap, VecDeque};
use std::fmt::{Debug, Display, Formatter};

use bitvec::array::BitArray;
use bitvec::vec::BitVec;
use bitvec::{BitArr, prelude as bv};
use lamacore::bytecode::Instruction;
use lamacore::bytefile::Bytefile;
use lamacore::decoder::{Decoder, DecoderError};

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
            Instruction::CALL { offset, .. } => *offset,
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

        // Walking queue
        let mut worklist = VecDeque::new();
        worklist.reserve(self.decoder.bf.public_symbols.len());

        // Add public symbols to the worklist
        for (_, offset) in &self.decoder.bf.public_symbols {
            worklist.push_back(*offset);
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
                    worklist.push_back(offset as u32);
                }
            }

            // Enqueue jump targets
            if Analyzer::is_jump(&instr) {
                match instr {
                    Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                        worklist.push_back(offset as u32);
                        target_offsets.set(offset as usize, true);
                    }
                    _ => {}
                }
            }

            // Push next instruction
            if !Analyzer::is_terminal(&instr) {
                worklist.push_back(self.decoder.ip as u32);
            }
        }

        Ok(ReachableResult {
            reachables: reachable_offsets,
            targets: target_offsets,
        })
    }

    pub fn get_frequency(&mut self) -> Result<Frequency, AnalysisError> {
        let mut frequency = Frequency::new();

        let ReachableResult {
            reachables,
            targets,
        } = self.get_reachables()?;

        // Get reachable addresses from bit vector
        let mut addresses = reachables.iter_ones().collect::<Vec<_>>();
        addresses.sort();
        addresses.dedup();

        self.decoder.ip = addresses[0];

        for address in addresses {
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

                frequency.add_instruction_pair(instr.clone(), next_instr);

                self.decoder.ip = next_instr_start;
            }

            // Single opcode sequence always counts
            frequency.add_instruction(instr);
        }

        Ok(frequency)
    }
}

pub struct ReachableResult {
    reachables: BitVec,
    targets: BitVec,
}

pub struct Frequency {
    frequency: HashMap<u16, u32>,
    instruction_to_id: Vec<Instruction>,
}

impl Frequency {
    pub fn new() -> Self {
        Frequency {
            frequency: HashMap::new(),
            instruction_to_id: Vec::new(),
        }
    }

    pub fn add_instruction(&mut self, instruction: Instruction) {
        let id = if self.instruction_to_id.contains(&instruction) {
            self.instruction_to_id
                .iter()
                .position(|i| *i == instruction)
                .unwrap() as u8
        } else {
            self.instruction_to_id.push(instruction);
            self.instruction_to_id.len() as u8 - 1
        };

        // Put instruction at first 8 bits
        *self.frequency.entry((id as u16) << 8).or_insert(0) += 1;
    }

    pub fn add_instruction_pair(&mut self, instruction1: Instruction, instruction2: Instruction) {
        let id1 = if self.instruction_to_id.contains(&instruction1) {
            self.instruction_to_id
                .iter()
                .position(|i| *i == instruction1)
                .unwrap() as u8
        } else {
            self.instruction_to_id.push(instruction1);
            self.instruction_to_id.len() as u8 - 1
        };

        let id2 = if self.instruction_to_id.contains(&instruction2) {
            self.instruction_to_id
                .iter()
                .position(|i| *i == instruction2)
                .unwrap() as u8
        } else {
            self.instruction_to_id.push(instruction2);
            self.instruction_to_id.len() as u8 - 1
        };

        // Put instruction pair at first 8 bits
        *self
            .frequency
            .entry((id1 as u16) << 8 | id2 as u16)
            .or_insert(0) += 1;
    }
}

impl Display for Frequency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut to_vec = self.frequency.iter().collect::<Vec<_>>();
        to_vec.sort_by(|a, b| b.1.cmp(&a.1));

        for (ids, count) in to_vec {
            let id1 = (ids >> 8) as u8;
            let id2 = (ids & 0xFF) as u8;

            if id2 == 0 {
                writeln!(
                    f,
                    "{}: {}",
                    self.instruction_to_id[id1 as usize].get_opcode_name(),
                    count
                )?;
            } else {
                writeln!(
                    f,
                    "{}; {}: {}",
                    self.instruction_to_id[id1 as usize].get_opcode_name(),
                    self.instruction_to_id[id2 as usize].get_opcode_name(),
                    count
                )?;
            }
        }
        Ok(())
    }
}

impl Debug for Frequency {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut to_vec = self.frequency.iter().collect::<Vec<_>>();
        to_vec.sort_by(|a, b| b.1.cmp(&a.1));

        for (ids, count) in to_vec {
            let id1 = (ids >> 8) as u8;
            let id2 = (ids & 0xFF) as u8;

            if id2 == 0 {
                writeln!(
                    f,
                    "{}: {}",
                    self.instruction_to_id[id1 as usize].get_opcode_name(),
                    count
                )?;
            } else {
                writeln!(
                    f,
                    "{}; {}: {}",
                    self.instruction_to_id[id1 as usize].get_opcode_name(),
                    self.instruction_to_id[id2 as usize].get_opcode_name(),
                    count
                )?;
            }
        }
        Ok(())
    }
}
