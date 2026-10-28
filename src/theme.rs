use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::builder::{PossibleValue, StringValueParser, TypedValueParser};

use crate::model::TimerStatus;

include!(concat!(env!("OUT_DIR"), "/builtin_themes.rs"));

const MAX_THEME_BYTES: u64 = 256 * 1024;
const MAX_FRAMES_PER_STATE: usize = 64;
const MAX_FRAME_WIDTH: usize = 120;
const MAX_FRAME_HEIGHT: usize = 30;
const MAX_THEME_NAME_CHARS: usize = 64;

#[derive(Clone)]
pub struct ThemeNameParser;

impl TypedValueParser for ThemeNameParser {
    type Value = String;

    fn parse_ref(
        &self,
        command: &clap::Command,
        argument: Option<&clap::Arg>,
        value: &std::ffi::OsStr,
    ) -> std::result::Result<Self::Value, clap::Error> {
        StringValueParser::new().parse_ref(command, argument, value)
    }

    fn possible_values(&self) -> Option<Box<dyn Iterator<Item = PossibleValue> + '_>> {
        Some(Box::new(BUILTIN_THEME_SOURCES.iter().map(|(id, _)| {
            PossibleValue::new(*id).help("Bundled theme")
        })))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone)]
struct Frame {
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Theme {
    id: String,
    name: String,
    fps: u16,
    alignment: Alignment,
    running: Vec<Frame>,
    paused: Vec<Frame>,
    done: Vec<Frame>,
    canvas_width: usize,
    canvas_height: usize,
    origin: ThemeOrigin,
}

#[derive(Debug, Clone)]
enum ThemeOrigin {
    BuiltIn,
    User(PathBuf),
    File(PathBuf),
}

impl ThemeOrigin {
    fn label(&self) -> String {
        match self {
            Self::BuiltIn => "built-in".to_owned(),
            Self::User(path) | Self::File(path) => path.display().to_string(),
        }
    }
}

#[derive(Debug)]
pub struct InvalidTheme {
    pub id: String,
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug)]
pub struct ThemeCatalog {
    themes: Vec<Theme>,
    invalid: Vec<InvalidTheme>,
}

#[derive(Debug)]
pub struct ThemeSession {
    themes: Vec<Theme>,
    selected: Option<usize>,
    animation_started: Instant,
    last_status: Option<TimerStatus>,
}

#[derive(Debug, Clone, Copy)]
enum FrameGroup {
    Running,
    Paused,
    Done,
}

impl Theme {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fps(&self) -> u16 {
        self.fps
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.canvas_width, self.canvas_height)
    }

    pub fn origin(&self) -> String {
        self.origin.label()
    }

    fn frames_for(&self, status: TimerStatus) -> &[Frame] {
        match status {
            TimerStatus::Paused if !self.paused.is_empty() => &self.paused,
            TimerStatus::Done if !self.done.is_empty() => &self.done,
            _ => &self.running,
        }
    }

    fn render(&self, status: TimerStatus, elapsed: Duration, terminal_width: usize) -> Vec<String> {
        if self.canvas_width > terminal_width {
            return Vec::new();
        }
        let frames = self.frames_for(status);
        let period_millis = 1_000_u128 / u128::from(self.fps);
        let index = ((elapsed.as_millis() / period_millis) % frames.len() as u128) as usize;
        let prefix = match self.alignment {
            Alignment::Left => 0,
            Alignment::Center => terminal_width.saturating_sub(self.canvas_width) / 2,
            Alignment::Right => terminal_width.saturating_sub(self.canvas_width),
        };
        frames[index]
            .lines
            .iter()
            .map(|line| format!("{}{line}", " ".repeat(prefix)))
            .collect()
    }

    fn completion_hold(&self, status: TimerStatus) -> Duration {
        if status != TimerStatus::Done || self.done.is_empty() {
            return Duration::ZERO;
        }
        let cycle = self.done.len() as u64 * 1_000 / u64::from(self.fps);
        Duration::from_millis(cycle.clamp(1_000, 2_500))
    }
}

impl ThemeCatalog {
    pub fn load() -> Result<Self> {
        Self::load_from_directory(&user_theme_dir()?)
    }

    fn load_from_directory(directory: &Path) -> Result<Self> {
        let mut themes = BTreeMap::new();
        for (id, contents) in BUILTIN_THEME_SOURCES {
            let theme = parse_theme(id, contents, ThemeOrigin::BuiltIn)
                .with_context(|| format!("built-in theme `{id}` is invalid"))?;
            themes.insert((*id).to_owned(), theme);
        }

        let mut invalid = Vec::new();
        let entries = match fs::read_dir(directory) {
            Ok(entries) => {
                let mut paths = entries
                    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .filter(|path| {
                        path.extension().and_then(|value| value.to_str()) == Some("toml")
                    })
                    .collect::<Vec<_>>();
                paths.sort();
                paths
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to read {}", directory.display()));
            }
        };

        for path in entries {
            let id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned();
            let parsed = read_theme_file(&path)
                .and_then(|contents| parse_theme(&id, &contents, ThemeOrigin::User(path.clone())));
            match parsed {
                Ok(theme) => {
                    themes.insert(id, theme);
                }
                Err(error) => {
                    themes.remove(&id);
                    invalid.push(InvalidTheme {
                        id,
                        path,
                        error: format!("{error:#}"),
                    });
                }
            }
        }

        Ok(Self {
            themes: themes.into_values().collect(),
            invalid,
        })
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn invalid(&self) -> &[InvalidTheme] {
        &self.invalid
    }

    pub fn into_session(self, selected: &str) -> Result<ThemeSession> {
        if let Some(invalid) = self.invalid.iter().find(|theme| theme.id == selected) {
            bail!(
                "theme `{selected}` is invalid ({}): {}",
                invalid.path.display(),
                invalid.error
            );
        }
        let Some(index) = self.themes.iter().position(|theme| theme.id == selected) else {
            let available = self
                .themes
                .iter()
                .map(|theme| theme.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            if available.is_empty() {
                bail!("theme `{selected}` was not found; no valid themes are installed");
            }
            bail!("theme `{selected}` was not found; available themes: {available}");
        };
        Ok(ThemeSession {
            themes: self.themes,
            selected: Some(index),
            animation_started: Instant::now(),
            last_status: None,
        })
    }
}

impl ThemeSession {
    pub fn load(selected: &str) -> Result<Self> {
        ThemeCatalog::load()?.into_session(selected)
    }

    pub fn current_name(&self) -> &str {
        self.selected
            .and_then(|index| self.themes.get(index))
            .map(|theme| theme.id.as_str())
            .unwrap_or("none")
    }

    pub fn render(&mut self, status: TimerStatus, terminal_width: usize) -> Vec<String> {
        if self.last_status != Some(status) {
            self.last_status = Some(status);
            self.animation_started = Instant::now();
        }
        self.current()
            .map(|theme| theme.render(status, self.animation_started.elapsed(), terminal_width))
            .unwrap_or_default()
    }

    pub fn next(&mut self) {
        self.selected = match self.selected {
            Some(index) if index + 1 < self.themes.len() => Some(index + 1),
            Some(_) => None,
            None if self.themes.is_empty() => None,
            None => Some(0),
        };
        self.reset_animation();
    }

    pub fn previous(&mut self) {
        self.selected = match self.selected {
            Some(0) => None,
            Some(index) => Some(index - 1),
            None if self.themes.is_empty() => None,
            None => Some(self.themes.len() - 1),
        };
        self.reset_animation();
    }

    pub fn completion_hold(&self, status: TimerStatus) -> Duration {
        self.current()
            .map(|theme| theme.completion_hold(status))
            .unwrap_or_default()
    }

    fn current(&self) -> Option<&Theme> {
        self.selected.and_then(|index| self.themes.get(index))
    }

    fn reset_animation(&mut self) {
        self.animation_started = Instant::now();
        self.last_status = None;
    }
}

pub fn user_theme_dir() -> Result<PathBuf> {
    let base = match env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => {
            let home = env::var_os("HOME").context("HOME is not set")?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("timerd-rs/themes"))
}

pub fn validate_file(path: &Path) -> Result<Theme> {
    let id = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow!("theme filename must be valid UTF-8"))?;
    let contents = read_theme_file(path)?;
    parse_theme(id, &contents, ThemeOrigin::File(path.to_owned()))
        .with_context(|| format!("failed to parse {}", path.display()))
}

fn read_theme_file(path: &Path) -> Result<String> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to inspect {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if metadata.len() > MAX_THEME_BYTES {
        bail!(
            "{} is too large (maximum {} KiB)",
            path.display(),
            MAX_THEME_BYTES / 1024
        );
    }
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

fn parse_theme(id: &str, contents: &str, origin: ThemeOrigin) -> Result<Theme> {
    validate_id(id)?;
    let lines = contents.lines().collect::<Vec<_>>();
    let mut index = 0;
    let mut version = None;
    let mut name = None;
    let mut fps = None;
    let mut alignment = None;
    let mut pending_group = None;
    let mut running = Vec::new();
    let mut paused = Vec::new();
    let mut done = Vec::new();

    while index < lines.len() {
        let line_number = index + 1;
        let original = lines[index].trim_end_matches('\r');
        let line = strip_comment(original).trim();
        if line.is_empty() {
            index += 1;
            continue;
        }

        if line.starts_with("[[") {
            if pending_group.is_some() {
                bail!("line {line_number}: previous frame is missing `art`");
            }
            let Some(section) = line
                .strip_prefix("[[")
                .and_then(|line| line.strip_suffix("]]"))
            else {
                bail!("line {line_number}: invalid frame section");
            };
            pending_group = Some(match section.trim() {
                "running" => FrameGroup::Running,
                "paused" => FrameGroup::Paused,
                "done" => FrameGroup::Done,
                other => bail!("line {line_number}: unknown frame state `{other}`"),
            });
            index += 1;
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            bail!("line {line_number}: expected `key = value`");
        };
        let key = key.trim();
        let value = value.trim();

        if let Some(group) = pending_group {
            if key != "art" {
                bail!("line {line_number}: a frame only supports the `art` field");
            }
            if value != "\"\"\"" {
                bail!("line {line_number}: `art` must start with three double quotes");
            }
            index += 1;
            let mut art = Vec::new();
            let mut closed = false;
            while index < lines.len() {
                let art_line = lines[index].trim_end_matches('\r');
                if art_line.trim() == "\"\"\"" {
                    closed = true;
                    break;
                }
                validate_art_line(art_line, index + 1)?;
                art.push(art_line.trim_end().to_owned());
                index += 1;
            }
            if !closed {
                bail!("line {line_number}: unterminated `art` string");
            }
            let frame = make_frame(art, line_number)?;
            let target = match group {
                FrameGroup::Running => &mut running,
                FrameGroup::Paused => &mut paused,
                FrameGroup::Done => &mut done,
            };
            if target.len() >= MAX_FRAMES_PER_STATE {
                bail!(
                    "line {line_number}: a theme may contain at most {MAX_FRAMES_PER_STATE} frames per state"
                );
            }
            target.push(frame);
            pending_group = None;
            index += 1;
            continue;
        }

        match key {
            "version" => set_once(
                &mut version,
                parse_integer(value, line_number, "version")?,
                line_number,
                key,
            )?,
            "name" => set_once(
                &mut name,
                parse_theme_name(value, line_number)?,
                line_number,
                key,
            )?,
            "fps" => {
                let parsed = parse_integer(value, line_number, "fps")?;
                let parsed = u16::try_from(parsed)
                    .map_err(|_| anyhow!("line {line_number}: `fps` is too large"))?;
                if !(1..=20).contains(&parsed) {
                    bail!("line {line_number}: `fps` must be between 1 and 20");
                }
                set_once(&mut fps, parsed, line_number, key)?;
            }
            "align" => {
                let parsed = match parse_string(value, line_number, "align")? {
                    "left" => Alignment::Left,
                    "center" => Alignment::Center,
                    "right" => Alignment::Right,
                    other => bail!(
                        "line {line_number}: invalid alignment `{other}`; expected left, center, or right"
                    ),
                };
                set_once(&mut alignment, parsed, line_number, key)?;
            }
            other => bail!("line {line_number}: unknown theme setting `{other}`"),
        }
        index += 1;
    }

    if pending_group.is_some() {
        bail!("last frame is missing `art`");
    }
    if version != Some(1) {
        bail!("theme must declare `version = 1`");
    }
    if running.is_empty() {
        bail!("theme must contain at least one `[[running]]` frame");
    }

    let canvas_width = running
        .iter()
        .chain(&paused)
        .chain(&done)
        .flat_map(|frame| &frame.lines)
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let canvas_height = running
        .iter()
        .chain(&paused)
        .chain(&done)
        .map(|frame| frame.lines.len())
        .max()
        .unwrap_or(0);
    normalize_frames(&mut running, canvas_width, canvas_height);
    normalize_frames(&mut paused, canvas_width, canvas_height);
    normalize_frames(&mut done, canvas_width, canvas_height);

    Ok(Theme {
        id: id.to_owned(),
        name: name.unwrap_or_else(|| id.to_owned()),
        fps: fps.unwrap_or(4),
        alignment: alignment.unwrap_or(Alignment::Center),
        running,
        paused,
        done,
        canvas_width,
        canvas_height,
        origin,
    })
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("theme filename must contain only ASCII letters, numbers, `-`, or `_`");
    }
    Ok(())
}

fn validate_art_line(line: &str, line_number: usize) -> Result<()> {
    if !line.is_ascii() || line.chars().any(char::is_control) {
        bail!("line {line_number}: theme art must contain printable ASCII characters only");
    }
    if line.chars().count() > MAX_FRAME_WIDTH {
        bail!("line {line_number}: theme art is wider than {MAX_FRAME_WIDTH} columns");
    }
    Ok(())
}

fn parse_theme_name(value: &str, line_number: usize) -> Result<String> {
    let name = parse_string(value, line_number, "name")?;
    if name.is_empty() {
        bail!("line {line_number}: `name` may not be empty");
    }
    if name.chars().any(char::is_control) {
        bail!("line {line_number}: `name` may not contain control characters");
    }
    if name.chars().count() > MAX_THEME_NAME_CHARS {
        bail!("line {line_number}: `name` is longer than {MAX_THEME_NAME_CHARS} characters");
    }
    Ok(name.to_owned())
}

fn make_frame(lines: Vec<String>, line_number: usize) -> Result<Frame> {
    if lines.is_empty() || lines.iter().all(|line| line.trim().is_empty()) {
        bail!("line {line_number}: theme frame may not be empty");
    }
    if lines.len() > MAX_FRAME_HEIGHT {
        bail!("line {line_number}: theme frame is taller than {MAX_FRAME_HEIGHT} rows");
    }
    Ok(Frame { lines })
}

fn normalize_frames(frames: &mut [Frame], width: usize, height: usize) {
    for frame in frames {
        for line in &mut frame.lines {
            line.push_str(&" ".repeat(width.saturating_sub(line.chars().count())));
        }
        frame.lines.resize(height, " ".repeat(width));
    }
}

fn parse_integer(value: &str, line_number: usize, key: &str) -> Result<u64> {
    value
        .parse()
        .with_context(|| format!("line {line_number}: `{key}` must be an integer"))
}

fn parse_string<'a>(value: &'a str, line_number: usize, key: &str) -> Result<&'a str> {
    quoted_string(value)
        .ok_or_else(|| anyhow!("line {line_number}: `{key}` must be a quoted string"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, line_number: usize, key: &str) -> Result<()> {
    if slot.is_some() {
        bail!("line {line_number}: duplicate theme setting `{key}`");
    }
    *slot = Some(value);
    Ok(())
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (index, character) in line.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if character == '#' && !quoted {
            return &line[..index];
        }
    }
    line
}

fn quoted_string(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| !value.contains('"'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: &str = r#"
version = 1
name = "Test Cat"
fps = 5
align = "center"

[[running]]
art = """
 /\_/\\
( o.o )
"""

[[running]]
art = """
 /\_/\\
( -.- )
"""

[[paused]]
art = """
 zzz
"""

[[done]]
art = """
 yay
"""
"#;

    #[test]
    fn parses_and_normalizes_animation_frames() {
        let theme = parse_theme("test-cat", THEME, ThemeOrigin::BuiltIn).unwrap();
        assert_eq!(theme.name(), "Test Cat");
        assert_eq!(theme.fps(), 5);
        assert_eq!(theme.running.len(), 2);
        assert_eq!(theme.paused.len(), 1);
        assert_eq!(theme.done.len(), 1);
        assert!(
            theme
                .running
                .iter()
                .flat_map(|frame| &frame.lines)
                .all(|line| line.chars().count() == theme.canvas_width)
        );
    }

    #[test]
    fn rejects_control_sequences_and_missing_running_frames() {
        let escape = "version = 1\n[[running]]\nart = \"\"\"\n\u{1b}[2J\n\"\"\"\n";
        assert!(parse_theme("bad", escape, ThemeOrigin::BuiltIn).is_err());

        let unsafe_name =
            "version = 1\nname = \"\u{1b}[2J\"\n[[running]]\nart = \"\"\"\ncat\n\"\"\"\n";
        assert!(parse_theme("bad", unsafe_name, ThemeOrigin::BuiltIn).is_err());

        let missing = "version = 1\n[[done]]\nart = \"\"\"\ndone\n\"\"\"\n";
        assert!(parse_theme("bad", missing, ThemeOrigin::BuiltIn).is_err());
    }

    #[test]
    fn session_cycles_through_themes_and_none() {
        let theme = parse_theme("test-cat", THEME, ThemeOrigin::BuiltIn).unwrap();
        let mut session = ThemeSession {
            themes: vec![theme],
            selected: Some(0),
            animation_started: Instant::now(),
            last_status: None,
        };
        assert_eq!(session.current_name(), "test-cat");
        session.next();
        assert_eq!(session.current_name(), "none");
        session.next();
        assert_eq!(session.current_name(), "test-cat");
        session.previous();
        assert_eq!(session.current_name(), "none");
    }

    #[test]
    fn every_embedded_theme_is_valid() {
        for (id, contents) in BUILTIN_THEME_SOURCES {
            parse_theme(id, contents, ThemeOrigin::BuiltIn)
                .unwrap_or_else(|error| panic!("embedded theme `{id}` is invalid: {error:#}"));
        }
    }

    #[test]
    fn done_animation_only_holds_natural_completion() {
        let theme = parse_theme("test-cat", THEME, ThemeOrigin::BuiltIn).unwrap();
        assert!(theme.completion_hold(TimerStatus::Done) > Duration::ZERO);
        assert_eq!(theme.completion_hold(TimerStatus::Stopped), Duration::ZERO);
        assert_eq!(
            theme.completion_hold(TimerStatus::Cancelled),
            Duration::ZERO
        );
    }

    #[test]
    fn discovers_a_theme_dropped_into_the_user_directory() {
        let directory = std::env::temp_dir().join(format!(
            "timerd-rs-theme-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("custom-cat.toml"), THEME).unwrap();

        let catalog = ThemeCatalog::load_from_directory(&directory).unwrap();
        let discovered = catalog
            .themes()
            .iter()
            .find(|theme| theme.id() == "custom-cat")
            .unwrap();
        assert_eq!(discovered.name(), "Test Cat");
        assert!(matches!(discovered.origin, ThemeOrigin::User(_)));

        fs::remove_dir_all(directory).unwrap();
    }
}
