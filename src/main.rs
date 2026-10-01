use clap::{Parser, Subcommand, ValueEnum};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};
use test_evidence::{compare, input, render};

#[derive(Parser)]
#[command(
    name = "test-evidence",
    version,
    about = "Compare local test-migration evidence, offline",
    long_about = "Compare one baseline snapshot with one candidate snapshot. Reads existing XML reports only; never executes tests, accesses the network, or collects telemetry. Timing is descriptive single-run evidence, not a repeated-run latency benchmark."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compare existing JUnit/Surefire reports and optional JaCoCo coverage
    #[command(
        after_help = "INPUTS: A file may have any name. A directory is scanned recursively for TEST-*.xml only. Report scope is its relative parent directory, not its filename. Symbolic links are omitted and make evidence inconclusive. Limits: 500 report files, 16 MiB per file, 128 MiB per snapshot, 64 directory levels, 100000 cases.\n\nEXIT STATUS: 0 accepted; 1 policy findings; 2 invalid or inconclusive evidence. Output goes to stdout; command errors go to stderr. No captured test stdout/stderr or exception bodies are emitted."
    )]
    Compare {
        /// Baseline XML file or report directory
        #[arg(long)]
        baseline: PathBuf,
        /// Candidate XML file or report directory
        #[arg(long)]
        candidate: PathBuf,
        /// Output format
        #[arg(long, value_enum, default_value_t = Format::Markdown)]
        format: Format,
        /// Baseline JaCoCo XML file; requires --candidate-jacoco
        #[arg(long, requires = "candidate_jacoco")]
        baseline_jacoco: Option<PathBuf>,
        /// Candidate JaCoCo XML file; requires --baseline-jacoco
        #[arg(long, requires = "baseline_jacoco")]
        candidate_jacoco: Option<PathBuf>,
        /// JSON array of explicit {baseline:{scope,id},candidate:{scope,id}} mappings
        #[arg(long)]
        mapping: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Markdown,
    Json,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<u8, String> {
    let Command::Compare {
        baseline,
        candidate,
        format,
        baseline_jacoco,
        candidate_jacoco,
        mapping,
    } = cli.command;
    let mappings = mapping
        .as_deref()
        .map(input::load_mapping)
        .transpose()?
        .unwrap_or_default();
    let baseline = input::load_snapshot(&baseline, baseline_jacoco.as_deref());
    let candidate = input::load_snapshot(&candidate, candidate_jacoco.as_deref());
    let comparison = compare::compare(&baseline, &candidate, &mappings);
    let output = match format {
        Format::Markdown => render::markdown(&comparison),
        Format::Json => render::json(&comparison).map_err(|_| "could not serialize comparison")?,
    };
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|e| format!("cannot write output: {}", e.kind()))?;
    Ok(comparison.verdict.exit_code() as u8)
}
