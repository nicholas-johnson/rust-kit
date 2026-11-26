//! `rustkit check` and `rustkit routes`: run the page compiler directly,
//! without Cargo, so they answer in milliseconds.

use rustkit_compiler::{Config, RouteSummary, compile_quiet};

use crate::cargo::{self, Workspace};
use crate::{CliError, value};

pub const CHECK_USAGE: &str = "\
usage: rustkit check [-p <package>]

Compiles every page, layout and component under src/routes and src/lib and
reports any errors. Rust code is not compiled; use `cargo check` for that.

  -p, --package   the application to check (default: every application in
                  the workspace)
";

pub const ROUTES_USAGE: &str = "\
usage: rustkit routes [-p <package>]

Lists every route, whether it is prerendered or rendered per request, and the
JavaScript and CSS it ships (before compression, without Tailwind).

  -p, --package   the application (default: the one in this directory)
";

fn package_arg(args: Vec<String>) -> Result<Option<String>, CliError> {
    let mut args = args.into_iter();
    let mut package = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => package = Some(value(&mut args, "--package")?),
            "-h" | "--help" => return Err(CliError::Help),
            other => return Err(CliError::Usage(format!("unknown option `{other}`"))),
        }
    }
    Ok(package)
}

fn compile_app(ws: &Workspace) -> Result<Vec<RouteSummary>, String> {
    let dir = ws.dir();
    let routes_dir = dir.join("src/routes");
    if !routes_dir.is_dir() {
        return Err(format!("`{}` has no src/routes directory", ws.package));
    }
    let cfg = Config {
        routes_dir,
        out_dir: ws.target.join("rustkit-check").join(&ws.package),
        state_type: "crate::AppState".into(),
        sources_module: "crate::sources".into(),
        dev: false,
        crate_dir: dir.to_path_buf(),
        tailwind: None,
    };
    compile_quiet(&cfg)
}

pub fn check(args: Vec<String>) -> Result<(), CliError> {
    let package = package_arg(args)?;
    let apps = cargo::apps(package.as_deref())?;
    let mut failed = Vec::new();
    for ws in &apps {
        match compile_app(ws) {
            Ok(routes) => eprintln!("\x1b[32m    ok\x1b[0m {} ({} routes)", ws.package, routes.len()),
            Err(e) => {
                eprintln!("{e}");
                eprintln!("\x1b[31mfailed\x1b[0m {}", ws.package);
                failed.push(ws.package.clone());
            }
        }
    }
    if failed.is_empty() { Ok(()) } else { Err(CliError::Failed(format!("pages failed to compile in {}", failed.join(", ")))) }
}

pub fn routes(args: Vec<String>) -> Result<(), CliError> {
    let package = package_arg(args)?;
    let ws = cargo::app(package.as_deref())?;
    let routes = compile_app(&ws)?;
    print!("{}", route_table(&routes));
    Ok(())
}

fn route_table(routes: &[RouteSummary]) -> String {
    let width = routes.iter().map(|r| r.path.len()).max().unwrap_or(0).max("route".len());
    let mut out = format!("{:<width$}  {:<8} {:>9} {:>9}\n", "route", "mode", "JS bytes", "CSS bytes");
    for r in routes {
        let mode = if r.prerender { "static" } else { "request" };
        out.push_str(&format!("{:<width$}  {:<8} {:>9} {:>9}\n", r.path, mode, r.js_bytes, r.css_bytes));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_aligns_columns() {
        let routes = vec![
            RouteSummary { path: "/".into(), prerender: true, js_bytes: 0, css_bytes: 120 },
            RouteSummary { path: "/blog/{slug}".into(), prerender: false, js_bytes: 3100, css_bytes: 0 },
        ];
        assert_eq!(
            route_table(&routes),
            "route         mode      JS bytes CSS bytes\n\
             /             static           0       120\n\
             /blog/{slug}  request       3100         0\n"
        );
    }
}
