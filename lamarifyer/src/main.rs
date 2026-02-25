use clap::Parser;
use lamacore::bytefile::Bytefile;
use lamacore::decoder::Decoder;
use lamarifyer::interpreter::{Interpreter, InterpreterError};
use lamarifyer::verifyer::Verifier;
use std::fs::File;
use std::io::Read;

/// Lama VM bytecode interpreter
#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    /// Source bytecode file
    #[arg(short, long)]
    lama_file: String,

    /// Dump parsed bytefile metadata
    #[arg(long, default_value_t = false)]
    dump_bytefile: bool,
}

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024; // 1GB

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Check file size
    let metadata = std::fs::metadata(&args.lama_file).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;
    if metadata.len() >= MAX_FILE_SIZE {
        return Err(InterpreterError::FileIsTooLarge(
            args.lama_file.to_string(),
            metadata.len(),
        ))
        .map_err(|err| {
            eprintln!("{}", err);
            err
        })?;
    }

    let mut file: File = File::open(args.lama_file).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;
    let mut content = Vec::new();
    file.read_to_end(&mut content).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;

    let bytefile = Bytefile::parse(content.clone()).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;

    if args.dump_bytefile {
        println!("{}", bytefile);
    }

    let decoder = Decoder::new(bytefile);

    let mut verifier = Verifier::new(decoder);
    let res = verifier.verify().map_err(|err| {
        eprintln!("{}", err);
        err
    })?;

    let mut new_bytefile = Bytefile::parse(content).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;
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

    let new_decoder = Decoder::new(new_bytefile);
    let mut interp = Interpreter::new(new_decoder);
    let _ = interp.run(res.1.reachables).map_err(|err| {
        eprintln!("{}", err);
        err
    });

    Ok(())
}
