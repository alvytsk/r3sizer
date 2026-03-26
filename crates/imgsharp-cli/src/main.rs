mod args;
mod output;
mod run;

use clap::Parser;

fn main() {
    let args = args::Cli::parse();
    if let Err(e) = run::run(&args) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
