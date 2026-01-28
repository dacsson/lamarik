use clap::Parser;
use lama_rs::disasm::Bytefile;

/// Lama VM bytecode interpreter
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Source bytecode file
    #[arg(short, long)]
    lama_file: String,

    /// Verbose output
    #[arg(long, default_value_t = false)]
    verbose: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let bytefile = Bytefile::parse(args.lama_file.as_str())?;

    if args.verbose {
        println!("{}", bytefile);
    }

    Ok(())
}
