use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use mb_resolver::utilprog::bashcap::{spec, model, doc};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("usage: bashcap [--output=<path>] [bash-args...]");
        eprintln!("       bashcap polyfill");
        eprintln!("       bashcap --help-full");
        eprintln!();
        eprintln!("Transparent bash wrapper. All args after bashcap's own flags");
        eprintln!("are passed directly to bash. Injection via BASH_ENV.");
        process::exit(1);
    }

    // Subcommand: polyfill
    if args[0] == "polyfill" {
        print!("{}", spec::POLYFILL);
        return;
    }

    // --help-full: print full user guide
    if args[0] == "--help-full" {
        print!("{}", doc::HELP_FULL);
        return;
    }

    // Split: bashcap's own --flags from bash args (everything else)
    let mut output_path: Option<String> = None;
    let mut bash_args: Vec<String> = Vec::new();
    for arg in &args {
        if let Some(val) = arg.strip_prefix("--output=") {
            output_path = Some(val.to_string());
        } else {
            bash_args.push(arg.clone());
        }
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let output_file = output_path.unwrap_or_else(|| format!(".bashcap.{timestamp}.jsonl"));

    // Run via InstrumentationSpec — handles BASH_ENV injection and harvest
    let result = spec::bashcap_spec()
        .run_bash(&bash_args)
        .unwrap_or_else(|e| {
            eprintln!("bashcap: {e}");
            process::exit(1);
        });

    // Convert __SNAP__-tagged captured calls to BashCapEntry
    let snap_calls = result.calls.get(&["__SNAP__"]);
    let mut entries = Vec::new();
    for (i, call) in snap_calls.iter().enumerate() {
        match model::parse_commandlist(&call.commandlist) {
            Ok(entry) => entries.push(entry),
            Err(e) => eprintln!("bashcap: warning: snap {}: {e}", i + 1),
        }
    }

    // Write JSONL output
    let output = PathBuf::from(&output_file);
    let mut out = fs::File::create(&output).unwrap_or_else(|e| {
        eprintln!("bashcap: cannot create output {}: {e}", output.display());
        process::exit(1);
    });
    for entry in &entries {
        let json = serde_json::to_string(entry).unwrap();
        writeln!(out, "{json}").unwrap();
    }

    eprintln!(
        "bashcap: {} entries captured → {}",
        entries.len(),
        output.display()
    );

    // Preserve bash exit code
    process::exit(result.exit_code.unwrap_or(1));
}
