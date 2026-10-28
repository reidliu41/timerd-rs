# Themes

Add one `*.toml` file to this directory to bundle another animated terminal
theme. The filename becomes the CLI name; for example, `rocket.toml` is used
with `timer 5m --theme rocket`. `build.rs` discovers all theme files, so no
Rust source file needs to be updated.

Use `sleepy-cat.toml` as the format reference, then validate a new file with:

```bash
cargo run -- theme check themes/rocket.toml
```

Every theme must have `version = 1` and one or more `[[running]]` frames.
`[[paused]]` and `[[done]]` are optional. Supported metadata is `name`, `fps`
(1–20), and `align` (`left`, `center`, or `right`). Keep each frame within 120
columns and 30 rows, using printable ASCII characters only.
