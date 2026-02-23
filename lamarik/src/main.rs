use clap::Parser;
use lamacore::disasm::Bytefile;
use lamarik::interpreter::{Interpreter, InterpreterError};
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

    let bytefile = Bytefile::parse(content).map_err(|err| {
        eprintln!("{}", err);
        err
    })?;

    if args.dump_bytefile {
        println!("{}", bytefile);
    }

    let mut interp = Interpreter::new(bytefile);

    let _ = interp.run().map_err(|err| {
        eprintln!("{}", err);
        err
    });

    Ok(())
}
