//! `rustkit add`: writes a starter page, layout or component.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cargo;
use crate::{CliError, value};

pub const USAGE: &str = "\
usage: rustkit add <page|layout|component> <name> [-p <package>]

  page <route>      src/routes/<route>.rk. `about` serves /about, `blog/[slug]`
                    serves /blog/{slug}, and `/` is the home page
  layout <dir>      src/routes/<dir>/layout.rk, wrapping every page in <dir>
                    (`/` for the whole app)
  component <Name>  src/lib/<Name>.rk, imported with
                    `import <Name> from \"$lib/<Name>.rk\";`

  -p, --package     the application (default: the one in this directory)
";

pub fn main(args: Vec<String>) -> Result<(), CliError> {
    let mut args = args.into_iter();
    let mut package = None;
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => package = Some(value(&mut args, "--package")?),
            "-h" | "--help" => return Err(CliError::Help),
            other if other.starts_with('-') && other != "/" => return Err(CliError::Usage(format!("unknown option `{other}`"))),
            _ => positional.push(arg),
        }
    }
    let [kind, name] = positional.as_slice() else {
        return Err(CliError::Usage(if positional.len() < 2 { "`add` needs a kind and a name".into() } else { format!("unexpected argument `{}`", positional[2]) }));
    };
    let plan = match kind.as_str() {
        "page" => page(name)?,
        "layout" => layout(name)?,
        "component" => component(name)?,
        other => return Err(CliError::Usage(format!("unknown kind `{other}`; use page, layout or component"))),
    };
    let ws = cargo::app(package.as_deref())?;
    let dir = ws.dir();
    for taken in &plan.conflicts {
        if dir.join(taken).exists() {
            return Err(CliError::Failed(format!("{} already exists", taken.display())));
        }
    }
    let path = dir.join(&plan.file);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    fs::write(&path, &plan.body).map_err(|e| format!("writing {}: {e}", path.display()))?;
    eprintln!("\x1b[32m     Created\x1b[0m {} in `{}`", plan.file.display(), ws.package);
    eprintln!("        {}", plan.note);
    Ok(())
}

#[derive(Debug)]
struct Plan {
    file: PathBuf,
    /// Files whose existence means the new one would clash.
    conflicts: Vec<PathBuf>,
    body: String,
    note: String,
}

/// The route's segments without slashes or `.rk`. Empty for the home page.
fn segments(route: &str) -> Result<Vec<&str>, String> {
    let route = route.trim_matches('/');
    let route = route.strip_suffix(".rk").unwrap_or(route);
    if route.is_empty() {
        return Ok(Vec::new());
    }
    let segs: Vec<&str> = route.split('/').collect();
    let mut params = 0;
    for s in &segs {
        if let Some(name) = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            params += 1;
            let ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                return Err(format!("`[{name}]` must be a plain identifier such as `[slug]`"));
            }
        } else if s.is_empty() || s.starts_with('.') || !s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
            return Err(format!("`{s}` can't be a route segment: use letters, digits, `-`, `_` and `.`, or `[param]`"));
        }
    }
    if params > 1 {
        return Err("a route can have only one `[param]` segment".into());
    }
    if segs.last() == Some(&"layout") {
        return Err("`layout` names a layout file; use `rustkit add layout`".into());
    }
    Ok(segs)
}

fn url(segs: &[&str]) -> String {
    let parts: Vec<String> = segs.iter().map(|s| match s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(p) => format!("{{{p}}}"),
        None => (*s).to_owned(),
    }).collect();
    format!("/{}", parts.join("/"))
}

fn page(route: &str) -> Result<Plan, String> {
    let segs = segments(route)?;
    let routes = Path::new("src/routes");
    let (file, conflicts) = match segs.split_last() {
        None => (routes.join("index.rk"), vec![routes.join("index.rk")]),
        Some((last, parent)) => {
            let dir = parent.iter().fold(routes.to_path_buf(), |d, s| d.join(s));
            let flat = dir.join(format!("{last}.rk"));
            let nested = dir.join(last).join("index.rk");
            (flat.clone(), vec![flat, nested])
        }
    };
    let param = segs.iter().find_map(|s| s.strip_prefix('[').and_then(|s| s.strip_suffix(']')));
    let title = match segs.iter().rev().find(|s| !s.starts_with('[')) {
        Some(s) => title_case(s),
        None => "Home".into(),
    };
    let mut body = format!("<title>{title}</title>\n\n<h1>{title}</h1>\n");
    match param {
        // `{…}` in a template is an expression, so show the segment as `[p]`.
        Some(p) => body.push_str(&format!("<p>This page serves <code>/{}</code>. Here <code>{p}</code> is <strong>{{{p}}}</strong>.</p>\n", segs.join("/"))),
        None => body.push_str(&format!("<p>This page serves <code>{}</code>. Edit it in <code>{}</code>.</p>\n", url(&segs), file.display())),
    }
    Ok(Plan { note: format!("serves {}", url(&segs)), file, conflicts, body })
}

fn layout(dir: &str) -> Result<Plan, String> {
    let segs = segments(dir)?;
    let file = segs.iter().fold(PathBuf::from("src/routes"), |d, s| d.join(s)).join("layout.rk");
    let body = "<header>\n  <a href=\"/\">Home</a>\n</header>\n<slot />\n".to_owned();
    let scope = if segs.is_empty() { "every page".into() } else { format!("every page under {}", url(&segs)) };
    Ok(Plan { note: format!("wraps {scope}"), conflicts: vec![file.clone()], file, body })
}

fn component(name: &str) -> Result<Plan, String> {
    let name = name.strip_suffix(".rk").unwrap_or(name);
    let ok = name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && name.chars().all(|c| c.is_ascii_alphanumeric());
    if !ok {
        return Err(format!("`{name}` isn't a component name: use PascalCase, such as `Button` or `UserCard`"));
    }
    let class = kebab(name);
    let file = PathBuf::from(format!("src/lib/{name}.rk"));
    let body = format!(
        "<div class=\"{class}\">\n  <strong>{{label}}</strong>\n  <slot />\n</div>\n\n<style>\n  .{class} {{ display: block; }}\n</style>\n"
    );
    let note = format!("use it with `import {name} from \"$lib/{name}.rk\";` and `<{name} label=\"…\">…</{name}>`");
    Ok(Plan { conflicts: vec![file.clone()], file, body, note })
}

fn title_case(s: &str) -> String {
    s.split(['-', '_', '.'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn kebab(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_map_to_files_and_urls() {
        let p = page("about").unwrap();
        assert_eq!(p.file, Path::new("src/routes/about.rk"));
        assert_eq!(p.conflicts[1], Path::new("src/routes/about/index.rk"));
        assert!(p.body.contains("<title>About</title>"));
        let p = page("/blog/[slug]").unwrap();
        assert_eq!(p.file, Path::new("src/routes/blog/[slug].rk"));
        assert_eq!(p.note, "serves /blog/{slug}");
        assert!(p.body.contains("<code>/blog/[slug]</code>"));
        assert!(p.body.contains("<strong>{slug}</strong>"));
        assert_eq!(page("/").unwrap().file, Path::new("src/routes/index.rk"));
    }

    #[test]
    fn bad_routes_are_rejected() {
        assert!(page("a/[x]/[y]").is_err());
        assert!(page("a b").is_err());
        assert!(page("[1x]").is_err());
        assert!(page("blog/layout").is_err());
        assert!(page("../escape").is_err());
    }

    #[test]
    fn layouts_and_components() {
        assert_eq!(layout("/").unwrap().file, Path::new("src/routes/layout.rk"));
        assert_eq!(layout("blog").unwrap().file, Path::new("src/routes/blog/layout.rk"));
        let c = component("UserCard").unwrap();
        assert_eq!(c.file, Path::new("src/lib/UserCard.rk"));
        assert!(c.body.contains("class=\"user-card\""));
        assert!(component("button").is_err());
    }
}
