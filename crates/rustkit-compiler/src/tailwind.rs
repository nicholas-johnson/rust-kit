//! Optional Tailwind CSS support.
//!
//! When it is on, the build runs the Tailwind v4 CLI once over the crate and
//! links the resulting stylesheet from every page, before the page's own
//! `<style>` block. Nothing here runs unless a build asks for it.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Environment variable naming the Tailwind CLI to run.
pub const TAILWIND_ENV: &str = "RUSTKIT_TAILWIND";

/// The stylesheet used when the crate has no `src/tailwind.css`: all of
/// Tailwind, with class detection limited to the page files.
const DEFAULT_INPUT: &str = "@import \"tailwindcss\" source(none);\n@source \"./src/routes\";\n";

/// How to run Tailwind for one crate.
#[derive(Debug, Clone, Default)]
pub struct Tailwind {
    /// The CLI to run. When `None`, it is found by [`Tailwind::find_cli`].
    pub cli: Option<PathBuf>,
    /// The input stylesheet. When `None`, `src/tailwind.css` is used if it
    /// exists, and otherwise a default that imports all of Tailwind.
    pub input: Option<PathBuf>,
    /// Minify the output. On by default through [`crate::Build::tailwind`].
    pub minify: bool,
}

impl Tailwind {
    /// The CLI to run, in order: `$RUSTKIT_TAILWIND`, the crate's
    /// `node_modules/.bin/tailwindcss`, then `tailwindcss` on `PATH`.
    pub fn find_cli(crate_dir: &Path) -> Option<PathBuf> {
        if let Some(p) = std::env::var_os(TAILWIND_ENV).filter(|p| !p.is_empty()) {
            return Some(PathBuf::from(p));
        }
        let local = crate_dir.join("node_modules/.bin/tailwindcss");
        if local.is_file() {
            return Some(local);
        }
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join("tailwindcss")).find(|p| p.is_file())
    }

    /// The input stylesheet this build uses, if it is a file.
    pub fn input_file(&self, crate_dir: &Path) -> Option<PathBuf> {
        self.input.clone().map(|p| crate_dir.join(p)).or_else(|| {
            let p = crate_dir.join("src/tailwind.css");
            p.is_file().then_some(p)
        })
    }

    /// Runs the CLI with `crate_dir` as its working directory and returns
    /// the generated CSS.
    pub fn run(&self, crate_dir: &Path) -> Result<String, String> {
        let cli = self.cli.clone().or_else(|| Self::find_cli(crate_dir)).ok_or_else(|| {
            format!(
                "error: Tailwind is on, but no Tailwind CLI was found\n  = help: install the standalone CLI \
                 (https://tailwindcss.com/blog/standalone-cli) and put `tailwindcss` on PATH, \
                 run `npm install -D @tailwindcss/cli` in {}, or set {TAILWIND_ENV} to the CLI's path\n",
                crate_dir.display()
            )
        })?;
        let input = self.input_file(crate_dir);
        let mut cmd = Command::new(&cli);
        cmd.current_dir(crate_dir).stdout(Stdio::piped()).stderr(Stdio::piped());
        match &input {
            Some(p) => cmd.arg("--input").arg(p).stdin(Stdio::null()),
            None => cmd.args(["--input", "-"]).stdin(Stdio::piped()),
        };
        if self.minify {
            cmd.arg("--minify");
        }
        let mut child = cmd.spawn().map_err(|e| format!("error: running {}: {e}\n", cli.display()))?;
        if input.is_none() {
            // Dropping the handle afterwards closes stdin so the CLI starts.
            let mut stdin = child.stdin.take().expect("stdin is piped");
            stdin.write_all(DEFAULT_INPUT.as_bytes()).map_err(|e| format!("error: writing to Tailwind: {e}\n"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("error: running {}: {e}\n", cli.display()))?;
        if !out.status.success() {
            return Err(format!(
                "error: Tailwind failed ({})\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        String::from_utf8(out.stdout).map_err(|_| "error: Tailwind wrote CSS that is not UTF-8\n".to_owned())
    }
}
