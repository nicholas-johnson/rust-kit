//! `rustkit build` and `rustkit serve`: run the application's own `build`
//! and `serve` commands from a release build. Apps made by `rustkit new`
//! have both.

use std::process::Command;

use crate::cargo;
use crate::{CliError, value};

pub const BUILD_USAGE: &str = "\
usage: rustkit build [dir] [-p <package>] [--debug] [--offline]

Builds the app in release mode and runs its `build` command, which writes the
prerendered pages and every asset to <dir> (default: dist).

  -p, --package   the application (default: the one in this directory)
  --debug         use the dev profile instead of release
  --offline       pass --offline to cargo
";

pub const SERVE_USAGE: &str = "\
usage: rustkit serve [addr] [-p <package>] [--debug] [--offline]

Builds the app in release mode and runs its `serve` command on <addr>
(default: 127.0.0.1:3000). Use `rustkit dev` while developing.

  -p, --package   the application (default: the one in this directory)
  --debug         use the dev profile instead of release
  --offline       pass --offline to cargo
";

struct Args {
    package: Option<String>,
    positional: Option<String>,
    cargo_flags: Vec<String>,
}

fn parse(args: Vec<String>) -> Result<Args, CliError> {
    let mut out = Args { package: None, positional: None, cargo_flags: vec!["--release".into()] };
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => out.package = Some(value(&mut args, "--package")?),
            "--debug" => out.cargo_flags.retain(|f| f != "--release"),
            "--offline" => out.cargo_flags.push(arg),
            "-h" | "--help" => return Err(CliError::Help),
            other if other.starts_with('-') => return Err(CliError::Usage(format!("unknown option `{other}`"))),
            _ if out.positional.is_some() => return Err(CliError::Usage(format!("unexpected argument `{arg}`"))),
            _ => out.positional = Some(arg),
        }
    }
    Ok(out)
}

fn run_app(args: Args, command: &str, default: &str) -> Result<(), CliError> {
    let ws = cargo::app(args.package.as_deref())?;
    let status = Command::new("cargo")
        .args(["run", "-p", &ws.package])
        .args(&args.cargo_flags)
        .args(["--", command, args.positional.as_deref().unwrap_or(default)])
        .status()
        .map_err(|e| format!("running cargo: {e}"))?;
    if status.success() { Ok(()) } else { Err(CliError::Failed(format!("`{}` exited with {status}", ws.package))) }
}

pub fn build(args: Vec<String>) -> Result<(), CliError> {
    run_app(parse(args)?, "build", "dist")
}

pub fn serve(args: Vec<String>) -> Result<(), CliError> {
    run_app(parse(args)?, "serve", "127.0.0.1:3000")
}
