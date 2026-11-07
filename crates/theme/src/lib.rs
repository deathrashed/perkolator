//! Versioned Perkolator theme schema and semantic tokens.
//!
//! This is Perkolator's own schema (`schemaVersion` 1). It is inspired by the
//! restrained FluxDown/Breeze look but makes no claim of compatibility with
//! any third-party theme format or schema URI.
//!
//! Colour policy encoded in the built-ins and checked by [`ThemeFile::validate`]:
//! `primary` is a rare accent, `focus` is reserved for keyboard focus, and
//! `selection` is a neutral (or lightly tinted) fill that is never either of them.

mod color;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use color::{Rgba, contrast_ratio};

/// The only schema version this build reads and writes.
pub const SCHEMA_VERSION: u32 = 1;

/// WCAG AA for body text.
pub const MIN_TEXT_CONTRAST: f64 = 4.5;
/// Secondary/tertiary labels (WCAG AA large-text / UI component threshold).
pub const MIN_SECONDARY_CONTRAST: f64 = 3.0;

const DENSITY_RANGE: (f32, f32) = (16.0, 96.0);
const RADIUS_RANGE: (f32, f32) = (0.0, 24.0);
const FONT_SIZE_RANGE: (f32, f32) = (8.0, 32.0);

const BUILTIN_DARK_JSON: &str = include_str!("../themes/perkolator-dark.json");
const BUILTIN_LIGHT_JSON: &str = include_str!("../themes/perkolator-light.json");

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("theme is not valid JSON or is missing a token: {0}")]
    Parse(String),
    #[error("theme is missing `schemaVersion` (this build reads version {SCHEMA_VERSION})")]
    MissingSchemaVersion,
    #[error("unsupported theme schemaVersion {found}; this build reads version {SCHEMA_VERSION}")]
    UnsupportedSchemaVersion { found: u64 },
    #[error("invalid colour {value:?}: expected \"#RRGGBB\" or \"#RRGGBBAA\"")]
    InvalidColor { value: String },
    #[error("theme `{field}` must not be empty")]
    Empty { field: &'static str },
    #[error("theme `{field}` = {value} is outside the allowed range {min}..={max}")]
    OutOfRange {
        field: &'static str,
        value: f32,
        min: f32,
        max: f32,
    },
    #[error(
        "contrast of `{foreground}` on `{background}` is {ratio:.2}:1; at least {min}:1 is required"
    )]
    LowContrast {
        foreground: &'static str,
        background: &'static str,
        ratio: f64,
        min: f64,
    },
    #[error("`selection` must be a neutral fill, not the `{token}` colour")]
    SelectionUsesAccent { token: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    Dark,
    Light,
}

/// Semantic colour tokens. UI code refers to roles, never raw hex values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColorTokens {
    /// Window/content background.
    pub background: Rgba,
    /// Title bar, sidebars, status bar.
    pub chrome: Rgba,
    /// Panels and inputs.
    pub surface: Rgba,
    /// Popovers, menus, raised controls.
    pub raised: Rgba,
    pub foreground: Rgba,
    pub muted_foreground: Rgba,
    pub tertiary_foreground: Rgba,
    pub border: Rgba,
    pub hairline: Rgba,
    /// Sparse accent for primary operations; never a default fill.
    pub primary: Rgba,
    /// Keyboard focus ring only.
    pub focus: Rgba,
    pub info: Rgba,
    pub warning: Rgba,
    pub destructive: Rgba,
    /// Selected row fill; neutral, never `primary`/`focus`.
    pub selection: Rgba,
    pub selection_foreground: Rgba,
    pub hover: Rgba,
}

/// Fixed control heights in logical pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Density {
    pub title_bar: f32,
    pub nav_row: f32,
    pub control: f32,
    pub status_bar: f32,
    pub result_row_compact: f32,
    pub result_row_comfortable: f32,
}

/// Corner radii in logical pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Radius {
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

/// Font sizes in logical pixels; font families fall back to the platform UI
/// and monospace fonts when unset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typography {
    pub body: f32,
    pub caption: f32,
    pub heading: f32,
    pub mono: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono_font: Option<String>,
}

/// A complete theme document. Every colour/size token is required.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeFile {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub mode: ThemeMode,
    pub colors: ColorTokens,
    pub density: Density,
    pub radius: Radius,
    pub typography: Typography,
}

impl ThemeFile {
    /// Parse and validate a theme. The schema version is checked before the
    /// tokens so a newer file yields a version error, not a missing-token one.
    pub fn from_json(text: &str) -> Result<Self, ThemeError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|error| ThemeError::Parse(error.to_string()))?;
        let version = value
            .get("schemaVersion")
            .ok_or(ThemeError::MissingSchemaVersion)?;
        match version.as_u64() {
            Some(found) if found == u64::from(SCHEMA_VERSION) => {}
            Some(found) => return Err(ThemeError::UnsupportedSchemaVersion { found }),
            None => {
                return Err(ThemeError::Parse(format!(
                    "`schemaVersion` must be a non-negative integer, found {version}"
                )));
            }
        }
        let theme: ThemeFile =
            serde_json::from_value(value).map_err(|error| ThemeError::Parse(error.to_string()))?;
        theme.validate()?;
        Ok(theme)
    }

    pub fn to_json_pretty(&self) -> String {
        let mut text = serde_json::to_string_pretty(self)
            .expect("theme serialisation is infallible: no maps with non-string keys");
        text.push('\n');
        text
    }

    pub fn builtin_dark() -> Self {
        Self::from_json(BUILTIN_DARK_JSON).expect("built-in dark theme is valid")
    }

    pub fn builtin_light() -> Self {
        Self::from_json(BUILTIN_LIGHT_JSON).expect("built-in light theme is valid")
    }

    pub fn builtin(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::builtin_dark(),
            ThemeMode::Light => Self::builtin_light(),
        }
    }

    pub fn validate(&self) -> Result<(), ThemeError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ThemeError::UnsupportedSchemaVersion {
                found: u64::from(self.schema_version),
            });
        }
        if self.id.trim().is_empty() {
            return Err(ThemeError::Empty { field: "id" });
        }
        if self.name.trim().is_empty() {
            return Err(ThemeError::Empty { field: "name" });
        }
        self.validate_sizes()?;
        self.validate_colors()
    }

    fn validate_sizes(&self) -> Result<(), ThemeError> {
        let d = &self.density;
        let r = &self.radius;
        let t = &self.typography;
        let checks: [(&'static str, f32, (f32, f32)); 14] = [
            ("density.titleBar", d.title_bar, DENSITY_RANGE),
            ("density.navRow", d.nav_row, DENSITY_RANGE),
            ("density.control", d.control, DENSITY_RANGE),
            ("density.statusBar", d.status_bar, DENSITY_RANGE),
            (
                "density.resultRowCompact",
                d.result_row_compact,
                DENSITY_RANGE,
            ),
            (
                "density.resultRowComfortable",
                d.result_row_comfortable,
                DENSITY_RANGE,
            ),
            ("radius.sm", r.sm, RADIUS_RANGE),
            ("radius.md", r.md, RADIUS_RANGE),
            ("radius.lg", r.lg, RADIUS_RANGE),
            ("radius.xl", r.xl, RADIUS_RANGE),
            ("typography.body", t.body, FONT_SIZE_RANGE),
            ("typography.caption", t.caption, FONT_SIZE_RANGE),
            ("typography.heading", t.heading, FONT_SIZE_RANGE),
            ("typography.mono", t.mono, FONT_SIZE_RANGE),
        ];
        for (field, value, (min, max)) in checks {
            // `!(a <= v && v <= b)` also rejects NaN.
            if !(min <= value && value <= max) {
                return Err(ThemeError::OutOfRange {
                    field,
                    value,
                    min,
                    max,
                });
            }
        }
        Ok(())
    }

    fn validate_colors(&self) -> Result<(), ThemeError> {
        let c = &self.colors;
        if c.selection == c.primary {
            return Err(ThemeError::SelectionUsesAccent { token: "primary" });
        }
        if c.selection == c.focus {
            return Err(ThemeError::SelectionUsesAccent { token: "focus" });
        }
        let pairs: [(&'static str, Rgba, &'static str, Rgba, f64); 6] = [
            (
                "foreground",
                c.foreground,
                "background",
                c.background,
                MIN_TEXT_CONTRAST,
            ),
            (
                "foreground",
                c.foreground,
                "surface",
                c.surface,
                MIN_TEXT_CONTRAST,
            ),
            (
                "foreground",
                c.foreground,
                "chrome",
                c.chrome,
                MIN_TEXT_CONTRAST,
            ),
            (
                "selectionForeground",
                c.selection_foreground,
                "selection",
                c.selection,
                MIN_TEXT_CONTRAST,
            ),
            (
                "mutedForeground",
                c.muted_foreground,
                "background",
                c.background,
                MIN_TEXT_CONTRAST,
            ),
            (
                "tertiaryForeground",
                c.tertiary_foreground,
                "background",
                c.background,
                MIN_SECONDARY_CONTRAST,
            ),
        ];
        for (foreground, fg, background, bg, min) in pairs {
            let ratio = contrast_ratio(fg, bg);
            if ratio < min {
                return Err(ThemeError::LowContrast {
                    foreground,
                    background,
                    ratio,
                    min,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dark_value() -> serde_json::Value {
        serde_json::from_str(BUILTIN_DARK_JSON).unwrap()
    }

    #[test]
    fn builtins_parse_and_validate() {
        for theme in [ThemeFile::builtin_dark(), ThemeFile::builtin_light()] {
            theme.validate().unwrap();
            assert_eq!(theme.schema_version, SCHEMA_VERSION);
        }
        assert_eq!(ThemeFile::builtin_dark().mode, ThemeMode::Dark);
        assert_eq!(ThemeFile::builtin_light().mode, ThemeMode::Light);
        assert_eq!(ThemeFile::builtin(ThemeMode::Light).id, "perkolator-light");
    }

    #[test]
    fn dark_matches_design_system() {
        let theme = ThemeFile::builtin_dark();
        let c = &theme.colors;
        assert_eq!(c.background.to_hex(), "#1e1e1e");
        assert_eq!(c.primary.to_hex(), "#1abc9b");
        assert_eq!(c.focus.to_hex(), "#00ff89");
        assert_eq!(c.selection.to_hex(), "#343436");
        assert_eq!(theme.density.title_bar, 40.0);
        assert_eq!(theme.density.result_row_comfortable, 42.0);
        assert_eq!(theme.radius.xl, 12.0);
        assert_eq!(theme.typography.body, 13.0);
    }

    #[test]
    fn selection_is_never_an_accent() {
        for theme in [ThemeFile::builtin_dark(), ThemeFile::builtin_light()] {
            assert_ne!(theme.colors.selection, theme.colors.primary);
            assert_ne!(theme.colors.selection, theme.colors.focus);
        }
        let mut theme = ThemeFile::builtin_dark();
        theme.colors.selection = theme.colors.primary;
        assert!(matches!(
            theme.validate(),
            Err(ThemeError::SelectionUsesAccent { token: "primary" })
        ));
    }

    #[test]
    fn json_round_trip_is_lossless() {
        for theme in [ThemeFile::builtin_dark(), ThemeFile::builtin_light()] {
            let text = theme.to_json_pretty();
            assert!(text.contains("\"schemaVersion\": 1"));
            assert!(text.contains("\"mutedForeground\""));
            assert!(text.contains("\"resultRowCompact\""));
            assert_eq!(ThemeFile::from_json(&text).unwrap(), theme);
        }
    }

    #[test]
    fn optional_fonts_round_trip() {
        let mut theme = ThemeFile::builtin_dark();
        theme.typography.ui_font = Some("Inter".into());
        let parsed = ThemeFile::from_json(&theme.to_json_pretty()).unwrap();
        assert_eq!(parsed.typography.ui_font.as_deref(), Some("Inter"));
        assert_eq!(parsed.typography.mono_font, None);
    }

    #[test]
    fn wrong_schema_version_is_rejected_clearly() {
        let mut value = dark_value();
        value["schemaVersion"] = 2.into();
        let error = ThemeFile::from_json(&value.to_string()).unwrap_err();
        assert!(matches!(
            error,
            ThemeError::UnsupportedSchemaVersion { found: 2 }
        ));
        assert!(error.to_string().contains("this build reads version 1"));

        value.as_object_mut().unwrap().remove("schemaVersion");
        assert!(matches!(
            ThemeFile::from_json(&value.to_string()),
            Err(ThemeError::MissingSchemaVersion)
        ));
    }

    #[test]
    fn missing_token_is_rejected() {
        let mut value = dark_value();
        value["colors"].as_object_mut().unwrap().remove("hairline");
        let error = ThemeFile::from_json(&value.to_string()).unwrap_err();
        assert!(matches!(error, ThemeError::Parse(_)));
        assert!(error.to_string().contains("hairline"), "{error}");
    }

    #[test]
    fn bad_hex_in_file_is_rejected() {
        let mut value = dark_value();
        value["colors"]["border"] = "48484a".into();
        let error = ThemeFile::from_json(&value.to_string()).unwrap_err();
        assert!(error.to_string().contains("48484a"), "{error}");
    }

    #[test]
    fn low_contrast_is_rejected() {
        let mut theme = ThemeFile::builtin_dark();
        theme.colors.foreground = Rgba::parse("#3a3a3a").unwrap();
        let error = theme.validate().unwrap_err();
        assert!(matches!(
            error,
            ThemeError::LowContrast {
                foreground: "foreground",
                background: "background",
                ..
            }
        ));
        let dark = ThemeFile::builtin_dark();
        assert!(contrast_ratio(dark.colors.foreground, dark.colors.background) >= 4.5);
        let light = ThemeFile::builtin_light();
        assert!(contrast_ratio(light.colors.foreground, light.colors.background) >= 4.5);
    }

    #[test]
    fn out_of_range_sizes_are_rejected() {
        let mut theme = ThemeFile::builtin_dark();
        theme.typography.body = 64.0;
        assert!(matches!(
            theme.validate(),
            Err(ThemeError::OutOfRange {
                field: "typography.body",
                ..
            })
        ));
        let mut theme = ThemeFile::builtin_dark();
        theme.density.control = f32::NAN;
        assert!(theme.validate().is_err());
    }

    #[test]
    fn empty_id_is_rejected() {
        let mut theme = ThemeFile::builtin_dark();
        theme.id = "  ".into();
        assert!(matches!(
            theme.validate(),
            Err(ThemeError::Empty { field: "id" })
        ));
    }
}
