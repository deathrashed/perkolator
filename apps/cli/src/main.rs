use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use perkolator_cli::{Cli, run};

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(output) => {
            let mut stdout = std::io::stdout().lock();
            if stdout.write_all(output.text.as_bytes()).is_err() || stdout.flush().is_err() {
                return ExitCode::FAILURE;
            }
            if output.success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("perk: {error}");
            ExitCode::FAILURE
        }
    }
}
