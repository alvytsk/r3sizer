mod args;
mod corpus;
mod diff;
mod output;
mod presets;
mod run;
mod sweep;

use clap::Parser;

fn main() {
    let args = args::Cli::parse();

    let result = if let Some(ref dir) = args.generate_corpus {
        corpus::generate_corpus(dir)
    } else if let Some(ref paths) = args.sweep_diff {
        diff::run_diff(&paths[0], &paths[1])
    } else if args.sweep_dir.is_some() {
        sweep::run_sweep(&args)
    } else {
        run::run(&args)
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
