use clap::Parser;
use lama_rs::analyzer::Analyzer;
use lama_rs::disasm::Bytefile;
use lama_rs::interpreter::Interpreter;
use std::fs::{self, File};
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

    /// Analyse opcodes frequency in the input file without execution
    #[arg(short, long, default_value_t = false)]
    frequency: bool,

    /// Dump control flow graph of the bytecode file
    #[arg(long, default_value_t = false)]
    dump_cfg: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut file: File = File::open(args.lama_file)?;
    let mut content = Vec::new();
    file.read_to_end(&mut content)?;

    let bytefile = Bytefile::parse(content)?;

    if args.dump_bytefile {
        println!("{}", bytefile);
    }

    let mut interp = Interpreter::new(bytefile);

    if args.frequency || args.dump_cfg {
        let instructions = interp.collect_instructions().map_err(|err| {
            eprintln!("{}", err);
            err
        })?;

        let mut analyzer = Analyzer::new();
        analyzer.build_cfg(instructions.to_vec());

        if args.dump_cfg {
            let cfg = analyzer.cfg_to_dot();
            println!("{}", cfg);
        } else {
            let frequency = analyzer.get_frequency();
            println!("{}", frequency);
        }
    } else {
        let _ = interp.run().map_err(|err| {
            eprintln!("{}", err);
            err
        });
    }

    Ok(())
}
