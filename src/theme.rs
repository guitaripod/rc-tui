use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Light,
    Dark,
}

impl Mode {
    pub fn parse(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub border: Color,
    pub border_strong: Color,
    pub text: Color,
    pub bright: Color,
    pub dim: Color,
    pub money: Color,
    pub acquisition: Color,
    pub retention: Color,
    pub trial: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub selection: Color,
}

const DARK: Palette = Palette {
    bg: Color::Rgb(0x0b, 0x0d, 0x11),
    surface: Color::Rgb(0x11, 0x15, 0x1c),
    border: Color::Rgb(0x1c, 0x22, 0x2a),
    border_strong: Color::Rgb(0x2d, 0x33, 0x3b),
    text: Color::Rgb(0xc9, 0xd1, 0xd9),
    bright: Color::Rgb(0xe8, 0xef, 0xf7),
    dim: Color::Rgb(0x7d, 0x86, 0x90),
    money: Color::Rgb(0x3f, 0xd0, 0x7f),
    acquisition: Color::Rgb(0x6a, 0xa9, 0xff),
    retention: Color::Rgb(0x39, 0xc5, 0xcf),
    trial: Color::Rgb(0xf0, 0x88, 0x3e),
    good: Color::Rgb(0x3f, 0xb9, 0x50),
    warn: Color::Rgb(0xf0, 0xc0, 0x60),
    bad: Color::Rgb(0xf8, 0x51, 0x49),
    selection: Color::Rgb(0x17, 0x25, 0x3f),
};

const LIGHT: Palette = Palette {
    bg: Color::Rgb(0xf7, 0xf3, 0xe8),
    surface: Color::Rgb(0xff, 0xfc, 0xf3),
    border: Color::Rgb(0xdd, 0xd6, 0xc5),
    border_strong: Color::Rgb(0xbf, 0xb6, 0xa2),
    text: Color::Rgb(0x30, 0x34, 0x3a),
    bright: Color::Rgb(0x14, 0x17, 0x1a),
    dim: Color::Rgb(0x6d, 0x67, 0x5c),
    money: Color::Rgb(0x0b, 0x7a, 0x43),
    acquisition: Color::Rgb(0x2f, 0x63, 0xb0),
    retention: Color::Rgb(0x0d, 0x74, 0x80),
    trial: Color::Rgb(0xa9, 0x4c, 0x13),
    good: Color::Rgb(0x2f, 0x7c, 0x37),
    warn: Color::Rgb(0x8a, 0x62, 0x00),
    bad: Color::Rgb(0xb3, 0x26, 0x1e),
    selection: Color::Rgb(0xe8, 0xee, 0xf8),
};

static LIGHT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn init(value: Option<&str>) {
    apply(Mode::parse(value));
}

pub fn apply(mode: Mode) {
    let light = match mode {
        Mode::Auto => detect_auto(),
        Mode::Light => true,
        Mode::Dark => false,
    };
    LIGHT_ACTIVE.store(light, Ordering::Relaxed);
}

pub fn toggle() -> Mode {
    let mode = if is_light() { Mode::Dark } else { Mode::Light };
    apply(mode);
    mode
}

pub fn is_light() -> bool {
    LIGHT_ACTIVE.load(Ordering::Relaxed)
}

pub fn palette() -> &'static Palette {
    if is_light() { &LIGHT } else { &DARK }
}

pub fn bg() -> Color {
    palette().bg
}

pub fn surface() -> Color {
    palette().surface
}

pub fn border() -> Color {
    palette().border
}

pub fn border_strong() -> Color {
    palette().border_strong
}

pub fn text() -> Color {
    palette().text
}

pub fn bright() -> Color {
    palette().bright
}

pub fn dim() -> Color {
    palette().dim
}

pub fn money() -> Color {
    palette().money
}

pub fn acquisition() -> Color {
    palette().acquisition
}

pub fn retention() -> Color {
    palette().retention
}

pub fn trial() -> Color {
    palette().trial
}

pub fn good() -> Color {
    palette().good
}

pub fn warn() -> Color {
    palette().warn
}

pub fn bad() -> Color {
    palette().bad
}

pub fn selection() -> Color {
    palette().selection
}

pub fn accent(id: &str) -> Color {
    let id = id.to_ascii_lowercase();
    if id.contains("trial") {
        trial()
    } else if id.contains("new") {
        acquisition()
    } else if id.contains("customer") || id.contains("active_user") {
        retention()
    } else if id.contains("mrr") || id.contains("revenue") || id.contains("subscription") {
        money()
    } else {
        dim()
    }
}

pub fn shade(color: Color, factor: f32) -> Color {
    shade_with(palette(), color, factor)
}

fn shade_with(palette: &Palette, color: Color, factor: f32) -> Color {
    let Color::Rgb(red, green, blue) = color else {
        return color;
    };
    if *palette == LIGHT {
        if factor <= 1.0 {
            mix(
                (red, green, blue),
                rgb(palette.bg),
                0.85 * (1.0 - factor).clamp(0.0, 1.0),
            )
        } else {
            mix(
                (red, green, blue),
                rgb(palette.bright),
                0.35 * (factor - 1.0).clamp(0.0, 1.0),
            )
        }
    } else {
        Color::Rgb(
            (f32::from(red) * factor).round().clamp(0.0, 255.0) as u8,
            (f32::from(green) * factor).round().clamp(0.0, 255.0) as u8,
            (f32::from(blue) * factor).round().clamp(0.0, 255.0) as u8,
        )
    }
}

fn detect_auto() -> bool {
    if let Some(light) = colorfgbg() {
        return light;
    }
    if let Some(light) = terminal_background() {
        return light;
    }
    false
}

fn colorfgbg() -> Option<bool> {
    let value = std::env::var("COLORFGBG").ok()?;
    let background = value.rsplit(';').next()?.trim().parse::<u8>().ok()?;
    Some(background == 7 || background >= 9)
}

fn terminal_background() -> Option<bool> {
    use std::io::{IsTerminal, Read, Write};
    use std::os::fd::AsRawFd;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    let mut tty = std::fs::File::open("/dev/tty").ok()?;
    ratatui::crossterm::terminal::enable_raw_mode().ok()?;
    let mut response = [0u8; 128];
    let read = (|| {
        let mut stdout = std::io::stdout();
        stdout.write_all(b"\x1b]11;?\x1b\\").ok()?;
        stdout.flush().ok()?;
        let mut poll = libc::pollfd {
            fd: tty.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, 140) };
        if ready > 0 {
            tty.read(&mut response).ok()
        } else {
            None
        }
    })();
    let _ = ratatui::crossterm::terminal::disable_raw_mode();
    let length = read?;
    parse_background(&response[..length])
}

fn parse_background(response: &[u8]) -> Option<bool> {
    let text = String::from_utf8_lossy(response);
    let start = text.find("rgb:")? + 4;
    let rgb = text[start..]
        .split(['/', ';', '\x07', '\x1b'])
        .take(3)
        .collect::<Vec<_>>();
    let channel = |value: &str| -> Option<u16> {
        let value = value.split(':').next()?;
        let digits = value.len();
        let value = u32::from_str_radix(value, 16).ok()?;
        let max = 16u32.pow(digits as u32).saturating_sub(1).max(1);
        Some((value * 255 / max) as u16)
    };
    let red = channel(rgb.first()?)?;
    let green = channel(rgb.get(1)?)?;
    let blue = channel(rgb.get(2)?)?;
    let luminance = 0.2126 * f32::from(red) + 0.7152 * f32::from(green) + 0.0722 * f32::from(blue);
    Some(luminance > 128.0)
}

fn rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(red, green, blue) => (red, green, blue),
        _ => (0, 0, 0),
    }
}

fn mix(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let channel = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color::Rgb(
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color) -> f32 {
        let (red, green, blue) = rgb(color);
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.03928 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
    }

    fn contrast(left: Color, right: Color) -> f32 {
        let (light, dark) = if luminance(left) > luminance(right) {
            (luminance(left), luminance(right))
        } else {
            (luminance(right), luminance(left))
        };
        (light + 0.05) / (dark + 0.05)
    }

    #[test]
    fn light_palette_keeps_every_ink_readable() {
        for ink in [
            LIGHT.text,
            LIGHT.bright,
            LIGHT.dim,
            LIGHT.money,
            LIGHT.acquisition,
            LIGHT.retention,
            LIGHT.trial,
            LIGHT.good,
            LIGHT.warn,
            LIGHT.bad,
        ] {
            assert!(
                contrast(ink, LIGHT.bg) >= 4.5,
                "{ink:?} on light background only reaches {:.2}:1",
                contrast(ink, LIGHT.bg)
            );
        }
        assert!(contrast(LIGHT.bright, LIGHT.surface) >= 12.0);
    }

    #[test]
    fn osc_background_colours_are_read_at_every_precision() {
        assert_eq!(
            parse_background(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
            Some(true)
        );
        assert_eq!(
            parse_background(b"\x1b]11;rgb:0000/0000/0000\x1b\\"),
            Some(false)
        );
        assert_eq!(
            parse_background(b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07"),
            Some(false)
        );
        assert_eq!(parse_background(b"\x1b]11;rgb:ff/ff/ff\x07"), Some(true));
    }

    #[test]
    fn a_light_chart_fill_is_lighter_than_its_edge() {
        let fill = shade_with(&LIGHT, LIGHT.money, 0.4);
        assert!(luminance(fill) > luminance(LIGHT.money));
    }
}
