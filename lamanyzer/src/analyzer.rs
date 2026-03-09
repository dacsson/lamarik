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
use std::cmp::Reverse;

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
            add_to_worklist(*offset, &mut worklist, &mut reachable_offsets);
        }

        while !worklist.is_empty() {
            let offset = worklist.pop_front().unwrap();

            // Move to work element location (offset) in bytecode
            self.decoder.ip = offset as usize;

            let addr = offset as usize;

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
                    add_to_worklist(offset as u32, &mut worklist, &mut reachable_offsets);
                }
            }

            // Enqueue jump targets
            if Analyzer::is_jump(&instr) {
                match instr {
                    Instruction::JMP { offset } | Instruction::CJMP { offset, .. } => {
                        add_to_worklist(offset as u32, &mut worklist, &mut reachable_offsets);
                        target_offsets.set(offset as usize, true);
                    }
                    _ => {}
                }
            }

            // Push next instruction
            if !Analyzer::is_terminal(&instr) {
                add_to_worklist(
                    self.decoder.ip as u32,
                    &mut worklist,
                    &mut reachable_offsets,
                );
            }

            // Mark visited
            reachable_offsets.set(addr, true);
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

        // Maximum possible frequency
        let max_singles = reachables.count_ones();
        // Maximum possible double frequency, we count it only when there is set bit followed by another set bit
        let max_doubles = reachables
            .iter()
            .zip(reachables.iter().skip(1))
            .filter(|(a, b)| **a != false && **b != false)
            .count();

        self.decoder.ip = reachables.first_one().unwrap() as usize;

        let mut singles: Vec<Occurence> = Vec::with_capacity(max_singles);
        let mut doubles: Vec<Occurence> = Vec::with_capacity(max_doubles);

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
                doubles.push(Occurence {
                    address: address as u32,
                    count: 0,
                });

                self.decoder.ip = next_instr_start;
            }

            // Single opcode sequence always counts
            singles.push(Occurence {
                address: address as u32,
                count: 0,
            });
        }

        // Sort by the underlying instruction at the given offset
        singles.sort_unstable_by(|a, b| {
            let addr_a = a.address;
            let addr_b = b.address;
            let instr_a = self.decode_at(addr_a);
            let instr_b = self.decode_at(addr_b);
            instr_a.partial_cmp(&instr_b).unwrap()
        });
        doubles.sort_unstable_by(|a, b| {
            let addr_a = a.address;
            let addr_b = b.address;
            let pair_a = self.decode_pair(addr_a);
            let pair_b = self.decode_pair(addr_b);

            match pair_a.0.partial_cmp(&pair_b.0).unwrap() {
                std::cmp::Ordering::Equal => pair_a.1.partial_cmp(&pair_b.1).unwrap(),
                ord => ord,
            }
        });

        // Because of sorting by instruction we actually will walk exactl amount of occurences before moving to next instruction here
        let mut singles_iter = singles.iter_mut();
        if let Some(mut occ) = singles_iter.next() {
            let mut count = 1;
            for next_occ in singles_iter {
                let instr = self.decode_at(occ.address);
                if instr == self.decode_at(next_occ.address) {
                    count += 1;
                } else {
                    occ.count = count;

                    occ = next_occ;
                    count = 1;
                }
            }
            occ.count = count;
        }

        let mut doubles_iter = doubles.iter_mut();
        if let Some(mut pair) = doubles_iter.next() {
            let mut count = 1;
            for pair_next in doubles_iter {
                let (next_instr1, next_instr2) = self.decode_pair(pair_next.address);
                let (instr1, instr2) = self.decode_pair(pair.address);
                if (next_instr1, next_instr2) == (instr1, instr2) {
                    count += 1;
                } else {
                    pair.count = count;

                    pair = pair_next;
                    count = 1;
                }
            }
            pair.count = count;
        }

        // Now sort by count
        singles.sort_by_key(|occ| Reverse(occ.count));
        doubles.sort_by_key(|occ| Reverse(occ.count));

        Ok(Frequency::new(singles, doubles, max_singles, max_doubles))
    }

    pub fn dump_frequency(&mut self, freq: Frequency) {
        let real_max_single = freq
            .frequency_single
            .iter()
            .max_by_key(|s| s.count)
            .unwrap();
        let real_max_double = freq
            .frequency_double
            .iter()
            .max_by_key(|s| s.count)
            .unwrap();
        let max = real_max_single.count.max(real_max_double.count);

        let mut singles_iter = freq.frequency_single.into_iter().peekable();
        let mut doubles_iter = freq.frequency_double.into_iter().peekable();

        while singles_iter.peek().is_some() || doubles_iter.peek().is_some() {
            // Both are sorted, so now we dump in order based on max frequency

            let only_singles_left = !doubles_iter.peek().is_some() && singles_iter.peek().is_some();
            let singles_not_empty = singles_iter.peek().is_some();
            let doubles_not_empty = doubles_iter.peek().is_some();
            let singles_closer_to_max = singles_not_empty
                && doubles_not_empty
                && (max - singles_iter.peek().unwrap().count
                    < max - doubles_iter.peek().unwrap().count);

            if singles_closer_to_max || only_singles_left {
                let Occurence { address, count } = singles_iter.next().unwrap();
                let instr_at_addr = self.decode_at(address);
                if count > 0 {
                    println!("{}: {}", instr_at_addr.get_opcode_name(), count);
                }
            } else if doubles_not_empty {
                let Occurence { address, count } = doubles_iter.next().unwrap();
                let (instr_at_addr, next_instr_at_addr) = self.decode_pair(address);
                if count > 0 {
                    println!(
                        "{}; {}: {}",
                        instr_at_addr.get_opcode_name(),
                        next_instr_at_addr.get_opcode_name(),
                        count
                    );
                }
            };
        }
    }

    fn decode_pair(&mut self, addr: u32) -> (Instruction, Instruction) {
        self.decoder.ip = addr as usize;

        let enc1 = self.decoder.next::<u8>().unwrap();
        let i1 = self.decoder.decode(enc1).unwrap();

        let enc2 = self.decoder.next::<u8>().unwrap();
        let i2 = self.decoder.decode(enc2).unwrap();

        (i1, i2)
    }

    fn decode_at(&mut self, addr: u32) -> Instruction {
        self.decoder.ip = addr as usize;

        let enc = self.decoder.next::<u8>().unwrap();
        self.decoder.decode(enc).unwrap()
    }
}

pub struct ReachableResult {
    reachables: BitVec,
    targets: BitVec,
}

pub struct Frequency {
    frequency_single: Vec<Occurence>,
    frequency_double: Vec<Occurence>,
    max_singles: usize,
    max_doubles: usize,
}

impl Frequency {
    pub fn new(
        frequency_single: Vec<Occurence>,
        frequency_double: Vec<Occurence>,
        max_singles: usize,
        max_doubles: usize,
    ) -> Self {
        Frequency {
            frequency_single,
            frequency_double,
            max_singles,
            max_doubles,
        }
    }
}

pub struct Occurence {
    address: u32,
    count: u32,
}
