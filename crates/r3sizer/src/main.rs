mod args;
mod corpus;
mod diff;
mod output;
mod presets;
mod run;
mod sweep;

use clap::Parser;

fn main() {
    let cli = args::Cli::parse();

    let result = match cli.command {
        args::Commands::Process(ref args) => run::run(args),
        args::Commands::Sweep(ref args) => sweep::run_sweep(args),
        args::Commands::Diff(ref args) => diff::run_diff(&args.baseline, &args.candidate),
        args::Commands::Corpus(ref args) => corpus::generate_corpus(&args.output_dir),
        args::Commands::Presets(ref cmd) => match cmd {
            args::PresetsCommand::List => presets::list_presets(),
            args::PresetsCommand::Show { name } => presets::show_preset(name),
        },
    };

    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
