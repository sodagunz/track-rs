//! Optional presentation settings; defaults never read the environment.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use ratatui::style::{Color, Style};
use serde::Deserialize;

use crate::state::Mode;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(try_from = "String")]
struct ThemeColor(Color);

impl TryFrom<String> for ThemeColor {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let color = match value.as_str() {
            "default" => Color::Reset,
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "gray" => Color::Gray,
            "dark-gray" => Color::DarkGray,
            "light-red" => Color::LightRed,
            "light-green" => Color::LightGreen,
            "light-yellow" => Color::LightYellow,
            "light-blue" => Color::LightBlue,
            "light-magenta" => Color::LightMagenta,
            "light-cyan" => Color::LightCyan,
            "white" => Color::White,
            _ => {
                if let Some(index) = value.strip_prefix("indexed:").and_then(|s| s.parse::<u8>().ok()) {
                    return Ok(Self(Color::Indexed(index)));
                }
                if value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
                    let rgb = u32::from_str_radix(&value[1..], 16).map_err(|e| e.to_string())?;
                    let bytes = rgb.to_be_bytes();
                    return Ok(Self(Color::Rgb(bytes[1], bytes[2], bytes[3])));
                }
                return Err(format!(
                    "invalid color {value:?}: use an ANSI name, default, indexed:0..255, or #RRGGBB"
                ));
            }
        };
        Ok(Self(color))
    }
}

/// Semantic colors shared by every view. Not part of a project or undo history.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    foreground: ThemeColor,
    background: ThemeColor,
    muted: ThemeColor,
    accent: ThemeColor,
    highlight_foreground: ThemeColor,
    cursor: ThemeColor,
    selection: ThemeColor,
    playback: ThemeColor,
    warning: ThemeColor,
    normal: ThemeColor,
    insert: ThemeColor,
    visual: ThemeColor,
    audition: ThemeColor,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            foreground: ThemeColor(Color::Reset),
            background: ThemeColor(Color::Reset),
            muted: ThemeColor(Color::DarkGray),
            accent: ThemeColor(Color::Cyan),
            highlight_foreground: ThemeColor(Color::Black),
            cursor: ThemeColor(Color::Green),
            selection: ThemeColor(Color::Blue),
            playback: ThemeColor(Color::DarkGray),
            warning: ThemeColor(Color::Red),
            normal: ThemeColor(Color::Blue),
            insert: ThemeColor(Color::Green),
            visual: ThemeColor(Color::Cyan),
            audition: ThemeColor(Color::Yellow),
        }
    }
}

impl Theme {
    /// Parse a partial theme, rejecting unknown roles and invalid colors.
    ///
    /// # Errors
    /// Requires a JSON object; JSON errors include line/column context.
    pub fn parse(json: &str) -> Result<Self> {
        if !json.trim_start().starts_with('{') {
            bail!("theme must be a JSON object");
        }
        Ok(serde_json::from_str(json)?)
    }

    pub(crate) fn base(&self) -> Style {
        Style::default().fg(self.foreground.0).bg(self.background.0)
    }
    pub(crate) const fn accent(&self) -> Color {
        self.accent.0
    }
    pub(crate) const fn muted(&self) -> Color {
        self.muted.0
    }
    pub(crate) const fn warning(&self) -> Color {
        self.warning.0
    }
    pub(crate) fn cursor(&self) -> Style {
        self.base().fg(self.highlight_foreground.0).bg(self.cursor.0)
    }
    pub(crate) fn selection(&self) -> Style {
        self.base().fg(self.highlight_foreground.0).bg(self.selection.0)
    }
    pub(crate) fn playback(&self) -> Style {
        self.base().bg(self.playback.0)
    }
    pub(crate) fn mode(&self, mode: Mode) -> Style {
        let background = match mode {
            Mode::Normal => self.normal,
            Mode::Insert => self.insert,
            Mode::Visual => self.visual,
            Mode::Audition => self.audition,
        };
        self.base().fg(self.highlight_foreground.0).bg(background.0)
    }
}

/// Resolve discovery without reading or modifying the process environment.
#[must_use]
pub fn discover(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    xdg.filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.filter(|s| !s.is_empty()).map(|s| PathBuf::from(s).join(".config")))
        .map(|base| base.join("track-rs/theme.json"))
}

/// Load an explicit path first, otherwise the discovered path. Only an absent
/// discovered file is ignored; malformed or unreadable files are errors.
///
/// # Errors
/// Reports the failing path and read/parse error.
pub fn load(explicit: Option<&Path>, discovered: Option<&Path>) -> Result<Theme> {
    let Some(path) = explicit.or(discovered) else {
        return Ok(Theme::default());
    };
    let json = match std::fs::read_to_string(path) {
        Ok(json) => json,
        Err(error) if explicit.is_none() && error.kind() == std::io::ErrorKind::NotFound => return Ok(Theme::default()),
        Err(error) => return Err(error).with_context(|| format!("read theme {}", path.display())),
    };
    Theme::parse(&json).with_context(|| format!("parse theme {}", path.display()))
}

/// Parse the sole startup option using OS strings, preserving non-UTF-8 paths.
///
/// # Errors
/// Rejects unknown, missing or repeated arguments.
pub fn theme_argument(args: impl IntoIterator<Item = OsString>) -> Result<Option<PathBuf>> {
    let mut args = args.into_iter();
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    if flag != "--theme" {
        bail!("unknown argument {}; usage: track-rs [--theme PATH]", flag.display());
    }
    let path = args
        .next()
        .context("--theme requires a path; usage: track-rs [--theme PATH]")?;
    if path.is_empty() || args.next().is_some() {
        bail!("usage: track-rs [--theme PATH]");
    }
    Ok(Some(path.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_defaults_and_colors() {
        let theme = Theme::parse(r##"{"accent":"#12abEF","background":"indexed:23"}"##).unwrap();
        assert_eq!(theme.accent(), Color::Rgb(18, 171, 239));
        assert_eq!(theme.base().bg, Some(Color::Indexed(23)));
        assert_eq!(theme.cursor().bg, Some(Color::Green));
        assert_eq!(
            Theme::default().base(),
            Style::default().fg(Color::Reset).bg(Color::Reset)
        );
        for value in ["default", "red", "light-cyan", "indexed:0", "indexed:255", "#000000"] {
            assert!(ThemeColor::try_from(value.to_owned()).is_ok());
        }
    }

    #[test]
    fn strict_invalid_configuration() {
        for json in [
            r#"{"typo":"red"}"#,
            r##"{"accent":"#123"}"##,
            r#"{"accent":"indexed:256"}"#,
            r#"{"accent":"RGB(1,2,3)"}"#,
            r#"{"accent":null}"#,
            r#"{"accent":12}"#,
            r#"{"accent":"red","accent":"blue"}"#,
            "[]",
            "{",
        ] {
            assert!(Theme::parse(json).is_err(), "{json}");
        }
    }

    #[test]
    fn discovery_priority_and_missing_paths() {
        assert_eq!(
            discover(Some("/xdg".into()), Some("/home".into())),
            Some("/xdg/track-rs/theme.json".into())
        );
        assert_eq!(
            discover(Some("".into()), Some("/home".into())),
            Some("/home/.config/track-rs/theme.json".into())
        );
        assert!(discover(None, None).is_none());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("主题.json");
        assert!(load(None, Some(&path)).is_ok());
        assert!(load(Some(&path), None).is_err());
        std::fs::write(&path, r#"{"accent":"magenta"}"#).unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, r#"{"accent":"invalid"}"#).unwrap();
        assert_eq!(load(Some(&path), Some(&bad)).unwrap().accent(), Color::Magenta);
        assert!(load(None, Some(&bad)).unwrap_err().to_string().contains("bad.json"));
        assert_eq!(
            theme_argument([OsString::from("--theme"), path.clone().into_os_string()]).unwrap(),
            Some(path)
        );
        for args in [vec!["--theme"], vec!["--bad"], vec!["--theme", "a", "b"]] {
            assert!(theme_argument(args.into_iter().map(OsString::from)).is_err());
        }
    }
}
