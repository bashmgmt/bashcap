use mb_resolver::utilprog::bashcap::cli;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(cli::main(&args));
}
