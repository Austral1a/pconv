mod cli;
mod codecs;
mod convert;
mod format;

use std::process::ExitCode;

use clap::Parser;

use cli::Cli;

fn main() -> ExitCode {
    let args = cli::normalize_argv(std::env::args());
    let cli = Cli::parse_from(args);

    match convert::run(&cli.upload_from, &cli.offload_to, &cli.output_ext) {
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
        Ok(result) => {
            for dest in &result.successes {
                println!("{}", dest.display());
            }
            for (source, error) in &result.failures {
                eprintln!("{}: {error}", source.display());
            }
            if result.failures.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
    }
}
