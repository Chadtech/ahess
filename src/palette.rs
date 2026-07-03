#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Color(u32);

impl Color {
    pub const fn rgb(self) -> u32 {
        self.0
    }

    pub fn css(self) -> String {
        format!("#{:06x}", self.0)
    }
}

pub const GREEN1: Color = Color(0x030907);
pub const GREEN2: Color = Color(0x071d10);
pub const GREEN3: Color = Color(0x082208);
pub const GREEN4: Color = Color(0x142909);
pub const GREEN5: Color = Color(0x30371a);
pub const GREEN6: Color = Color(0x366317);
pub const GREEN7: Color = Color(0x0aca1a);

pub const GRAY1: Color = Color(0x131610);
pub const GRAY2: Color = Color(0x2c2826);
pub const GRAY3: Color = Color(0x57524f);
pub const GRAY4: Color = Color(0x807672);
pub const GRAY5: Color = Color(0xb0a69a);
pub const GRAY6: Color = Color(0xe0d6ca);

pub const YELLOW1: Color = Color(0x161303);
pub const YELLOW2: Color = Color(0x302507);
pub const YELLOW3: Color = Color(0x5a4f0e);
pub const YELLOW4: Color = Color(0x837339);
pub const YELLOW5: Color = Color(0xb39f4b);
pub const YELLOW6: Color = Color(0xe3d34b);

pub const BLUE1: Color = Color(0x175cfe);
pub const BLUE2: Color = Color(0x0abab5);

pub const RED1: Color = Color(0x651a20);
pub const RED2: Color = Color(0xf21d23);

pub const WHITE: Color = Color(0xffffff);

pub const CSS_COLORS: &[(&str, Color)] = &[
    ("green-1", GREEN1),
    ("green-2", GREEN2),
    ("green-3", GREEN3),
    ("green-4", GREEN4),
    ("green-5", GREEN5),
    ("green-6", GREEN6),
    ("green-7", GREEN7),
    ("gray-1", GRAY1),
    ("gray-2", GRAY2),
    ("gray-3", GRAY3),
    ("gray-4", GRAY4),
    ("gray-5", GRAY5),
    ("gray-6", GRAY6),
    ("yellow-1", YELLOW1),
    ("yellow-2", YELLOW2),
    ("yellow-3", YELLOW3),
    ("yellow-4", YELLOW4),
    ("yellow-5", YELLOW5),
    ("yellow-6", YELLOW6),
    ("blue-1", BLUE1),
    ("blue-2", BLUE2),
    ("red-1", RED1),
    ("red-2", RED2),
    ("white", WHITE),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_css_hex_colors() {
        assert_eq!(GREEN1.css(), "#030907");
        assert_eq!(WHITE.css(), "#ffffff");
    }
}
