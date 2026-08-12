//! A transparent bash wrapper: run a script and write the full state of
//! every shell at every `BASHCAP` call site.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use mb_resolver::bash::rig::{run, Doing, ExitStatus, Failure};
use mb_resolver::bashcap::{captures, BashCap};

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

        /// Record what each call was passed. This asks the subject's shells
        /// for `extdebug`, which also makes ERR, DEBUG and RETURN traps
        /// inherited by functions and subshells.
        #[arg(long)]
        trace_calls: bool,

        /// The wrapped command, program included — `bash build.bash`, or
        /// `make test`, whose own shells join too. Everything from the first
        /// plain word on is the subject's; a command that itself starts with
        /// a dash goes behind `--`.
        #[arg(trailing_var_arg = true, required = true)]
        argv: Vec<String>,
    },

    /// Render a capture written by `run`.
    Show {
        /// The file to read: one JSON snapshot per line.
        from: PathBuf,
    },
}

fn main() {
    let code = match Cli::try_parse() {
        Ok(cli) => perform(cli.what).unwrap_or_else(|error| {
            eprintln!("bashcap: {error}");
            1
        }),
        Err(complaint) => {
            let _ = complaint.print();
            2
        }
    };

    std::process::exit(code);
}

/// The exit code the subcommand earned. Only `run` has one of its own — it is
/// the subject's — and everything that fails does so the same way.
fn perform(what: What) -> Result<i32, Failure> {
    match what {
        What::Run { into, verbose, trace_calls, argv } => {
            capture(&argv, &into, verbose, trace_calls).map(ExitStatus::shell_code)
        }
        What::Show { from } => show(&from).map(|()| 0),
    }
}

/// Both the reading and the rendering are the library's, so what this prints
/// and what a library caller prints are the same text.
fn show(from: &Path) -> Result<(), Failure> {
    let reading = || format!("reading {}", from.display());
    let seen = captures(&std::fs::read_to_string(from).doing(reading)?).doing(reading)?;

    let shells: HashSet<u32> = seen.iter().map(|capture| capture.pid).collect();
    println!("{} snapshots from {} shells\n", seen.len(), shells.len());

    for (at, capture) in seen.iter().enumerate() {
        println!("[{at}] {capture}");
    }
    Ok(())
}

/// The exit code is the subject's, so a wrapped script is indistinguishable
/// from an unwrapped one.
fn capture(
    argv: &[String],
    into: &Path,
    verbose: bool,
    trace_calls: bool,
) -> Result<ExitStatus, Failure> {
    let mut bashcap = BashCap::writing(into);
    if trace_calls {
        bashcap = bashcap.tracing_calls();
    }

    let ran = run(&bashcap, argv)?;

    if verbose {
        eprintln!("bashcap: {} snapshots -> {}", ran.session.written, into.display());
    }

    // The subject's own status either way: it was seen out even when the
    // capture broke, and a wrapper that reported its own trouble as the
    // subject's would not be transparent.
    if let Some(why) = ran.failed {
        eprintln!("bashcap: {why}");
    }

    Ok(ran.subject)
}
