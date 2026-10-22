use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::display::DisplaySize;

#[derive(Debug)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub exists: bool,
    pub display_size: DisplaySize,
}

pub fn path() -> Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => {
            let home = env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("timerd-rs/config.toml"))
}

pub fn load() -> Result<LoadedConfig> {
    let path = path()?;
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedConfig {
                path,
                exists: false,
                display_size: DisplaySize::default(),
            });
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", path.display()));
        }
    };
    let display_size = parse_display_size(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(LoadedConfig {
        path,
        exists: true,
        display_size,
    })
}

fn parse_display_size(contents: &str) -> Result<DisplaySize> {
    let mut in_display = false;
    let mut display_size = None;

    for (index, original_line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = strip_comment(original_line).trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            let Some(section) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            else {
                bail!("line {line_number}: invalid section header");
            };
            if section.trim() != "display" {
                bail!("line {line_number}: unknown section `{}`", section.trim());
            }
            in_display = true;
            continue;
        }

        if !in_display {
            bail!("line {line_number}: expected `[display]` before a setting");
        }
        let Some((key, value)) = line.split_once('=') else {
            bail!("line {line_number}: expected `key = value`");
        };
        let key = key.trim();
        if key != "size" {
            bail!("line {line_number}: unknown display setting `{key}`");
        }
        if display_size.is_some() {
            bail!("line {line_number}: duplicate display setting `size`");
        }

        let value = value.trim();
        let Some(value) = quoted_string(value) else {
            bail!("line {line_number}: `size` must be a quoted string");
        };
        display_size = Some(DisplaySize::from_config_value(value).with_context(|| {
            format!(
                "line {line_number}: invalid size `{value}`; expected small, medium, large, or xlarge"
            )
        })?);
    }

    Ok(display_size.unwrap_or_default())
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
        } else if character == '#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}

fn quoted_string(value: &str) -> Option<&str> {
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') || value.chars().last()? != quote || value.len() < 2 {
        return None;
    }
    let inner = &value[quote.len_utf8()..value.len() - quote.len_utf8()];
    (!inner.contains(quote)).then_some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_small_display() {
        assert_eq!(parse_display_size("").unwrap(), DisplaySize::Small);
    }

    #[test]
    fn parses_display_size() {
        assert_eq!(
            parse_display_size("[display]\nsize = \"xlarge\" # preferred\n").unwrap(),
            DisplaySize::Xlarge
        );
    }

    #[test]
    fn rejects_typos_instead_of_ignoring_them() {
        let error = parse_display_size("[display]\nszie = \"large\"\n").unwrap_err();
        assert!(error.to_string().contains("unknown display setting `szie`"));
    }
}
