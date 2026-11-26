//! The `rustkit` command.

mod add;
mod cargo;
mod dev;
mod new;
mod pages;
mod run;

use std::process::ExitCode;

const USAGE: &str = "\
usage: rustkit <command> [options]

Commands:
  new <path>        create a new Rust-kit application
  add <kind> <name> add a page, layout or component to an application
  dev               run the app, rebuilding and reloading pages on save
  check             compile every page and report errors, without building
  routes            list the app's routes, how each is served, and its JS and CSS
  build [dir]       prerender the app into a directory (default: dist)
  serve [addr]      run a release build of the app

Run `rustkit <command> --help` for a command's options.
";

/// How a command failed.
pub enum CliError {
    /// `--help`: print the command's usage.
    Help,
    /// Bad arguments: print the message and the usage.
    Usage(String),
    /// The command ran and failed.
    Failed(String),
}

impl From<String> for CliError {
    fn from(e: String) -> Self {
        CliError::Failed(e)
    }
}

/// The value after an option such as `--addr`.
pub fn value(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, CliError> {
    args.next().ok_or_else(|| CliError::Usage(format!("`{name}` needs a value")))
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let rest: Vec<String> = args.collect();
    let (result, usage) = match command.as_deref() {
        Some("new") => (new::main(rest), new::USAGE),
        Some("add") => (add::main(rest), add::USAGE),
        Some("dev") => (dev::main(rest), dev::USAGE),
        Some("check") => (pages::check(rest), pages::CHECK_USAGE),
        Some("routes") => (pages::routes(rest), pages::ROUTES_USAGE),
        Some("build") => (run::build(rest), run::BUILD_USAGE),
        Some("serve") => (run::serve(rest), run::SERVE_USAGE),
        Some("-V" | "--version" | "version") => {
            println!("rustkit {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        None => (Err(CliError::Usage(String::new())), USAGE),
        Some(other) => (Err(CliError::Usage(format!("unknown command `{other}`"))), USAGE),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Help) => {
            print!("{usage}");
            ExitCode::SUCCESS
        }
        Err(CliError::Usage(e)) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{usage}");
            ExitCode::from(2)
        }
        Err(CliError::Failed(e)) => {
            eprintln!("rustkit: {e}");
            ExitCode::FAILURE
        }
    }
}
