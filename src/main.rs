use clap::Parser as _;

use byte::cli::{Cli, run};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    match run::run(cli) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            byte::output::error(&e.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}
