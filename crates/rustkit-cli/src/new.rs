//! `rustkit new`: writes a small working application.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::{CliError, value};

pub const USAGE: &str = "\
usage: rustkit new <path> [--name <name>] [--tailwind] [--rustkit-path <dir> | --git <url>]

Creates a Rust-kit application at <path>: a prerendered home page, a counter
with browser state, a page that loads server data, a layout, a component and
tests. Inside a Cargo workspace it is added to the workspace's members.

  --name          the package name (default: the last part of <path>)
  --tailwind      turn on Tailwind CSS (needs the Tailwind CLI; see the README)
  --rustkit-path  depend on a local checkout of the rust-kit repository
  --git           depend on rust-kit from this git URL (default: the
                  official repository, or the enclosing rust-kit checkout)
";

const DEFAULT_GIT: &str = "https://github.com/nicholas-johnson/rust-kit";

const FILES: &[(&str, &str)] = &[
    ("Cargo.toml", include_str!("../templates/app/Cargo.toml")),
    ("build.rs", include_str!("../templates/app/build.rs")),
    (".gitignore", include_str!("../templates/app/gitignore")),
    ("README.md", include_str!("../templates/app/README.md")),
    ("src/lib.rs", include_str!("../templates/app/src/lib.rs")),
    ("src/main.rs", include_str!("../templates/app/src/main.rs")),
    ("src/sources.rs", include_str!("../templates/app/src/sources.rs")),
    ("src/lib/Card.rk", include_str!("../templates/app/src/lib/Card.rk")),
    ("src/routes/index.rk", include_str!("../templates/app/src/routes/index.rk")),
    ("src/routes/counter.rk", include_str!("../templates/app/src/routes/counter.rk")),
    ("src/routes/hello.rk", include_str!("../templates/app/src/routes/hello.rk")),
    ("tests/pages.rs", include_str!("../templates/app/tests/pages.rs")),
];
const LAYOUT_PLAIN: &str = include_str!("../templates/app/src/routes/layout-plain.rk");
const LAYOUT_TAILWIND: &str = include_str!("../templates/app/src/routes/layout-tailwind.rk");
const PACKAGE_JSON: &str = "{\n  \"private\": true,\n  \"devDependencies\": {\n    \"@tailwindcss/cli\": \"^4\",\n    \"tailwindcss\": \"^4\"\n  }\n}\n";

enum Dep {
    Path(PathBuf),
    Git(String),
}

struct Options {
    path: PathBuf,
    name: Option<String>,
    tailwind: bool,
    dep: Option<Dep>,
}

fn parse(args: Vec<String>) -> Result<Options, CliError> {
    let mut args = args.into_iter();
    let mut path = None;
    let mut opts = Options { path: PathBuf::new(), name: None, tailwind: false, dep: None };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--name" => opts.name = Some(value(&mut args, "--name")?),
            "--tailwind" => opts.tailwind = true,
            "--rustkit-path" => opts.dep = Some(Dep::Path(value(&mut args, "--rustkit-path")?.into())),
            "--git" => opts.dep = Some(Dep::Git(value(&mut args, "--git")?)),
            "-h" | "--help" => return Err(CliError::Help),
            other if other.starts_with('-') => return Err(CliError::Usage(format!("unknown option `{other}`"))),
            _ if path.is_some() => return Err(CliError::Usage(format!("unexpected argument `{arg}`"))),
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    opts.path = path.ok_or_else(|| CliError::Usage("missing <path>".into()))?;
    Ok(opts)
}

pub fn main(args: Vec<String>) -> Result<(), CliError> {
    let opts = parse(args)?;
    let name = match &opts.name {
        Some(n) => n.clone(),
        None => opts.path.file_name().and_then(|n| n.to_str()).ok_or_else(|| "can't take a package name from that path; pass `--name`".to_owned())?.to_owned(),
    };
    check_name(&name)?;
    if opts.path.exists() && fs::read_dir(&opts.path).map_err(|e| e.to_string())?.next().is_some() {
        return Err(CliError::Failed(format!("{} already exists and is not empty", opts.path.display())));
    }
    fs::create_dir_all(&opts.path).map_err(|e| format!("creating {}: {e}", opts.path.display()))?;
    let dir = opts.path.canonicalize().map_err(|e| e.to_string())?;

    let workspace = enclosing_workspace(&dir);
    let dep = match opts.dep {
        Some(Dep::Path(p)) => {
            let p = p.canonicalize().map_err(|e| format!("--rustkit-path {}: {e}", p.display()))?;
            if !p.join("crates/rustkit/Cargo.toml").is_file() {
                return Err(CliError::Failed(format!("{} is not a rust-kit checkout (no crates/rustkit)", p.display())));
            }
            Dep::Path(p)
        }
        Some(git) => git,
        None => match &workspace {
            Some(root) if root.join("crates/rustkit/Cargo.toml").is_file() => Dep::Path(root.clone()),
            _ => Dep::Git(DEFAULT_GIT.into()),
        },
    };
    let (rustkit_dep, compiler_dep) = match &dep {
        Dep::Path(p) => {
            let rel = relative(&dir, p);
            let rel = rel.to_string_lossy().replace('\\', "/");
            (format!("{{ path = \"{rel}/crates/rustkit\" }}"), format!("{{ path = \"{rel}/crates/rustkit-compiler\" }}"))
        }
        Dep::Git(url) => (format!("{{ git = \"{url}\" }}"), format!("{{ git = \"{url}\" }}")),
    };
    let files = app_files(&name, &rustkit_dep, &compiler_dep, opts.tailwind);
    for (rel, body) in &files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        fs::write(&path, body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }

    let mut notes = Vec::new();
    if let Some(root) = &workspace {
        let member = relative(root, &dir).to_string_lossy().replace('\\', "/");
        match add_member(&root.join("Cargo.toml"), &member) {
            Ok(true) => notes.push(format!("added `{member}` to the workspace members in {}", root.join("Cargo.toml").display())),
            Ok(false) => {}
            Err(e) => notes.push(format!("add \"{member}\" to `members` in {}: {e}", root.join("Cargo.toml").display())),
        }
    }

    eprintln!("\x1b[32m     Created\x1b[0m Rust-kit app `{name}` in {}", opts.path.display());
    for n in notes {
        eprintln!("        note: {n}");
    }
    eprintln!("\nNext:\n");
    eprintln!("    cd {}", opts.path.display());
    if opts.tailwind {
        eprintln!("    npm install        # the Tailwind CLI the build uses");
    }
    if workspace.is_some() {
        eprintln!("    rustkit dev -p {name}");
    } else {
        eprintln!("    rustkit dev");
    }
    eprintln!("\nThen open http://127.0.0.1:3000.");
    Ok(())
}

/// Every file of a new app, with its placeholders filled.
fn app_files(name: &str, rustkit_dep: &str, compiler_dep: &str, tailwind: bool) -> Vec<(&'static str, String)> {
    let build_call = if tailwind { "rustkit_compiler::Build::new().tailwind().run();" } else { "rustkit_compiler::build_script();" };
    let fill = |s: &str| {
        s.replace("{{name}}", name)
            .replace("{{crate}}", &name.replace('-', "_"))
            .replace("{{rustkit_dep}}", rustkit_dep)
            .replace("{{compiler_dep}}", compiler_dep)
            .replace("{{build_call}}", build_call)
    };
    let mut files: Vec<(&str, String)> = FILES.iter().map(|(p, s)| (*p, fill(s))).collect();
    files.push(("src/routes/layout.rk", fill(if tailwind { LAYOUT_TAILWIND } else { LAYOUT_PLAIN })));
    if tailwind {
        files.push(("package.json", PACKAGE_JSON.into()));
        if let Some((_, ignore)) = files.iter_mut().find(|(p, _)| *p == ".gitignore") {
            ignore.push_str("/node_modules\n");
        }
    }
    files
}

/// Cargo's rules for a package name, plus a crate name that isn't reserved.
fn check_name(name: &str) -> Result<(), String> {
    let first_ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_ok = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !first_ok || !rest_ok {
        return Err(format!("`{name}` isn't a valid package name: use letters, digits, `-` and `_`, starting with a letter; pass `--name`"));
    }
    let crate_name = name.replace('-', "_");
    if matches!(crate_name.as_str(), "rustkit" | "rustkit_compiler" | "test" | "core" | "std" | "alloc" | "proc_macro" | "self" | "super" | "crate")
    {
        return Err(format!("`{name}` would clash with an existing crate; pass `--name`"));
    }
    Ok(())
}

/// The root of the Cargo workspace above `dir`, if there is one.
fn enclosing_workspace(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().skip(1).find(|d| fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|s| s.lines().any(|l| l.trim() == "[workspace]"))).map(Path::to_path_buf)
}

/// Adds `member` to the `members = [...]` array of a workspace manifest.
/// Returns false when it is already listed or covered by a glob.
fn add_member(manifest: &Path, member: &str) -> Result<bool, String> {
    let text = fs::read_to_string(manifest).map_err(|e| e.to_string())?;
    let (updated, added) = insert_member(&text, member)?;
    if added {
        fs::write(manifest, updated).map_err(|e| e.to_string())?;
    }
    Ok(added)
}

fn insert_member(text: &str, member: &str) -> Result<(String, bool), String> {
    let start = text
        .match_indices("members")
        .map(|(i, _)| i)
        .find(|&i| text[..i].rsplit('\n').next().is_some_and(|before| before.trim().is_empty()))
        .ok_or("it has no `members` list")?;
    let open = start + text[start..].find('[').ok_or("can't read its `members` list")?;
    let close = open + text[open..].find(']').ok_or("can't read its `members` list")?;
    let list = &text[open + 1..close];
    let entries: Vec<&str> = list.split(',').map(|e| e.trim().trim_matches('"')).filter(|e| !e.is_empty()).collect();
    let covered = entries.iter().any(|e| {
        *e == member || e.strip_suffix("/*").is_some_and(|prefix| Path::new(member).parent().is_some_and(|p| p == Path::new(prefix)))
    });
    if covered {
        return Ok((text.to_owned(), false));
    }
    let multiline = list.contains('\n');
    let trimmed = list.trim_end();
    let comma = if trimmed.is_empty() || trimmed.ends_with(',') { "" } else { "," };
    let mut out = String::with_capacity(text.len() + member.len() + 8);
    out.push_str(&text[..open + 1]);
    out.push_str(trimmed);
    if multiline {
        out.push_str(&format!("{comma}\n    \"{member}\",\n"));
    } else if trimmed.is_empty() {
        out.push_str(&format!("\"{member}\""));
    } else {
        out.push_str(&format!("{comma} \"{member}\""));
    }
    out.push_str(&text[close..]);
    Ok((out, true))
}

/// `to` relative to `from`. Both are absolute and canonical.
fn relative(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    // Sharing only the root: an absolute path reads better than `../../..`.
    if common <= 1 {
        return to.iter().collect();
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c);
    }
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_compiles() {
        for tailwind in [false, true] {
            let dir = std::env::temp_dir().join(format!("rustkit-new-test-{}-{tailwind}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            for (rel, body) in app_files("my-app", "{ path = \"x\" }", "{ path = \"y\" }", tailwind) {
                assert!(!body.contains("{{"), "{rel} has an unfilled placeholder");
                let path = dir.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, body).unwrap();
            }
            let cfg = rustkit_compiler::Config {
                routes_dir: dir.join("src/routes"),
                out_dir: dir.join("out"),
                state_type: "crate::AppState".into(),
                sources_module: "crate::sources".into(),
                dev: false,
                crate_dir: dir.clone(),
                tailwind: None,
            };
            let routes = rustkit_compiler::compile_quiet(&cfg).unwrap_or_else(|e| panic!("{e}"));
            let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
            assert_eq!(paths, ["/counter", "/hello", "/"]);
            assert!(fs::read_to_string(dir.join("src/main.rs")).unwrap().contains("my_app::app("));
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn inserts_into_a_one_line_members_list() {
        let (out, added) = insert_member("[workspace]\nmembers = [\"a\", \"b\"]\n", "examples/c").unwrap();
        assert!(added);
        assert_eq!(out, "[workspace]\nmembers = [\"a\", \"b\", \"examples/c\"]\n");
    }

    #[test]
    fn inserts_into_a_multiline_members_list() {
        let (out, _) = insert_member("[workspace]\nmembers = [\n    \"a\",\n]\n", "c").unwrap();
        assert_eq!(out, "[workspace]\nmembers = [\n    \"a\",\n    \"c\",\n]\n");
    }

    #[test]
    fn skips_default_members() {
        let (out, _) = insert_member("default-members = [\"a\"]\nmembers = [\"a\"]\n", "b").unwrap();
        assert_eq!(out, "default-members = [\"a\"]\nmembers = [\"a\", \"b\"]\n");
    }

    #[test]
    fn leaves_members_covered_by_a_glob() {
        let (_, added) = insert_member("members = [\"examples/*\"]", "examples/c").unwrap();
        assert!(!added);
        let (_, added) = insert_member("members = [\"examples/c\"]", "examples/c").unwrap();
        assert!(!added);
    }

    #[test]
    fn relative_paths_walk_up_then_down() {
        assert_eq!(relative(Path::new("/a/b/c"), Path::new("/a")), PathBuf::from("../.."));
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/x/y")), PathBuf::from("../x/y"));
        assert_eq!(relative(Path::new("/tmp/app"), Path::new("/home/rk")), PathBuf::from("/home/rk"));
    }

    #[test]
    fn rejects_bad_names() {
        assert!(check_name("my-app").is_ok());
        assert!(check_name("1app").is_err());
        assert!(check_name("my app").is_err());
        assert!(check_name("rustkit").is_err());
    }
}
