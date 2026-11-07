use std::fmt;
use std::str::FromStr;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

use crate::ThemeError;

/// An sRGB colour with straight (non-premultiplied) alpha.
///
/// Serialised as `"#rrggbb"` when opaque and `"#rrggbbaa"` otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self {
            r,
            g,
            b,
            a: u8::MAX,
        }
    }

    /// Parse `#RRGGBB` or `#RRGGBBAA` (case-insensitive, `#` required).
    pub fn parse(text: &str) -> Result<Self, ThemeError> {
        let invalid = || ThemeError::InvalidColor {
            value: text.to_owned(),
        };
        let hex = text.strip_prefix('#').ok_or_else(invalid)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let channel = |index: usize| u8::from_str_radix(&hex[index..index + 2], 16);
        let parse = || -> Result<Self, std::num::ParseIntError> {
            Ok(Self {
                r: channel(0)?,
                g: channel(2)?,
                b: channel(4)?,
                a: if hex.len() == 8 { channel(6)? } else { u8::MAX },
            })
        };
        parse().map_err(|_| invalid())
    }

    /// Lowercase hex; the alpha pair is emitted only when not fully opaque.
    pub fn to_hex(self) -> String {
        if self.a == u8::MAX {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
        }
    }

    /// Packed `0xRRGGBBAA`, the layout GPUI's `rgba()` helper expects.
    pub fn to_u32(self) -> u32 {
        u32::from_be_bytes([self.r, self.g, self.b, self.a])
    }

    /// WCAG 2.x relative luminance. Alpha is ignored (treated as opaque).
    pub fn relative_luminance(self) -> f64 {
        fn linear(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }
}

/// WCAG contrast ratio in `1.0..=21.0`. Alpha is ignored.
pub fn contrast_ratio(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (a.relative_luminance(), b.relative_luminance());
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl FromStr for Rgba {
    type Err = ThemeError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct HexVisitor;
        impl Visitor<'_> for HexVisitor {
            type Value = Rgba;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a colour string \"#RRGGBB\" or \"#RRGGBBAA\"")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Rgba, E> {
                Rgba::parse(value).map_err(E::custom)
            }
        }
        deserializer.deserialize_str(HexVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_hex() {
        let color = Rgba::parse("#1ABC9B").unwrap();
        assert_eq!(color, Rgba::rgb(0x1a, 0xbc, 0x9b));
        assert_eq!(color.to_hex(), "#1abc9b");
        assert_eq!(color.to_u32(), 0x1abc9bff);
        let translucent = Rgba::parse("#00ff8980").unwrap();
        assert_eq!(translucent.a, 0x80);
        assert_eq!(translucent.to_hex(), "#00ff8980");
        assert_eq!(translucent.to_u32(), 0x00ff8980);
    }

    #[test]
    fn rejects_bad_hex() {
        for bad in [
            "1abc9b",
            "#1abc9",
            "#1abc9bf",
            "#gggggg",
            "",
            "#",
            "#1abc9b0000",
            "#+1abc9b",
        ] {
            assert!(Rgba::parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn contrast_extremes() {
        let black = Rgba::rgb(0, 0, 0);
        let white = Rgba::rgb(255, 255, 255);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        assert_eq!(contrast_ratio(black, white), contrast_ratio(white, black));
    }
}
