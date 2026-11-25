//! `rustkit.toml`, read from the crate root by [`crate::Build::run`].
//!
//! ```toml
//! css_scope = false
//! ```
//!
//! Scoping stays on when the file is missing or does not set `css_scope`.

use std::fs;
use std::path::Path;

pub struct FileConfig {
    pub css_scope: bool,
}

pub fn load(dir: &Path) -> Result<FileConfig, String> {
    let path = dir.join("rustkit.toml");
    if !path.is_file() {
        return Ok(FileConfig { css_scope: true });
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("error: reading {}: {e}\n", path.display()))?;
    parse(&text).map_err(|e| format!("error: {}: {e}\n", path.display()))
}

fn parse(text: &str) -> Result<FileConfig, String> {
    let mut css_scope = true;
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `key = value`", n + 1));
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "css_scope" => {
                css_scope = match value {
                    "true" => true,
                    "false" => false,
                    _ => return Err(format!("line {}: `css_scope` must be true or false", n + 1)),
                }
            }
            _ => return Err(format!("line {}: unknown setting `{key}`", n + 1)),
        }
    }
    Ok(FileConfig { css_scope })
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut quote = '\0';
    for (i, c) in line.char_indices() {
        if in_string {
            if c == quote {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = true;
                quote = c;
            }
            '#' => return &line[..i],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_turns_scoping_off_and_defaults_to_on() {
        assert!(parse("").unwrap().css_scope);
        assert!(parse("# comment\n").unwrap().css_scope);
        assert!(!parse("css_scope = false\n").unwrap().css_scope);
        assert!(parse("css_scope = true # keep it\n").unwrap().css_scope);
        assert!(parse("not a setting").is_err());
        assert!(parse("css_scope = yes").is_err());
        assert!(parse("other = true").is_err());
    }
}
