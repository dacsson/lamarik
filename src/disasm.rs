//! Disassembler of Lama VM bytecode file

#[cfg(test)]
use std::ffi::CString;
use std::fmt::Display;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom};

#[derive(Debug)]
pub enum BytefileError {
    InvalidFileFormat,
    FileReadFailed,
    MemoryAllocationFailed,
    UnexpectedEOF,
    NoCodeSection,
    InvalidStringIndexInStringTable,
}

// Memory layout of the bytecode file
// +------------------------------------+
// |           File Header              |
// |------------------------------------|
// |  int32: S       | 4 bytes          |
// |  int32: glob_count | 4 bytes       |
// |  int32: P       | 4 bytes          |
// |  P × (int32, int32) | 8 bytes each |
// +------------------------------------+
// |           String Table             |
// |------------------------------------|
// |  S bytes        | Variable         |
// |  e.g., "string1\0string2\0"        |
// +------------------------------------+
// |           Code Region              |
// |------------------------------------|
// |  Variable bytes | Instructions     |
// |  e.g., 0x01 0x02 ... 0xFF          |
// +------------------------------------+
pub struct Bytefile {
    stringtab_size: u32,
    pub global_area_size: u32,
    public_symbols_number: u32,
    public_symbols: Vec<(u32, u32)>,
    string_table: Vec<u8>,
    pub code_section: Vec<u8>, // Kept raw for later interpretation
}

impl Display for BytefileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytefileError::InvalidFileFormat => write!(f, "Invalid file format"),
            BytefileError::FileReadFailed => write!(f, "File read failed"),
            BytefileError::MemoryAllocationFailed => write!(f, "Memory allocation failed"),
            BytefileError::UnexpectedEOF => write!(f, "Unexpected end of file"),
            BytefileError::NoCodeSection => write!(f, "No code section"),
            BytefileError::InvalidStringIndexInStringTable => {
                write!(f, "Invalid string index in string table")
            }
        }
    }
}

impl std::error::Error for BytefileError {}

impl Bytefile {
    /// Parse a bytecode file into a Bytefile struct.
    /// Leaves code section raw (as raw bytes) to be interpreted later,
    /// while all other sections are parsed and stored to be easily accessed.
    pub fn parse(source: Vec<u8>) -> Result<Bytefile, BytefileError> {
        let source_len = source.len();
        let mut reader = BufReader::new(Cursor::new(source));

        let mut buf = [0u8; 4];
        reader
            .read_exact(&mut buf)
            .map_err(|_| BytefileError::UnexpectedEOF)?;
        let stringtab_size = u32::from_le_bytes(buf);

        buf.fill(0);
        reader
            .read_exact(&mut buf)
            .map_err(|_| BytefileError::UnexpectedEOF)?;
        let global_area_size = u32::from_le_bytes(buf);

        buf.fill(0);
        reader
            .read_exact(&mut buf)
            .map_err(|_| BytefileError::UnexpectedEOF)?;
        let public_symbols_number = u32::from_le_bytes(buf);

        // Read public symbol table
        // P × (int32, int32) | 8 bytes each
        let mut public_symbols = Vec::with_capacity(public_symbols_number as usize);
        for _ in 0..public_symbols_number {
            buf.fill(0);
            reader
                .read_exact(&mut buf)
                .map_err(|_| BytefileError::UnexpectedEOF)?;
            let symbol = u32::from_le_bytes(buf);
            reader
                .read_exact(&mut buf)
                .map_err(|_| BytefileError::UnexpectedEOF)?;
            let name = u32::from_le_bytes(buf);
            public_symbols.push((symbol, name));
        }

        // Read string table
        let mut byte = [0u8; 1];
        let mut string_table = Vec::with_capacity(stringtab_size as usize);
        for _ in 0..stringtab_size {
            buf.fill(0);
            reader
                .read_exact(&mut byte)
                .map_err(|_| BytefileError::UnexpectedEOF)?;
            string_table.push(byte[0]);
        }

        // Read code section
        let bytes_till_end = source_len - reader.buffer().len();
        let mut code_section = Vec::with_capacity(bytes_till_end as usize);
        reader
            .read_to_end(&mut code_section)
            .map_err(|_| BytefileError::UnexpectedEOF)?;

        Ok(Bytefile {
            stringtab_size,
            global_area_size,
            public_symbols_number,
            public_symbols,
            string_table,
            code_section,
        })
    }

    /// Given a strings as array of bytes (including null terminators), find nth string
    pub fn get_string_at(&self, index: usize) -> Result<Vec<u8>, BytefileError> {
        let mut reader = BufReader::new(Cursor::new(&self.string_table));
        let mut strings = Vec::new();

        for _ in 0..self.stringtab_size {
            let mut buff = vec![];
            reader
                .read_until(0x00, &mut buff)
                .map_err(|_| BytefileError::InvalidStringIndexInStringTable)?;
            strings.push(buff);
        }

        #[cfg(feature = "runtime_checks")]
        if index >= strings.len() {
            return Err(BytefileError::InvalidStringIndexInStringTable);
        }

        Ok(strings[index].to_vec())
    }

    /// Given a strings as array of bytes (including null terminators), read string to null-terminator at offset `offset`
    pub fn get_string_at_offset(&self, offset: usize) -> Result<Vec<u8>, BytefileError> {
        #[cfg(feature = "runtime_checks")]
        if offset >= self.string_table.len() {
            return Err(BytefileError::InvalidStringIndexInStringTable);
        }

        let slice = &self.string_table[offset..];
        let first_null = slice
            .iter()
            .position(|&b| b == 0)
            .ok_or(BytefileError::InvalidStringIndexInStringTable)?;
        let buff = slice[..=first_null].to_vec();

        Ok(buff)
    }

    /// Create a dummy Bytefile for testing purposes
    #[cfg(test)]
    pub fn new_dummy() -> Self {
        Bytefile {
            stringtab_size: 0,
            global_area_size: 100,
            public_symbols_number: 100,
            code_section: vec![0; 100],
            string_table: vec![],
            public_symbols: vec![],
        }
    }

    /// Push an arbitrary string in string table
    #[cfg(test)]
    pub fn put_string(&mut self, str: CString) {
        let slice = str.as_bytes_with_nul();
        slice.iter().for_each(|b| self.string_table.push(*b));
        self.stringtab_size += 1;
    }
}

impl Display for Bytefile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "--------- Bytefile Dump ----------\n")?;
        write!(f, " - String Table Size: {}\n", self.stringtab_size)?;
        write!(f, " - Global Area Size: {}\n", self.global_area_size)?;
        write!(
            f,
            " - Public Symbol Table Size: {}\n",
            self.public_symbols_number
        )?;
        write!(
            f,
            " - Code Section Byte Size: {}\n",
            self.code_section.len()
        )?;

        write!(f, " - Public symbols: \n")?;
        for (s, n) in &self.public_symbols {
            write!(f, "  - {}: {}\n", s, n)?;
        }

        let str_table = String::from_utf8(self.string_table.clone()).unwrap();
        write!(f, " - String table raw: {:?}\n", self.string_table)?;
        write!(f, " - String Table: {}\n", str_table)?;

        write!(f, " - Code Section:\n")?;
        for s in &self.code_section {
            write!(f, "{:02X?}", s)?;
        }

        write!(f, "\n-----------------------------\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_minimal_file() -> Result<(), Box<dyn std::error::Error>> {
        // ~ =>  xxd dump/test1.bc
        // 00000000: 0500 0000 0100 0000 0100 0000 0000 0000  ................
        // 00000010: 0000 0000 6d61 696e 0052 0200 0000 0000  ....main.R......
        // 00000020: 0000 1002 0000 0010 0300 0000 015a 0100  .............Z..
        // 00000030: 0000 4000 0000 0018 5a02 0000 005a 0400  ..@.....Z....Z..
        // 00000040: 0000 2000 0000 0071 16ff                 .. ....q..
        let data: Vec<u8> = vec![
            0x05, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x52, 0x02, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x02, 0x00, 0x00, 0x00, 0x10, 0x03, 0x00,
            0x00, 0x00, 0x01, 0x5a, 0x01, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x18,
            0x5a, 0x02, 0x00, 0x00, 0x00, 0x5a, 0x04, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
            0x00, 0x71, 0x16, 0xff,
        ];

        let bytefile: Bytefile = Bytefile::parse(data)?;

        assert_eq!(bytefile.stringtab_size, 5);
        assert_eq!(bytefile.global_area_size, 1);
        assert_eq!(bytefile.public_symbols_number, 1);

        // Find "main" function name stored in string table
        let main_str = bytefile.get_string_at(0)?;
        assert_eq!(String::from_utf8(main_str)?, "main\0");

        Ok(())
    }
}
