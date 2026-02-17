use clap::Parser;
use lama_rs::disasm::Bytefile;
use lama_rs::interpreter::{Interpreter, InterpreterOpts};
use std::fs::File;
use std::io::Read;

/// Lama VM bytecode interpreter
#[derive(Parser, Debug)]
#[command(about, long_about = None)]
struct Args {
    /// Source bytecode file
    #[arg(short, long)]
    lama_file: String,

    /// Verbose output
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Parse only, do not execute
    #[arg(short, long, default_value_t = false)]
    parse_only: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut file: File = File::open(args.lama_file)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    let bytefile = Bytefile::parse(content)?;

    if args.verbose {
        println!("{}", bytefile);
    }

    let mut interp = Interpreter::new(
        bytefile,
        InterpreterOpts::new(args.parse_only, args.verbose),
    );

    let _ = interp.run().or_else(|err| {
        eprintln!("{}", err);
        Err(err)
    });

    Ok(())
}
