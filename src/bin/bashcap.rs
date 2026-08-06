//! A transparent bash wrapper: run a script and write the full state of
//! every shell at every `BASHCAP` call site.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use mb_resolver::bash::rig::{run, ExitStatus, Failure};
use mb_resolver::bashcap::{BashCap, POLYFILL};

#[derive(Parser)]
#[command(name = "bashcap", about = "Capture bash shell state at every BASHCAP call site")]
struct Cli {
    #[command(subcommand)]
    what: What,
}

#[derive(Subcommand)]
enum What {
    /// Run a bash command under capture.
    Run {
        /// Where the capture goes: one JSON snapshot per line.
        #[arg(long)]
        into: PathBuf,

        /// Tally what was written, on stderr.
        #[arg(long)]
        verbose: bool,

        /// The wrapped command. Everything from the first plain word on is
        /// the subject's; a command that itself starts with a dash goes
        /// behind `--`.
        #[arg(trailing_var_arg = true, required = true)]
        argv: Vec<String>,
    },

    /// Print the client-side no-op stubs.
    Polyfill,
}

fn main() {
    let code = match Cli::try_parse() {
        Ok(Cli { what: What::Polyfill }) => {
            print!("{POLYFILL}");
            0
        }
        Ok(Cli { what: What::Run { into, verbose, argv } }) => {
            match capture(&argv, &into, verbose) {
                Ok(status) => status.shell_code(),
                Err(error) => {
                    eprintln!("bashcap: {error}");
                    1
                }
            }
        }
        Err(complaint) => {
            let _ = complaint.print();
            2
        }
    };

    std::process::exit(code);
}

/// The exit code is the subject's, so a wrapped script is indistinguishable
/// from an unwrapped one.
fn capture(argv: &[String], into: &Path, verbose: bool) -> Result<ExitStatus, Failure> {
    let (capturing, status) = run(&BashCap::writing(into), argv)?;

    if verbose {
        eprintln!("bashcap: {} snapshots -> {}", capturing.written, into.display());
    }

    Ok(status)
}
