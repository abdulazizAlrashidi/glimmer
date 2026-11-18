//! Saving and loading the settings and the high-score table. Both are plain
//! text files in the per-user data directory (std only, no network).
//!
//! Data directory: macOS `~/Library/Application Support/Glimmer`, Windows
//! `%APPDATA%\Glimmer`, other Unix `$XDG_DATA_HOME/glimmer` or
//! `~/.local/share/glimmer`. Falls back to `./glimmer-data`. Writes are atomic
//! (write temp file, then rename). Unreadable/corrupt files yield defaults.
//!
//! # File formats
//! Both files are human-editable UTF-8 text; lines starting with `#` are
//! comments and blank lines are ignored.
//!
//! - [`SETTINGS_FILE`]: `key=value` lines (keys are the [`Settings`] field
//!   names). Unknown keys and unparsable values are ignored, out-of-range
//!   numbers are clamped.
//! - [`SCORES_FILE`]: one entry per line, `NAME SCORE LEVEL`, where spaces
//!   inside the 3-character name are written as `_` (e.g. `A_B 12340 4`).
//!   Malformed lines are skipped.

use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// File name of the settings file inside [`data_dir`].
pub const SETTINGS_FILE: &str = "settings.txt";
/// File name of the high-score file inside [`data_dir`].
pub const SCORES_FILE: &str = "highscores.txt";

/// Length of a high-score name, in characters.
pub const NAME_LEN: usize = 3;
/// Every character allowed in a high-score name, in name-entry cycling order.
pub const NAME_CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-!? ";

/// Files larger than this are only read up to this many bytes (a sane data
/// file is a few hundred bytes; this keeps a corrupt huge file harmless).
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Volumes go from 0 to this, inclusive.
const VOLUME_MAX: u8 = 10;
/// Inclusive range of starting lives.
const LIVES_MIN: u8 = 1;
const LIVES_MAX: u8 = 5;
/// Inclusive range of a non-zero extra-life threshold.
const EXTRA_LIFE_MIN: u32 = 5_000;
const EXTRA_LIFE_MAX: u32 = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0..=10 each.
    pub master_volume: u8,
    pub music_volume: u8,
    pub sfx_volume: u8,
    pub fullscreen: bool,
    pub screen_shake: bool,
    /// Subtle CRT scanline/vignette overlay.
    pub scanlines: bool,
    pub show_fps: bool,
    /// 1..=5 lives at the start of a game.
    pub start_lives: u8,
    /// Extra life threshold in points; 0 = disabled.
    pub extra_life_at: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            master_volume: 10,
            music_volume: 8,
            sfx_volume: 9,
            fullscreen: false,
            screen_shake: true,
            scanlines: false,
            show_fps: false,
            start_lives: 3,
            extra_life_at: 15_000,
        }
    }
}

impl Settings {
    /// `key=value` lines, with every value clamped to its valid range.
    pub fn to_text(&self) -> String {
        let s = self.clamped();
        format!(
            "# Glimmer settings (key=value; lines starting with # are ignored)\n\
             master_volume={}\n\
             music_volume={}\n\
             sfx_volume={}\n\
             fullscreen={}\n\
             screen_shake={}\n\
             scanlines={}\n\
             show_fps={}\n\
             start_lives={}\n\
             extra_life_at={}\n",
            s.master_volume,
            s.music_volume,
            s.sfx_volume,
            s.fullscreen,
            s.screen_shake,
            s.scanlines,
            s.show_fps,
            s.start_lives,
            s.extra_life_at,
        )
    }

    /// Parse `key=value` lines. Unknown keys are ignored and values are
    /// clamped to their valid ranges.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            // Tolerate trailing comments: `music_volume=5  # quieter`.
            let value = value.split('#').next().unwrap_or("").trim();
            let int = || value.parse::<i64>().ok();
            let flag = || parse_bool(value);
            match key.trim().to_ascii_lowercase().as_str() {
                "master_volume" => set(&mut s.master_volume, int().map(clamp_volume)),
                "music_volume" => set(&mut s.music_volume, int().map(clamp_volume)),
                "sfx_volume" => set(&mut s.sfx_volume, int().map(clamp_volume)),
                "fullscreen" => set(&mut s.fullscreen, flag()),
                "screen_shake" => set(&mut s.screen_shake, flag()),
                "scanlines" => set(&mut s.scanlines, flag()),
                "show_fps" => set(&mut s.show_fps, flag()),
                "start_lives" => set(&mut s.start_lives, int().map(clamp_lives)),
                "extra_life_at" => set(&mut s.extra_life_at, int().map(clamp_extra_life)),
                _ => {} // unknown keys (e.g. from a newer version) are ignored
            }
        }
        s
    }

    /// A copy with every field forced into its valid range.
    pub fn clamped(&self) -> Settings {
        Settings {
            master_volume: self.master_volume.min(VOLUME_MAX),
            music_volume: self.music_volume.min(VOLUME_MAX),
            sfx_volume: self.sfx_volume.min(VOLUME_MAX),
            start_lives: self.start_lives.clamp(LIVES_MIN, LIVES_MAX),
            extra_life_at: clamp_extra_life(i64::from(self.extra_life_at)),
            ..self.clone()
        }
    }
}

/// Overwrite `slot` only when a valid value was parsed.
fn set<T>(slot: &mut T, value: Option<T>) {
    if let Some(v) = value {
        *slot = v;
    }
}

fn clamp_volume(v: i64) -> u8 {
    // The clamp guarantees the value fits in a u8.
    v.clamp(0, i64::from(VOLUME_MAX)) as u8
}

fn clamp_lives(v: i64) -> u8 {
    v.clamp(i64::from(LIVES_MIN), i64::from(LIVES_MAX)) as u8
}

/// `<= 0` disables the extra life; anything else lands in the valid range.
fn clamp_extra_life(v: i64) -> u32 {
    if v <= 0 {
        0
    } else {
        v.clamp(i64::from(EXTRA_LIFE_MIN), i64::from(EXTRA_LIFE_MAX)) as u32
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    const TRUE: [&str; 4] = ["true", "1", "yes", "on"];
    const FALSE: [&str; 4] = ["false", "0", "no", "off"];
    if TRUE.iter().any(|t| v.eq_ignore_ascii_case(t)) {
        Some(true)
    } else if FALSE.iter().any(|f| v.eq_ignore_ascii_case(f)) {
        Some(false)
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighScore {
    /// Exactly 3 characters from `A-Z 0-9 . - ! ?` and space.
    pub name: String,
    pub score: u32,
    /// Level reached.
    pub level: u32,
}

impl HighScore {
    /// Build an entry, normalising `name` with [`sanitize_name`].
    pub fn new(name: &str, score: u32, level: u32) -> HighScore {
        HighScore {
            name: sanitize_name(name),
            score,
            level,
        }
    }
}

/// Is `c` allowed in a high-score name?
pub fn is_name_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '!' | '?' | ' ')
}

/// Normalise arbitrary input to a valid name: uppercased, invalid characters
/// replaced by spaces, truncated or space-padded to exactly [`NAME_LEN`].
pub fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(NAME_LEN);
    for c in raw.chars().take(NAME_LEN) {
        let c = c.to_ascii_uppercase();
        out.push(if is_name_char(c) { c } else { ' ' });
    }
    while out.len() < NAME_LEN {
        out.push(' ');
    }
    out
}

/// Top-10 table, sorted by score descending (ties: earlier entry first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighScores {
    pub entries: Vec<HighScore>,
}

impl HighScores {
    pub const MAX: usize = 10;

    /// A full table of modest default scores so a fresh install isn't empty.
    pub fn defaults() -> HighScores {
        const TABLE: [(&str, u32, u32); HighScores::MAX] = [
            ("GLM", 30_000, 6),
            ("LUX", 27_000, 6),
            ("NOX", 24_000, 5),
            ("ZEN", 21_000, 5),
            ("ORB", 18_000, 4),
            ("SKY", 15_000, 4),
            ("IVY", 12_000, 3),
            ("RAY", 9_000, 3),
            ("KAI", 6_000, 2),
            ("EVE", 3_000, 1),
        ];
        HighScores {
            entries: TABLE
                .iter()
                .map(|&(name, score, level)| HighScore::new(name, score, level))
                .collect(),
        }
    }

    /// Would `score` enter the table? (score > 0 and beats the 10th entry or
    /// the table has room.)
    pub fn qualifies(&self, score: u32) -> bool {
        score > 0
            && self
                .entries
                .get(Self::MAX - 1)
                .is_none_or(|last| score > last.score)
    }

    /// Insert, keep sorted, truncate to MAX; returns the 0-based rank if it stayed.
    ///
    /// The insertion is stable: a new entry goes *after* existing entries with
    /// an equal score (the earlier achiever keeps the higher rank). The name
    /// is normalised with [`sanitize_name`].
    pub fn insert(&mut self, entry: HighScore) -> Option<usize> {
        self.normalize();
        let entry = HighScore {
            name: sanitize_name(&entry.name),
            ..entry
        };
        let rank = self
            .entries
            .iter()
            .position(|e| e.score < entry.score)
            .unwrap_or(self.entries.len());
        if rank >= Self::MAX {
            return None;
        }
        self.entries.insert(rank, entry);
        self.entries.truncate(Self::MAX);
        Some(rank)
    }

    /// Best score in the table (0 if empty).
    pub fn best(&self) -> u32 {
        self.entries.first().map(|e| e.score).unwrap_or(0)
    }

    pub fn to_text(&self) -> String {
        let mut out = String::with_capacity(32 + self.entries.len() * 24);
        out.push_str("# Glimmer high scores: NAME SCORE LEVEL (spaces in names as _)\n");
        for e in &self.entries {
            for c in sanitize_name(&e.name).chars() {
                out.push(if c == ' ' { '_' } else { c });
            }
            out.push(' ');
            out.push_str(&e.score.to_string());
            out.push(' ');
            out.push_str(&e.level.to_string());
            out.push('\n');
        }
        out
    }

    /// Lenient parser; invalid lines skipped; result sorted and truncated.
    pub fn from_text(text: &str) -> HighScores {
        let mut scores = HighScores {
            entries: text.lines().filter_map(parse_score_line).collect(),
        };
        scores.normalize();
        scores
    }

    /// Stable-sort by score (descending) and drop entries beyond [`Self::MAX`].
    fn normalize(&mut self) {
        self.entries.sort_by_key(|e| Reverse(e.score));
        self.entries.truncate(Self::MAX);
    }
}

/// Parse one `NAME SCORE LEVEL` line; `None` for comments and malformed lines.
fn parse_score_line(line: &str) -> Option<HighScore> {
    let line = line.trim_start_matches('\u{feff}').trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split_whitespace();
    let (raw_name, score, level) = (fields.next()?, fields.next()?, fields.next()?);
    if fields.next().is_some() {
        return None;
    }
    let mut name = String::with_capacity(NAME_LEN);
    for c in raw_name.chars() {
        let c = if c == '_' {
            ' '
        } else {
            c.to_ascii_uppercase()
        };
        if !is_name_char(c) || name.len() >= NAME_LEN {
            return None;
        }
        name.push(c);
    }
    if name.len() != NAME_LEN {
        return None;
    }
    Some(HighScore {
        name,
        score: score.parse().ok()?,
        level: level.parse::<u32>().ok()?.max(1),
    })
}

// ---------------------------------------------------------------------------
// Filesystem
// ---------------------------------------------------------------------------

/// Platform families with distinct data-directory conventions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    MacOs,
    Windows,
    OtherUnix,
}

impl Platform {
    fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::OtherUnix
        }
    }
}

/// Resolve the data directory for `platform`, reading environment variables
/// through `env` (so the logic is testable without touching the real
/// environment). Empty variables count as unset.
fn resolve_data_dir(platform: Platform, env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let var = |key: &str| env(key).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = match platform {
        Platform::MacOs => var("HOME").map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("Glimmer")
        }),
        Platform::Windows => var("APPDATA").map(|appdata| appdata.join("Glimmer")),
        Platform::OtherUnix => var("XDG_DATA_HOME")
            // The XDG spec says relative paths are invalid and must be ignored.
            .filter(|p| p.is_absolute())
            .map(|xdg| xdg.join("glimmer"))
            .or_else(|| var("HOME").map(|home| home.join(".local").join("share").join("glimmer"))),
    };
    dir.unwrap_or_else(|| Path::new(".").join("glimmer-data"))
}

/// The per-user data directory (not created).
pub fn data_dir() -> PathBuf {
    resolve_data_dir(Platform::current(), |key| std::env::var_os(key))
}

/// Full path of the settings file.
pub fn settings_path() -> PathBuf {
    data_dir().join(SETTINGS_FILE)
}

/// Full path of the high-score file.
pub fn scores_path() -> PathBuf {
    data_dir().join(SCORES_FILE)
}

pub fn load_settings() -> Settings {
    load_settings_from(&settings_path())
}

pub fn save_settings(settings: &Settings) -> std::io::Result<()> {
    save_settings_to(&settings_path(), settings)
}

/// Missing file -> `HighScores::defaults()`.
pub fn load_scores() -> HighScores {
    load_scores_from(&scores_path())
}

pub fn save_scores(scores: &HighScores) -> std::io::Result<()> {
    save_scores_to(&scores_path(), scores)
}

/// Load settings from an explicit path; any error yields the defaults.
pub(crate) fn load_settings_from(path: &Path) -> Settings {
    read_text(path)
        .map(|t| Settings::from_text(&t))
        .unwrap_or_default()
}

/// Atomically write settings to an explicit path, creating its directory.
pub(crate) fn save_settings_to(path: &Path, settings: &Settings) -> io::Result<()> {
    write_atomic(path, &settings.to_text())
}

/// Load high scores from an explicit path. A missing, unreadable or
/// entirely invalid file yields [`HighScores::defaults`]; a partially
/// corrupt file keeps its valid lines.
pub(crate) fn load_scores_from(path: &Path) -> HighScores {
    match read_text(path) {
        Ok(text) => {
            let scores = HighScores::from_text(&text);
            if scores.entries.is_empty() {
                HighScores::defaults()
            } else {
                scores
            }
        }
        Err(_) => HighScores::defaults(),
    }
}

/// Atomically write high scores to an explicit path, creating its directory.
pub(crate) fn save_scores_to(path: &Path, scores: &HighScores) -> io::Result<()> {
    write_atomic(path, &scores.to_text())
}

/// Read at most [`MAX_FILE_BYTES`] of a file as text (invalid UTF-8 is
/// replaced, so the lenient parsers simply skip the damaged lines).
fn read_text(path: &Path) -> io::Result<String> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_FILE_BYTES)
        .read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Write `contents` to `path` atomically: the data goes to a uniquely named
/// temporary file in the same directory, is flushed to disk, and is then
/// renamed over the destination, so readers only ever see the old or the new
/// file, never a torn one. The directory is created if needed.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)?;

    let mut tmp_name = OsString::from(".");
    tmp_name.push(file_name);
    tmp_name.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = dir.join(tmp_name);

    let result = write_then_rename(&tmp, path, contents.as_bytes());
    if result.is_err() {
        // Best effort: never leave stray temp files behind.
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_then_rename(tmp: &Path, dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn entry(name: &str, score: u32, level: u32) -> HighScore {
        HighScore::new(name, score, level)
    }

    // -- Settings -----------------------------------------------------------

    #[test]
    fn settings_roundtrip() {
        let s = Settings {
            master_volume: 3,
            music_volume: 0,
            sfx_volume: 10,
            fullscreen: true,
            screen_shake: false,
            scanlines: true,
            show_fps: true,
            start_lives: 5,
            extra_life_at: 0,
        };
        assert_eq!(Settings::from_text(&s.to_text()), s);
        let d = Settings::default();
        assert_eq!(Settings::from_text(&d.to_text()), d);
    }

    #[test]
    fn default_settings() {
        let d = Settings::default();
        assert_eq!(d.extra_life_at, 15_000);
        assert_eq!(d.start_lives, 3);
        // With the dB volume curve (see `audio::set_volumes`) these put
        // effects at about -4 dB and music at about -9 dB.
        assert_eq!((d.master_volume, d.music_volume, d.sfx_volume), (10, 8, 9));
        assert_eq!(d.clamped(), d, "defaults are within range");
        assert!(d.to_text().contains("extra_life_at=15000\n"));
    }

    #[test]
    fn settings_empty_or_garbage_yields_defaults() {
        assert_eq!(Settings::from_text(""), Settings::default());
        assert_eq!(
            Settings::from_text("\u{0}\u{1}garbage\n===\n=5\nmaster_volume\n\u{fffd}\u{fffd}"),
            Settings::default()
        );
    }

    #[test]
    fn settings_ignore_unknown_keys_comments_and_bad_values() {
        let text = "\u{feff}# comment line\n\
                    \n\
                    unknown_key=42\n\
                    # master_volume=1\n\
                    master_volume=five\n\
                    music_volume = 4   # inline comment\n\
                    FULLSCREEN=Yes\n\
                    screen_shake=off\n\
                    scanlines=maybe\n\
                    show_fps=1\n";
        let s = Settings::from_text(text);
        let d = Settings::default();
        assert_eq!(s.master_volume, d.master_volume);
        assert_eq!(s.music_volume, 4);
        assert!(s.fullscreen);
        assert!(!s.screen_shake);
        assert_eq!(s.scanlines, d.scanlines);
        assert!(s.show_fps);
    }

    #[test]
    fn settings_clamp_ranges() {
        let s = Settings::from_text(
            "master_volume=99\nmusic_volume=-3\nsfx_volume=300000000000\n\
             start_lives=0\nextra_life_at=1\n",
        );
        assert_eq!(s.master_volume, 10);
        assert_eq!(s.music_volume, 0);
        assert_eq!(s.sfx_volume, 10);
        assert_eq!(s.start_lives, 1);
        assert_eq!(s.extra_life_at, 5_000);

        let s = Settings::from_text("start_lives=9\nextra_life_at=250000\n");
        assert_eq!(s.start_lives, 5);
        assert_eq!(s.extra_life_at, 100_000);

        for (v, want) in [("0", 0), ("-10", 0), ("4999", 5_000), ("20000", 20_000)] {
            let s = Settings::from_text(&format!("extra_life_at={v}"));
            assert_eq!(s.extra_life_at, want, "extra_life_at={v}");
        }

        // Out-of-range in-memory values are clamped on output too.
        let wild = Settings {
            master_volume: 200,
            start_lives: 0,
            extra_life_at: 7,
            ..Settings::default()
        };
        let back = Settings::from_text(&wild.to_text());
        assert_eq!(back, wild.clamped());
        assert_eq!(
            (back.master_volume, back.start_lives, back.extra_life_at),
            (10, 1, 5_000)
        );
    }

    // -- Names --------------------------------------------------------------

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_name("abc"), "ABC");
        assert_eq!(sanitize_name("a"), "A  ");
        assert_eq!(sanitize_name(""), "   ");
        assert_eq!(sanitize_name("ABCDEF"), "ABC");
        assert_eq!(sanitize_name("a_é"), "A  ");
        assert_eq!(sanitize_name("?!."), "?!.");
        for c in NAME_CHARSET.chars() {
            assert!(is_name_char(c), "{c:?}");
        }
        assert_eq!(NAME_CHARSET.chars().count(), 26 + 10 + 5);
    }

    // -- High scores --------------------------------------------------------

    #[test]
    fn defaults_are_full_sorted_and_valid() {
        let d = HighScores::defaults();
        assert_eq!(d.entries.len(), HighScores::MAX);
        assert_eq!(d.best(), 30_000);
        assert_eq!(d.entries.last().map(|e| e.score), Some(3_000));
        assert!(d.entries.windows(2).all(|w| w[0].score > w[1].score));
        for e in &d.entries {
            assert_eq!(e.name.chars().count(), NAME_LEN);
            assert!(e.name.chars().all(is_name_char));
            assert!(e.level >= 1);
        }
    }

    #[test]
    fn scores_roundtrip_including_spaces_in_names() {
        let mut t = HighScores::defaults();
        assert_eq!(t.insert(entry("A B", 25_000, 5)), Some(2));
        assert_eq!(t.insert(entry("  Z", 6_500, 1)), Some(9));
        let text = t.to_text();
        assert!(text.contains("A_B 25000 5\n"));
        assert!(text.contains("__Z 6500 1\n"));
        assert_eq!(HighScores::from_text(&text), t);
        let d = HighScores::defaults();
        assert_eq!(HighScores::from_text(&d.to_text()), d);
    }

    #[test]
    fn scores_parser_skips_garbage_and_sorts() {
        let text = "# header\n\
                    \n\
                    LOW 100 1\n\
                    toolong 500 2\n\
                    AB 600 2\n\
                    A@B 700 2\n\
                    HI! 9000 4\n\
                    BAD -5 1\n\
                    BAD 5 x\n\
                    EXT 5 1 extra\n\
                    abc 300 0\n\
                    \u{0}\u{1}\u{2}\n\
                    TIE 100 3\n";
        let t = HighScores::from_text(text);
        let got: Vec<(&str, u32, u32)> = t
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.score, e.level))
            .collect();
        assert_eq!(
            got,
            vec![
                ("HI!", 9000, 4),
                ("ABC", 300, 1),
                ("LOW", 100, 1),
                ("TIE", 100, 3)
            ]
        );
        assert!(HighScores::from_text("").entries.is_empty());
        assert!(HighScores::from_text("\u{fffd}\u{fffd}\n%%%%")
            .entries
            .is_empty());
    }

    #[test]
    fn scores_parser_truncates_to_max() {
        let text: String = (0..25)
            .map(|i| format!("P{:02} {} 1\n", i, i * 10))
            .collect();
        let t = HighScores::from_text(&text);
        assert_eq!(t.entries.len(), HighScores::MAX);
        assert_eq!(t.best(), 240);
        assert_eq!(t.entries.last().map(|e| e.score), Some(150));
    }

    #[test]
    fn qualifies_edge_cases() {
        let empty = HighScores {
            entries: Vec::new(),
        };
        assert!(!empty.qualifies(0));
        assert!(empty.qualifies(1));

        let partial = HighScores {
            entries: vec![entry("AAA", 500, 1)],
        };
        assert!(
            partial.qualifies(1),
            "room left: any positive score qualifies"
        );
        assert!(!partial.qualifies(0));

        let full = HighScores::defaults();
        assert!(!full.qualifies(0));
        assert!(!full.qualifies(2_999));
        assert!(
            !full.qualifies(3_000),
            "a tie with the 10th place does not enter"
        );
        assert!(full.qualifies(3_001));
        assert!(full.qualifies(u32::MAX));
    }

    #[test]
    fn insert_is_stable_and_reports_rank() {
        let mut t = HighScores {
            entries: Vec::new(),
        };
        assert_eq!(t.insert(entry("ONE", 100, 1)), Some(0));
        assert_eq!(
            t.insert(entry("TWO", 100, 1)),
            Some(1),
            "ties go after existing"
        );
        assert_eq!(t.insert(entry("TOP", 200, 2)), Some(0));
        assert_eq!(t.insert(entry("THR", 100, 1)), Some(3));
        let names: Vec<&str> = t.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["TOP", "ONE", "TWO", "THR"]);

        // Names are normalised on insert.
        assert_eq!(t.insert(entry("x", 50, 1)), Some(4));
        assert_eq!(t.entries[4].name, "X  ");
    }

    #[test]
    fn insert_into_full_table() {
        let mut t = HighScores::defaults();
        // Equal to the last entry: would land at index 10 -> dropped.
        assert_eq!(t.insert(entry("NEW", 3_000, 1)), None);
        assert_eq!(t, HighScores::defaults());
        // Ties with a middle entry go after it.
        assert_eq!(t.insert(entry("MID", 18_000, 4)), Some(5));
        assert_eq!(t.entries.len(), HighScores::MAX);
        assert_eq!(t.entries[4].name, "ORB");
        assert_eq!(t.entries[5].name, "MID");
        assert_eq!(t.entries.last().map(|e| e.name.as_str()), Some("KAI"));
        // A new record goes to the top.
        assert_eq!(t.insert(entry("WOW", 99_999, 9)), Some(0));
        assert_eq!(t.best(), 99_999);
        assert_eq!(t.entries.len(), HighScores::MAX);
    }

    #[test]
    fn insert_normalizes_an_unsorted_oversized_table() {
        let mut t = HighScores {
            entries: (0..12).map(|i| entry("ZZZ", i * 10, 1)).collect(),
        };
        assert_eq!(t.insert(entry("NEW", 55, 1)), Some(6));
        assert_eq!(t.entries.len(), HighScores::MAX);
        assert!(t.entries.windows(2).all(|w| w[0].score >= w[1].score));
    }

    // -- Data directory -----------------------------------------------------

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn data_dir_per_platform() {
        let home = PathBuf::from("/home/u");
        let mac = resolve_data_dir(Platform::MacOs, env_of(&[("HOME", "/home/u")]));
        assert_eq!(
            mac,
            home.join("Library")
                .join("Application Support")
                .join("Glimmer")
        );

        let win = resolve_data_dir(Platform::Windows, env_of(&[("APPDATA", "/appdata")]));
        assert_eq!(win, PathBuf::from("/appdata").join("Glimmer"));

        let xdg = resolve_data_dir(
            Platform::OtherUnix,
            env_of(&[("XDG_DATA_HOME", "/xdg"), ("HOME", "/home/u")]),
        );
        assert_eq!(xdg, PathBuf::from("/xdg").join("glimmer"));

        let unix = resolve_data_dir(Platform::OtherUnix, env_of(&[("HOME", "/home/u")]));
        assert_eq!(unix, home.join(".local").join("share").join("glimmer"));

        // Relative or empty XDG_DATA_HOME is ignored.
        for bad in ["relative/dir", ""] {
            let p = resolve_data_dir(
                Platform::OtherUnix,
                env_of(&[("XDG_DATA_HOME", bad), ("HOME", "/home/u")]),
            );
            assert_eq!(p, home.join(".local").join("share").join("glimmer"));
        }
    }

    #[test]
    fn data_dir_falls_back_when_env_is_missing() {
        let fallback = Path::new(".").join("glimmer-data");
        for platform in [Platform::MacOs, Platform::Windows, Platform::OtherUnix] {
            assert_eq!(resolve_data_dir(platform, env_of(&[])), fallback);
            assert_eq!(
                resolve_data_dir(platform, env_of(&[("HOME", "")])),
                fallback
            );
        }
        // The real lookup never panics and names the game's folder.
        let real = data_dir();
        let last = real
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase());
        assert!(matches!(
            last.as_deref(),
            Some("glimmer") | Some("glimmer-data")
        ));
    }

    // -- Filesystem ---------------------------------------------------------

    /// A unique scratch directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static N: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir().join(format!(
                "glimmer-test-{}-{}-{}-{}",
                tag,
                std::process::id(),
                nanos,
                N.fetch_add(1, Ordering::Relaxed)
            ));
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn file_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("readable dir")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn filesystem_roundtrip() {
        let tmp = TempDir::new("roundtrip");
        // Nested directories that do not exist yet are created.
        let dir = tmp.0.join("nested").join("data");
        let settings_file = dir.join(SETTINGS_FILE);
        let scores_file = dir.join(SCORES_FILE);

        // Missing files yield defaults.
        assert_eq!(load_settings_from(&settings_file), Settings::default());
        assert_eq!(load_scores_from(&scores_file), HighScores::defaults());

        let settings = Settings {
            music_volume: 2,
            show_fps: true,
            ..Settings::default()
        };
        save_settings_to(&settings_file, &settings).expect("save settings");
        assert_eq!(load_settings_from(&settings_file), settings);

        let mut scores = HighScores::defaults();
        scores.insert(entry("Q Q", 77_777, 8));
        save_scores_to(&scores_file, &scores).expect("save scores");
        assert_eq!(load_scores_from(&scores_file), scores);

        // Overwriting an existing file works and leaves no temp files behind.
        let louder = Settings {
            music_volume: 9,
            ..settings
        };
        save_settings_to(&settings_file, &louder).expect("overwrite settings");
        assert_eq!(load_settings_from(&settings_file), louder);
        assert_eq!(
            file_names(&dir),
            vec![SCORES_FILE.to_string(), SETTINGS_FILE.to_string()]
        );
    }

    #[test]
    fn corrupt_files_yield_defaults() {
        let tmp = TempDir::new("corrupt");
        fs::create_dir_all(&tmp.0).expect("create temp dir");
        let settings_file = tmp.0.join(SETTINGS_FILE);
        let scores_file = tmp.0.join(SCORES_FILE);
        let garbage: Vec<u8> = (0..4096u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        fs::write(&settings_file, &garbage).expect("write garbage");
        fs::write(&scores_file, &garbage).expect("write garbage");
        assert_eq!(load_settings_from(&settings_file), Settings::default());
        assert_eq!(load_scores_from(&scores_file), HighScores::defaults());

        // A directory where a file is expected is an error, not a panic.
        let as_dir = tmp.0.join("dir-not-file");
        fs::create_dir_all(&as_dir).expect("create dir");
        assert_eq!(load_settings_from(&as_dir), Settings::default());
        assert_eq!(load_scores_from(&as_dir), HighScores::defaults());
        assert!(save_settings_to(&as_dir, &Settings::default()).is_err());
        // ...and the failed save cleaned up its temp file.
        assert_eq!(
            file_names(&tmp.0),
            vec!["dir-not-file", SCORES_FILE, SETTINGS_FILE]
        );

        // A partially damaged score file keeps its valid lines.
        fs::write(&scores_file, b"ACE 4200 2\n\xff\xfe garbage\nBOB 100 1\n").expect("write");
        let partial = load_scores_from(&scores_file);
        assert_eq!(
            partial.entries,
            vec![entry("ACE", 4200, 2), entry("BOB", 100, 1)]
        );
    }

    #[test]
    fn write_atomic_rejects_paths_without_file_name() {
        assert!(write_atomic(Path::new("/"), "x").is_err());
    }
}
