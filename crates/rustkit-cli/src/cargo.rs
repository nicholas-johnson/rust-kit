//! Finding Rust-kit applications with `cargo metadata`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One application package and the workspace it lives in.
pub struct Workspace {
    pub root: PathBuf,
    pub target: PathBuf,
    pub package: String,
    pub manifest: String,
}

impl Workspace {
    /// The package's directory, which holds `src/routes`.
    pub fn dir(&self) -> &Path {
        Path::new(&self.manifest).parent().unwrap_or(Path::new("."))
    }
}

/// Every binary package in the workspace that depends on `rustkit`, or the
/// one named `wanted`.
pub fn apps(wanted: Option<&str>) -> Result<Vec<Workspace>, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("running cargo: {e}"))?;
    if !out.status.success() {
        return Err("`cargo metadata` failed; run this inside a Cargo project".into());
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let has_bin = |p: &serde_json::Value| {
        p["targets"].as_array().is_some_and(|ts| ts.iter().any(|t| t["kind"].as_array().is_some_and(|k| k.iter().any(|k| k == "bin"))))
    };
    let uses_rustkit =
        |p: &serde_json::Value| p["dependencies"].as_array().is_some_and(|ds| ds.iter().any(|d| d["name"] == "rustkit"));
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
    let found: Vec<Workspace> = packages
        .iter()
        .filter(|p| match wanted {
            Some(name) => p["name"] == name,
            None => has_bin(p) && uses_rustkit(p),
        })
        .map(|p| Workspace {
            root: PathBuf::from(s(&meta["workspace_root"])),
            target: PathBuf::from(s(&meta["target_directory"])),
            package: s(&p["name"]),
            manifest: s(&p["manifest_path"]),
        })
        .collect();
    if found.is_empty() {
        return Err(match wanted {
            Some(name) => format!("no package named `{name}` in this workspace"),
            None => "no binary package in this workspace depends on rustkit; pass `-p <package>`".into(),
        });
    }
    Ok(found)
}

/// The one application a command acts on: the package named `wanted`, the
/// only application in the workspace, or the one whose directory holds the
/// current directory.
pub fn app(wanted: Option<&str>) -> Result<Workspace, String> {
    let mut found = apps(wanted)?;
    if found.len() == 1 {
        return Ok(found.remove(0));
    }
    if let Ok(cwd) = std::env::current_dir() {
        let cwd = cwd.canonicalize().unwrap_or(cwd);
        if let Some(i) = found.iter().position(|w| w.dir().canonicalize().is_ok_and(|d| cwd.starts_with(d))) {
            return Ok(found.remove(i));
        }
    }
    let names: Vec<&str> = found.iter().map(|w| w.package.as_str()).collect();
    Err(format!("several applications to choose from ({}); pass `-p <package>`", names.join(", ")))
}
