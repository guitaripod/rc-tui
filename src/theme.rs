use ratatui::style::Color;

pub const BG: Color = Color::Rgb(0x0b, 0x0d, 0x11);
pub const SURFACE: Color = Color::Rgb(0x11, 0x15, 0x1c);
pub const BORDER: Color = Color::Rgb(0x1c, 0x22, 0x2a);
pub const BORDER_STRONG: Color = Color::Rgb(0x2d, 0x33, 0x3b);
pub const TEXT: Color = Color::Rgb(0xc9, 0xd1, 0xd9);
pub const BRIGHT: Color = Color::Rgb(0xe8, 0xef, 0xf7);
pub const DIM: Color = Color::Rgb(0x7d, 0x86, 0x90);
pub const MONEY: Color = Color::Rgb(0x3f, 0xd0, 0x7f);
pub const ACQUISITION: Color = Color::Rgb(0x6a, 0xa9, 0xff);
pub const RETENTION: Color = Color::Rgb(0x39, 0xc5, 0xcf);
pub const TRIAL: Color = Color::Rgb(0xf0, 0x88, 0x3e);
pub const GOOD: Color = Color::Rgb(0x3f, 0xb9, 0x50);
pub const WARN: Color = Color::Rgb(0xf0, 0xc0, 0x60);
pub const BAD: Color = Color::Rgb(0xf8, 0x51, 0x49);
pub const SELECTION: Color = Color::Rgb(0x17, 0x25, 0x3f);

pub fn accent(id: &str) -> Color {
    let id = id.to_ascii_lowercase();
    if id.contains("trial") {
        TRIAL
    } else if id.contains("new") {
        ACQUISITION
    } else if id.contains("customer") || id.contains("active_user") {
        RETENTION
    } else if id.contains("mrr") || id.contains("revenue") || id.contains("subscription") {
        MONEY
    } else {
        DIM
    }
}

pub fn shade(color: Color, factor: f32) -> Color {
    match color {
        Color::Rgb(red, green, blue) => Color::Rgb(
            (f32::from(red) * factor).round().clamp(0.0, 255.0) as u8,
            (f32::from(green) * factor).round().clamp(0.0, 255.0) as u8,
            (f32::from(blue) * factor).round().clamp(0.0, 255.0) as u8,
        ),
        other => other,
    }
}
