use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};

use lamacore::bytefile::Bytefile;

#[derive(Debug)]
pub enum AnalysisError {
    FileIsTooLarge(String, u64),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnalysisError::FileIsTooLarge(file, size) => {
                write!(f, "File {} is too large: {}, max is 1GB", file, size)
            }
        }
    }
}

impl std::error::Error for AnalysisError {}

// pub struct Analyzer {
//     decoder: Decoder,
// }

// impl Analyzer {
//     pub fn new(decoder: Decoder) -> Self {
//         Analyzer { decoder }
//     }

//     pub fn analyze(&self) -> Result<Frequency, AnalysisError> {
//         let mut frequency = Frequency::new();

//         for instruction in self.decoder.decode()? {
//             frequency.add_instruction(instruction.opcode_name());
//         }

//         Ok(frequency)
//     }
// }

// pub struct Frequency {
//     frequency: HashMap<String, u32>,
// }

// impl Frequency {
//     pub fn new() -> Self {
//         Frequency {
//             frequency: HashMap::new(),
//         }
//     }

//     pub fn add_instruction(&mut self, opcode_name: String) {
//         *self.frequency.entry(opcode_name).or_insert(0) += 1;
//     }
// }

// impl Display for Frequency {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         let mut to_vec = self.frequency.iter().collect::<Vec<_>>();
//         to_vec.sort_by(|a, b| b.1.cmp(&a.1));

//         for (opcode, count) in &to_vec {
//             writeln!(f, "{}: {}", opcode, count)?;
//         }
//         Ok(())
//     }
// }

// impl Debug for Frequency {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         for (opcode, count) in &self.frequency {
//             writeln!(f, "{}: {}", opcode, count)?;
//         }
//         Ok(())
//     }
// }
