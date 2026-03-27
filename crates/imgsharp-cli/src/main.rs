mod args;
mod output;
mod run;
mod sweep;

use clap::Parser;

fn main() {
    let args = args::Cli::parse();

    let result = if args.sweep_dir.is_some() {
        sweep::run_sweep(&args)
    } else {
        run::run(&args)
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
