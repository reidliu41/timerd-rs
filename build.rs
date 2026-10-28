use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=themes");

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let themes_dir = manifest_dir.join("themes");
    let mut themes = fs::read_dir(&themes_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", themes_dir.display()))
        .map(|entry| {
            entry
                .expect("failed to read a theme directory entry")
                .path()
        })
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("toml"))
        .collect::<Vec<_>>();
    themes.sort();

    let mut generated = String::from("pub const BUILTIN_THEME_SOURCES: &[(&str, &str)] = &[\n");
    for path in themes {
        let id = path
            .file_stem()
            .and_then(|value| value.to_str())
            .expect("theme filenames must be valid UTF-8");
        assert!(
            valid_id(id),
            "theme filename `{id}` must contain only ASCII letters, numbers, `-`, or `_`"
        );
        generated.push_str(&format!(
            "    ({id:?}, include_str!({path:?})),\n",
            path = path.to_string_lossy()
        ));
    }
    generated.push_str("];\n");

    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("builtin_themes.rs");
    fs::write(&output, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", output.display()));
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
