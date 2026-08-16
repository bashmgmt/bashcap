//! A transparent bash wrapper: write the full state of every shell at every
//! `BASHCAP` call site.
//!
//! Two ways in, and they differ only in who started the shells. `run` starts
//! a command line, exports the session's address into it and — unless told
//! otherwise — `BASH_ENV`, so its whole process tree joins; `serve` is started
//! *by* a bash script and hands that script the address to join. What is
//! captured, and where it goes, is the same either way — which is why both
//! take the same options, from the same type.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

use bash_interop::rig::{
    Attended, Doing, Driving, ExitStatus, Failure, Layout, Serving, JOINING,
};
use bashcap::{captures, BashCap};

#[derive(Parser)]
#[command(name = "bashcap", about = "Capture bash shell state at every BASHCAP call site")]
struct Cli {
    #[command(subcommand)]
    what: What,
}

#[derive(Subcommand)]
enum What {
    /// Run a command line under capture. Every shell finds the session's
    /// workspace in BC_SESSION; --reach says whether it has already joined.
    #[command(after_long_help = JOINING)]
    Run {
        #[command(flatten)]
        capture: Capture,

        /// How the shells find the instrument: bash-env has every
        /// non-interactive bash in the tree join as it starts; by-hand
        /// leaves it to the scripts, which join with
        /// `source "$BC_SESSION/session.bash"`.
        #[arg(long, value_enum, default_value_t = Reach::BashEnv)]
        reach: Reach,

        /// The wrapped command, program included — `bash build.bash`, or
        /// `make test`, whose own shells join too. Everything from the first
        /// plain word on is the subject's; a command that itself starts with
        /// a dash goes behind `--`.
        #[arg(trailing_var_arg = true, required = true)]
        argv: Vec<String>,
    },

    /// Capture for a bash script that started this process as a coprocess:
    /// it holds this process's standard input, and lets go to end the
    /// session. Nothing is written back — the client probes and joins by the
    /// same directory it names here (`BC_UP`, `BC_ATTACH`).
    #[command(after_long_help = JOINING)]
    Serve {
        #[command(flatten)]
        capture: Capture,

        /// The workspace the session is laid in — the client's to choose
        /// and to have made, so the client holds the address before this
        /// runs. Must exist; left behind.
        #[arg(long)]
        at: PathBuf,
    },

    /// Render a capture written by either of them.
    Show {
        /// The file to read: one JSON snapshot per line.
        from: PathBuf,
    },
}

/// How the subject's shells find the session — this tool's vocabulary,
/// mapped onto the run's environment closure. `BC_SESSION` carries the
/// workspace directory: the tools' own convention, spelled here, consulted
/// by nothing in the core.
#[derive(Copy, Clone, ValueEnum)]
enum Reach {
    /// BC_SESSION carries the workspace and BASH_ENV the session file:
    /// every non-interactive bash in the tree joins as it starts.
    BashEnv,

    /// BC_SESSION alone: a script joins where it says
    /// `source "$BC_SESSION/session.bash"`.
    ByHand,
}

impl Reach {
    fn environment(self, at: &Layout) -> Vec<(OsString, OsString)> {
        let session = (OsString::from("BC_SESSION"), OsString::from(at.text()));
        match self {
            Self::BashEnv => vec![session, at.bash_env()],
            Self::ByHand => vec![session],
        }
    }
}

/// What to capture and where to put it — the same question in both roles.
#[derive(Args)]
struct Capture {
    /// Where the capture goes: one JSON snapshot per line.
    #[arg(long)]
    into: PathBuf,

    /// Tally what was written, on stderr, keeping stdout the subject's
    /// own in both roles.
    #[arg(long)]
    verbose: bool,

    /// Record what each call was passed. This asks the subject's shells for
    /// `extdebug`, which also makes ERR, DEBUG and RETURN traps inherited by
    /// functions and subshells. Sourced into a shell that is already running —
    /// under `serve`, or `--reach by-hand` — it installs a DEBUG trap there,
    /// replacing one the client had.
    #[arg(long)]
    trace_calls: bool,
}

impl Capture {
    /// The tool this asks for. The file is opened here, so a path that cannot
    /// be written is known before any shell has run.
    fn tool(&self) -> Result<BashCap, Failure> {
        let bashcap = BashCap::writing(&self.into)?;

        Ok(match self.trace_calls {
            true => bashcap.tracing_calls(),
            false => bashcap,
        })
    }

    /// How many snapshots the run wrote, summed over the shells that wrote
    /// them.
    fn tally(&self, shells: &[Attended<usize>]) {
        if self.verbose {
            let written: usize = shells.iter().map(|shell| shell.kept).sum();

            eprintln!("bashcap: {written} snapshots -> {}", self.into.display());
        }
    }

    /// The exit code is the subject's, so a wrapped script is indistinguishable
    /// from an unwrapped one.
    async fn run(&self, reach: Reach, argv: &[String]) -> Result<ExitStatus, Failure> {
        let ran = self.tool()?.run(argv, |at| reach.environment(at)).await?;

        self.tally(&ran.shells);

        // The subject's own status either way: it was seen out to the end.
        if let Some(why) = ran.failed {
            eprintln!("bashcap: {why}");
        }

        Ok(ran.subject)
    }

    /// Nothing here starts a shell or ends one, so there is no subject's status
    /// to hand back — only whether the capture itself came out whole.
    async fn serve(&self, at: &Path) -> Result<(), Failure> {
        let served = self.tool()?.serve_coprocess(at).await?;

        self.tally(&served.shells);

        served.failed.map_or(Ok(()), Err)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let code = match Cli::try_parse() {
        Ok(cli) => perform(&cli.what).await.unwrap_or_else(|error| {
            eprintln!("bashcap: {error}");
            1
        }),
        // `--help` and `--version` are complaints too, and clap gives them
        // their own code — 0, where a real misuse is 2.
        Err(complaint) => {
            let _ = complaint.print();
            complaint.exit_code()
        }
    };

    std::process::exit(code);
}

/// The exit code the subcommand earned. Only `run` has one of its own — it is
/// the subject's — and everything that fails does so the same way.
async fn perform(what: &What) -> Result<i32, Failure> {
    match what {
        What::Run { capture, reach, argv } => {
            capture.run(*reach, argv).await.map(ExitStatus::shell_code)
        }
        What::Serve { capture, at } => capture.serve(at).await.map(|()| 0),
        What::Show { from } => show(from).map(|()| 0),
    }
}

/// Both the reading and the rendering are the library's, so what this prints
/// and what a library caller prints are the same text.
fn show(from: &Path) -> Result<(), Failure> {
    let reading = || format!("reading {}", from.display());
    let seen = captures(&std::fs::read_to_string(from).doing(reading)?).doing(reading)?;

    let shells: HashSet<usize> = seen.iter().map(|capture| capture.shell.nth).collect();
    println!("{} snapshots from {} shells\n", seen.len(), shells.len());

    for (at, capture) in seen.iter().enumerate() {
        println!("[{at}] {capture}");
    }
    Ok(())
}
