//! Console entry point for scripting: `nouwallctl probe | list | apply <id> | ...`.

#[cfg(windows)]
fn main() {
    std::process::exit(nouwall::cli::main(nouwall::cli::Frontend::Console));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("nouwallctl only runs on Windows");
    std::process::exit(1);
}
