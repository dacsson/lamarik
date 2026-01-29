use std::fs::File;
use std::io::Read;
use clap::Parser;
use lama_rs::disasm::Bytefile;

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

    Ok(())
}
