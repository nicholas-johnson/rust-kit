//! `rustkit dev`: rebuilds the application when a file changes, restarts it,
//! and tells open pages to reload.
//!
//! This process owns the listening socket and hands it to each new copy of
//! the application, so browsers never see "connection refused" during a
//! restart; their connections wait in the socket's backlog. While no copy is
//! running (the first build failed, or the app exited), this process answers
//! requests itself with a page that explains why.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const USAGE: &str = "\
usage: rustkit dev [-p <package>] [--addr <host:port>] [--release] [--offline] [-- <app args>]

Builds the package, runs it, and on every change under the workspace rebuilds
it, restarts it and reloads open pages. Sessions survive restarts.

  -p, --package   the application to run (default: the one binary package
                  in the workspace that depends on rustkit)
  --addr          where to listen (default: 127.0.0.1:3000)
  --release       build with the release profile
  --offline       pass --offline to cargo
  -- <args>       arguments for the application (default: serve)
";

struct Options {
    package: Option<String>,
    addr: String,
    cargo_flags: Vec<String>,
    app_args: Vec<String>,
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("dev") => {}
        Some("-h" | "--help" | "help") | None => return Err(String::new()),
        Some(other) => return Err(format!("unknown command `{other}`")),
    }
    let mut opts = Options {
        package: None,
        addr: "127.0.0.1:3000".into(),
        cargo_flags: Vec::new(),
        app_args: Vec::new(),
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => opts.package = Some(args.next().ok_or("`--package` needs a value")?),
            "--addr" => opts.addr = args.next().ok_or("`--addr` needs a value")?,
            "--release" | "--offline" => opts.cargo_flags.push(arg),
            "--" => opts.app_args.extend(args.by_ref()),
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    if opts.app_args.is_empty() {
        opts.app_args.push("serve".into());
    }
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rustkit: {e}");
            ExitCode::FAILURE
        }
    }
}

/// What `cargo metadata` says about the workspace.
struct Workspace {
    root: PathBuf,
    target: PathBuf,
    package: String,
    manifest: String,
}

fn workspace(wanted: Option<&str>) -> Result<Workspace, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("running cargo: {e}"))?;
    if !out.status.success() {
        return Err("`cargo metadata` failed".into());
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let has_bin = |p: &serde_json::Value| {
        p["targets"].as_array().is_some_and(|ts| ts.iter().any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "bin"))))
    };
    let uses_rustkit =
        |p: &serde_json::Value| p["dependencies"].as_array().is_some_and(|ds| ds.iter().any(|d| d["name"] == "rustkit"));
    let candidates: Vec<&serde_json::Value> = match wanted {
        Some(name) => packages.iter().filter(|p| p["name"] == name).collect(),
        None => packages.iter().filter(|p| has_bin(p) && uses_rustkit(p)).collect(),
    };
    let pkg = match candidates.as_slice() {
        [one] => *one,
        [] => {
            return Err(match wanted {
                Some(name) => format!("no package named `{name}` in this workspace"),
                None => "no binary package in this workspace depends on rustkit; pass `-p <package>`".into(),
            });
        }
        many => {
            let names: Vec<&str> = many.iter().filter_map(|p| p["name"].as_str()).collect();
            return Err(format!("several applications to choose from ({}); pass `-p <package>`", names.join(", ")));
        }
    };
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
    Ok(Workspace {
        root: PathBuf::from(s(&meta["workspace_root"])),
        target: PathBuf::from(s(&meta["target_directory"])),
        package: s(&pkg["name"]),
        manifest: s(&pkg["manifest_path"]),
    })
}

struct Built {
    exe: PathBuf,
    /// Cargo didn't need to rebuild the executable.
    fresh: bool,
}

/// Runs `cargo build`, streaming its output to the terminal. On failure,
/// returns that output without colours, for the browser overlay.
fn build(ws: &Workspace, flags: &[String]) -> Result<Built, String> {
    let mut child = Command::new("cargo")
        .args(["build", "-p", &ws.package, "--message-format=json-render-diagnostics", "--color=always"])
        .args(flags)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("running cargo: {e}"))?;
    let mut stderr = child.stderr.take().unwrap();
    let tee = thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 8192];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let _ = std::io::stderr().write_all(&buf[..n]);
            all.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&all).into_owned()
    });
    let mut built = None;
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        if msg["reason"] == "compiler-artifact"
            && msg["manifest_path"] == ws.manifest.as_str()
            && let Some(exe) = msg["executable"].as_str()
        {
            built = Some(Built { exe: exe.into(), fresh: msg["fresh"] == true });
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let log = tee.join().unwrap_or_default();
    match built {
        Some(b) if status.success() => Ok(b),
        _ if status.success() => Err(format!("cargo built no executable for `{}`", ws.package)),
        _ => Err(failure_text(&log)),
    }
}

/// Cargo's error output, minus colours and progress lines. A failing build
/// script (where the page compiler reports) has its own stderr unindented.
fn failure_text(log: &str) -> String {
    let plain = strip_ansi(log);
    if let Some((_, script)) = plain.split_once("--- stderr\n") {
        let lines: Vec<&str> = script.lines().map(|l| l.strip_prefix("  ").unwrap_or(l)).collect();
        return lines.join("\n").trim().to_owned();
    }
    let lines: Vec<&str> = plain
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("Compiling ") || t.starts_with("Building ") || t.starts_with("Checking "))
        })
        .collect();
    lines.join("\n").trim().to_owned()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ params final-byte
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A fingerprint of every source file in the workspace.
fn scan(ws: &Workspace) -> u64 {
    fn walk(dir: &Path, skip: &Path, h: &mut DefaultHasher) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name();
            let name = name.to_string_lossy();
            let path = e.path();
            if name.starts_with('.') || matches!(&*name, "target" | "dist" | "node_modules") || path == skip {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, skip, h);
            } else {
                path.hash(h);
                meta.len().hash(h);
                meta.modified().ok().hash(h);
            }
        }
    }
    let mut h = DefaultHasher::new();
    walk(&ws.root, &ws.target, &mut h);
    h.finish()
}

/// The running application.
struct App {
    child: Child,
    stdin: ChildStdin,
}

impl App {
    fn tell(&mut self, msg: serde_json::Value) {
        let _ = writeln!(self.stdin, "{msg}");
        let _ = self.stdin.flush();
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Session<'a> {
    ws: &'a Workspace,
    opts: &'a Options,
    listener: TcpListener,
    build_no: u64,
    app: Option<App>,
    fallback: Option<Fallback>,
    fallback_no: u64,
}

impl Session<'_> {
    fn spawn(&mut self, exe: &Path) -> Result<(), String> {
        self.build_no += 1;
        if let Some(mut app) = self.app.take() {
            // Let open pages' event streams end cleanly first.
            app.tell(serde_json::json!({ "status": "restarting" }));
            thread::sleep(Duration::from_millis(50));
            app.stop();
        }
        if let Some(f) = self.fallback.take() {
            f.stop();
        }
        let fd = listener_fd(&self.listener);
        let sessions = self.ws.target.join("rustkit-dev").join(&self.ws.package).join("sessions.json");
        let mut cmd = Command::new(exe);
        cmd.args(&self.opts.app_args)
            .env("RUSTKIT_DEV", "1")
            .env("RUSTKIT_DEV_BUILD", self.build_no.to_string())
            .env("RUSTKIT_DEV_LISTEN_FD", fd.to_string())
            .env("RUSTKIT_DEV_SESSIONS", sessions)
            .stdin(Stdio::piped());
        inherit_fd(&mut cmd, fd);
        let mut child = cmd.spawn().map_err(|e| format!("starting {}: {e}", exe.display()))?;
        let stdin = child.stdin.take().unwrap();
        self.app = Some(App { child, stdin });
        Ok(())
    }

    fn show_error(&mut self, title: &str, text: &str) {
        if let Some(app) = &mut self.app {
            app.tell(serde_json::json!({ "status": "failed", "output": text }));
            return;
        }
        if let Some(f) = self.fallback.take() {
            f.stop();
        }
        self.fallback_no += 1;
        self.fallback = Some(Fallback::start(&self.listener, self.fallback_no, title, text));
    }

    fn rebuild(&mut self) {
        if let Some(app) = &mut self.app {
            app.tell(serde_json::json!({ "status": "building" }));
        }
        let started = Instant::now();
        match build(self.ws, &self.opts.cargo_flags) {
            Ok(b) if b.fresh && self.app.is_some() => {
                if let Some(app) = &mut self.app {
                    app.tell(serde_json::json!({ "status": "idle" }));
                }
            }
            Ok(b) => match self.spawn(&b.exe) {
                Ok(()) => eprintln!(
                    "\x1b[32mrustkit\x1b[0m build {} running ({:.1}s) on http://{}",
                    self.build_no,
                    started.elapsed().as_secs_f64(),
                    self.opts.addr
                ),
                Err(e) => {
                    eprintln!("rustkit: {e}");
                    self.show_error("Couldn't start the app", &e);
                }
            },
            Err(text) => {
                eprintln!("\x1b[31mrustkit\x1b[0m build failed; still serving the last good build");
                self.show_error("Build failed", &text);
            }
        }
    }

    /// Notices an application that exited on its own (a panic at startup, say).
    fn check_app(&mut self) {
        let exited = match &mut self.app {
            Some(app) => app.child.try_wait().ok().flatten(),
            None => None,
        };
        if let Some(status) = exited {
            self.app = None;
            eprintln!("\x1b[31mrustkit\x1b[0m the app exited ({status}); waiting for a change");
            self.show_error("The app exited", &format!("It exited with {status}. Its output is in the terminal running `rustkit dev`."));
        }
    }
}

fn run(opts: Options) -> Result<(), String> {
    let ws = workspace(opts.package.as_deref())?;
    let listener = TcpListener::bind(&opts.addr).map_err(|e| format!("binding {}: {e}", opts.addr))?;
    eprintln!("\x1b[32mrustkit\x1b[0m dev: `{}` on http://{}, watching {}", ws.package, opts.addr, ws.root.display());
    let mut s = Session { ws: &ws, opts: &opts, listener, build_no: 0, app: None, fallback: None, fallback_no: 0 };
    s.rebuild();
    let mut seen = scan(&ws);
    loop {
        thread::sleep(Duration::from_millis(200));
        s.check_app();
        let mut now = scan(&ws);
        if now == seen {
            continue;
        }
        // Wait for editors that write in several steps to finish.
        loop {
            thread::sleep(Duration::from_millis(80));
            let again = scan(&ws);
            if again == now {
                break;
            }
            now = again;
        }
        seen = now;
        s.rebuild();
    }
}

#[cfg(unix)]
fn listener_fd(l: &TcpListener) -> i32 {
    use std::os::fd::AsRawFd;
    l.as_raw_fd()
}

/// Lets the child inherit the listening socket. Rust opens sockets
/// close-on-exec; this clears the flag in the child only, after the fork.
#[cfg(unix)]
fn inherit_fd(cmd: &mut Command, fd: i32) {
    use std::os::unix::process::CommandExt;
    // SAFETY: fcntl is async-signal-safe, which is all pre_exec requires.
    unsafe {
        cmd.pre_exec(move || {
            if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn listener_fd(_: &TcpListener) -> i32 {
    compile_error!("rustkit dev needs a Unix platform to hand its socket to the app");
}

#[cfg(not(unix))]
fn inherit_fd(_: &mut Command, _: i32) {}

/// Answers requests while no application is running.
struct Fallback {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Fallback {
    fn start(listener: &TcpListener, generation: u64, title: &str, text: &str) -> Self {
        let listener = listener.try_clone().expect("cloning the listening socket");
        listener.set_nonblocking(true).expect("non-blocking socket");
        let page = fallback_page(generation, title, text);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => respond(stream, generation, &page),
                    Err(e) if e.kind() == ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(30)),
                    Err(_) => thread::sleep(Duration::from_millis(100)),
                }
            }
        });
        Fallback { stop, thread }
    }

    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.thread.join();
    }
}

fn respond(mut stream: TcpStream, generation: u64, page: &str) {
    // Accepted sockets inherit non-blocking mode on some platforms.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut req = Vec::new();
    let mut buf = [0u8; 2048];
    while !req.windows(4).any(|w| w == b"\r\n\r\n") && req.len() < 16 * 1024 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => req.extend_from_slice(&buf[..n]),
        }
    }
    let head = format!(
        "HTTP/1.1 503 Service Unavailable\r\ncontent-type: text/html; charset=utf-8\r\ncache-control: no-store\r\nx-rustkit-dev-fallback: {generation}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        page.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(page.as_bytes());
}

fn fallback_page(generation: u64, title: &str, text: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<style>
body{{margin:0;background:#1b1b1f;color:#eee;font:13px/1.5 ui-monospace,Menlo,monospace;padding:4vh 4vw}}
h1{{font:600 16px system-ui,sans-serif;color:#ff8080;margin:0 0 12px}}
pre{{white-space:pre-wrap;margin:0}}
p{{font:12px system-ui,sans-serif;color:#aaa}}
</style></head>
<body><h1>{title}</h1><pre>{text}</pre>
<p>Nothing is running yet. Fix the problem and save; this page reloads when the app is up.</p>
<script>
setInterval(async () => {{
  try {{
    const res = await fetch("/_rk/dev/ping", {{ cache: "no-store" }});
    if (res.headers.get("x-rustkit-dev-fallback") !== "{generation}") location.reload();
  }} catch {{}}
}}, 400);
</script></body></html>
"#,
        title = esc(title),
        text = esc(text),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_text_unwraps_build_script_stderr() {
        let log = "\x1b[1m\x1b[32m   Compiling\x1b[0m account-demo v0.1.0\n\x1b[1m\x1b[31merror\x1b[0m: failed to run custom build command for `account-demo`\n\nCaused by:\n  process didn't exit successfully\n  --- stderr\n  error: `formatDate(…)` cannot be rendered\n   --> src/routes/account.rk:3:15\n";
        assert_eq!(failure_text(log), "error: `formatDate(…)` cannot be rendered\n --> src/routes/account.rk:3:15");
    }

    #[test]
    fn failure_text_drops_progress_lines() {
        let log = "   Compiling rustkit v0.1.0\nerror[E0425]: cannot find value `x`\n --> src/lib.rs:1:1\n";
        assert_eq!(failure_text(log), "error[E0425]: cannot find value `x`\n --> src/lib.rs:1:1");
    }

    #[test]
    fn fallback_page_escapes_output() {
        let page = fallback_page(3, "Build failed", "<script>alert(1)</script>");
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(page.contains(r#"!== "3""#));
    }
}
