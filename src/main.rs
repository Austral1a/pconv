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

    let workers = cli.workers.unwrap_or_else(convert::cpu_count);
    match convert::run(
        &cli.upload_from,
        &cli.offload_to,
        &cli.output_ext,
        workers,
        cli.speed,
        cli.quality,
        cli.memory,
        cli.skip_existing,
    ) {
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
