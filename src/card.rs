use std::fmt::Write as _;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{Datelike, Local};

use crate::moments::{Kind, Moment, Tone};

const WIDTH: f64 = 1200.0;
const HEIGHT: f64 = 675.0;
const ZOOM: &str = "2";
const MARGIN: f64 = 72.0;
const MONO: &str = "FiraCode Nerd Font, Noto Color Emoji, monospace";
const SERIF: &str = "Noto Serif, Liberation Serif, serif";
const CHAR_WIDTH: f64 = 0.6;
const BG: &str = "#0b0d11";
const SURFACE: &str = "#11151c";
const BORDER: &str = "#1c222a";
const TEXT: &str = "#c9d1d9";
const BRIGHT: &str = "#e8eff7";
const DIM: &str = "#7d8690";
const ICON_SIZE: f64 = 256.0;
const YEARBOOK_ROW: f64 = 74.0;
const YEARBOOK_NOTE: f64 = 26.0;
const YEARBOOK_MONTH: f64 = 64.0;

/// The colour each tone wears on cards, icons and lights, as RGB.
pub fn tone_rgb(tone: Tone) -> (u8, u8, u8) {
    match tone {
        Tone::Money => (0x3f, 0xd0, 0x7f),
        Tone::People => (0x39, 0xc5, 0xcf),
        Tone::World => (0x6a, 0xa9, 0xff),
        Tone::Stars => (0xf0, 0xc0, 0x60),
        Tone::Alert => (0xf8, 0x51, 0x49),
        Tone::Quiet => (0x7d, 0x86, 0x90),
    }
}

pub fn tone_hex(tone: Tone) -> String {
    let (red, green, blue) = tone_rgb(tone);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// Whether `rsvg-convert` is on the PATH; without it no PNG can be made.
pub fn available() -> bool {
    Command::new("rsvg-convert")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Renders a moment's 2400×1350 card into `dir`, never replacing an existing picture.
pub fn render(moment: &Moment, dir: &Path) -> io::Result<PathBuf> {
    let day = moment
        .local_time()
        .map(|time| time.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "moment".to_string());
    let prefix = if moment.test { "test-" } else { "" };
    let path = free_path(dir, &format!("{prefix}{day}-{}", moment.kind.slug()), "png");
    rasterize(&card_svg(moment), &path, ZOOM)?;
    Ok(path)
}

/// A square glyph (or flag) for the notification popup, cached by what it shows.
pub fn icon(moment: &Moment, cache: &Path) -> io::Result<PathBuf> {
    let name = match (&moment.kind, &moment.country) {
        (Kind::NewCountry, Some(code)) => format!("flag-{}", code.to_ascii_lowercase()),
        (kind, _) => kind.slug().to_string(),
    };
    let path = cache.join(format!("{name}.png"));
    if path.exists() {
        return Ok(path);
    }
    rasterize(&icon_svg(moment), &path, "1")?;
    Ok(path)
}

/// Renders the year's moments as one tall poster into `dir`.
pub fn yearbook(moments: &[Moment], year: i32, dir: &Path) -> io::Result<PathBuf> {
    let path = free_path(dir, &format!("{year}-yearbook"), "png");
    rasterize(&yearbook_svg(moments, year), &path, ZOOM)?;
    Ok(path)
}

fn free_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{extension}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|index| dir.join(format!("{stem}-{index}.{extension}")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

fn rasterize(svg: &str, path: &Path, zoom: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut child = Command::new("rsvg-convert")
        .args(["-z", zoom, "-f", "png", "-o"])
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(svg.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if output.status.success() && path.exists() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "rsvg-convert failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn card_svg(moment: &Moment) -> String {
    let accent = tone_hex(moment.kind.tone());
    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\">\
         <defs>\
         <radialGradient id=\"glow\" cx=\"0.86\" cy=\"0.08\" r=\"0.95\">\
         <stop offset=\"0\" stop-color=\"{accent}\" stop-opacity=\"0.30\"/>\
         <stop offset=\"0.45\" stop-color=\"{accent}\" stop-opacity=\"0.07\"/>\
         <stop offset=\"1\" stop-color=\"{accent}\" stop-opacity=\"0\"/>\
         </radialGradient>\
         <radialGradient id=\"under\" cx=\"0.08\" cy=\"1.05\" r=\"0.7\">\
         <stop offset=\"0\" stop-color=\"{accent}\" stop-opacity=\"0.10\"/>\
         <stop offset=\"1\" stop-color=\"{accent}\" stop-opacity=\"0\"/>\
         </radialGradient>\
         <linearGradient id=\"fill\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
         <stop offset=\"0\" stop-color=\"{accent}\" stop-opacity=\"0.42\"/>\
         <stop offset=\"1\" stop-color=\"{accent}\" stop-opacity=\"0\"/>\
         </linearGradient>\
         </defs>\
         <rect width=\"100%\" height=\"100%\" fill=\"{BG}\"/>\
         <rect width=\"100%\" height=\"100%\" fill=\"url(#glow)\"/>\
         <rect width=\"100%\" height=\"100%\" fill=\"url(#under)\"/>\
         <rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"4\" fill=\"{accent}\" opacity=\"0.85\"/>"
    );
    let date = moment
        .local_time()
        .map(|time| time.format("%A %-d %B %Y").to_string())
        .unwrap_or_default();
    text(
        &mut svg,
        MARGIN,
        82.0,
        17.0,
        DIM,
        "start",
        "",
        "letter-spacing=\"4\"",
        "RC-TUI · MOMENT",
    );
    text(
        &mut svg,
        WIDTH - MARGIN,
        82.0,
        17.0,
        DIM,
        "end",
        "",
        "",
        &date,
    );
    if moment.test {
        text(
            &mut svg,
            WIDTH / 2.0,
            82.0,
            17.0,
            &accent,
            "middle",
            "bold",
            "letter-spacing=\"4\"",
            "SAMPLE",
        );
    }

    text(
        &mut svg,
        MARGIN,
        176.0,
        44.0,
        &accent,
        "start",
        "",
        "",
        moment.kind.glyph(),
    );
    let title_size = fit(&moment.title, 54.0, WIDTH - 2.0 * MARGIN - 70.0);
    text(
        &mut svg,
        MARGIN + 66.0,
        174.0,
        title_size,
        BRIGHT,
        "start",
        "bold",
        "",
        &moment.title,
    );

    let hero = crate::moments::plain(&moment.value);
    let stamp = match (&moment.kind, &moment.country) {
        (Kind::NewCountry, Some(code)) => Some(code.to_ascii_uppercase()),
        _ => None,
    };
    let stamp_room = if stamp.is_some() { 220.0 } else { 0.0 };
    let hero_size = fit(&hero, 168.0, WIDTH - 2.0 * MARGIN - stamp_room);
    let hero_y = 196.0 + hero_size * 0.92;
    if let Some(code) = &stamp {
        draw_stamp(
            &mut svg,
            MARGIN,
            hero_y - hero_size * 0.74,
            hero_size * 0.78,
            code,
            &accent,
        );
    }
    text(
        &mut svg,
        MARGIN + stamp_room - hero_size * 0.04,
        hero_y,
        hero_size,
        &accent,
        "start",
        "bold",
        "",
        &hero,
    );

    let mut y = hero_y + 66.0;
    match &moment.quote {
        Some(quote) if moment.kind == Kind::FiveStars => {
            for line in wrap(&format!("“{quote}”"), 52).into_iter().take(3) {
                serif(&mut svg, MARGIN, y, 30.0, BRIGHT, &line);
                y += 42.0;
            }
            for line in wrap(&moment.line, 70).into_iter().take(1) {
                text(&mut svg, MARGIN, y + 6.0, 22.0, DIM, "start", "", "", &line);
            }
        }
        _ => {
            for line in wrap(&crate::moments::plain(&moment.line), 66)
                .into_iter()
                .take(3)
            {
                text(&mut svg, MARGIN, y, 26.0, TEXT, "start", "", "", &line);
                y += 38.0;
            }
        }
    }

    let band_top = (y + 4.0).max(500.0);
    let band = Band {
        left: MARGIN,
        top: band_top,
        width: WIDTH - 2.0 * MARGIN,
        height: 604.0 - band_top,
    };
    match moment.kind {
        Kind::BestDay | Kind::BestMonth if moment.series.len() >= 2 => {
            let floor = if moment.kind == Kind::BestMonth {
                moment.series.iter().copied().fold(f64::MAX, f64::min) * 0.85
            } else {
                0.0
            };
            draw_series(&mut svg, &moment.series, moment.mark, floor, &band, &accent);
        }
        Kind::Enough => draw_enough(&mut svg, moment, &band, &accent),
        Kind::People | Kind::AppPeople => draw_ladder(&mut svg, moment, &band, &accent),
        _ => {
            let _ = write!(
                svg,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"2\" fill=\"{BORDER}\"/>\
                 <rect x=\"{}\" y=\"{}\" width=\"140\" height=\"2\" fill=\"{accent}\"/>",
                band.left,
                band.top + band.height,
                band.width,
                band.left,
                band.top + band.height
            );
        }
    }

    if let Some(note) = &moment.note {
        serif(
            &mut svg,
            MARGIN,
            638.0,
            20.0,
            DIM,
            &clip(&format!("“{note}”"), 70),
        );
    } else {
        text(
            &mut svg,
            MARGIN,
            638.0,
            16.0,
            DIM,
            "start",
            "",
            "letter-spacing=\"2\"",
            "✦ rc-tui",
        );
    }
    if let Some(app) = &moment.app {
        text(
            &mut svg,
            WIDTH - MARGIN,
            638.0,
            18.0,
            TEXT,
            "end",
            "",
            "",
            app,
        );
    }
    svg.push_str("</svg>");
    svg
}

struct Band {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

/// The days (or rolling months) leading to a record: a soft area, the old record dashed, the new one lit.
fn draw_series(
    svg: &mut String,
    series: &[f64],
    mark: Option<f64>,
    floor: f64,
    band: &Band,
    accent: &str,
) {
    let top = band.top + 26.0;
    let height = (band.height - 26.0).max(20.0);
    let floor = floor.max(0.0);
    let peak = series
        .iter()
        .copied()
        .chain(mark)
        .fold(0.0_f64, f64::max)
        .max(floor + 1.0);
    let step = band.width / (series.len() - 1) as f64;
    let point = |index: usize, value: f64| {
        (
            band.left + index as f64 * step,
            top + height - ((value - floor).max(0.0) / (peak - floor)) * height,
        )
    };
    let mut line = String::new();
    for (index, value) in series.iter().enumerate() {
        let (x, y) = point(index, *value);
        let _ = write!(line, "{}{x:.1},{y:.1} ", if index == 0 { "M" } else { "L" });
    }
    let bottom = top + height;
    let _ = write!(
        svg,
        "<path d=\"{line}L{:.1},{bottom:.1} L{:.1},{bottom:.1} Z\" fill=\"url(#fill)\"/>\
         <path d=\"{line}\" fill=\"none\" stroke=\"{accent}\" stroke-width=\"3\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
        band.left + band.width,
        band.left
    );
    if let Some(mark) = mark {
        let (_, y) = point(0, mark);
        let _ = write!(
            svg,
            "<line x1=\"{:.1}\" y1=\"{y:.1}\" x2=\"{:.1}\" y2=\"{y:.1}\" stroke=\"{DIM}\" stroke-width=\"1.5\" stroke-dasharray=\"6 8\" opacity=\"0.8\"/>",
            band.left,
            band.left + band.width
        );
        text(
            svg,
            band.left + 4.0,
            y - 10.0,
            15.0,
            DIM,
            "start",
            "",
            "letter-spacing=\"2\"",
            "OLD RECORD",
        );
    }
    let last = series.len() - 1;
    let (x, y) = point(last, series[last]);
    let _ = write!(
        svg,
        "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"18\" fill=\"{accent}\" opacity=\"0.18\"/>\
         <circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"7\" fill=\"{accent}\"/>"
    );
}

/// A bar that fills to the 28-day total, with the enough line marked where it was crossed.
fn draw_enough(svg: &mut String, moment: &Moment, band: &Band, accent: &str) {
    let target = moment.mark.unwrap_or(1.0).max(1.0);
    let total = moment
        .value
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect::<String>()
        .parse::<f64>()
        .unwrap_or(target);
    let scale = (total / target).max(1.0) * 1.08;
    let y = band.top + band.height - 14.0;
    let filled = band.width * (total / target / scale).min(1.0);
    let tick = band.left + band.width / scale;
    let _ = write!(
        svg,
        "<rect x=\"{}\" y=\"{y}\" width=\"{}\" height=\"14\" rx=\"7\" fill=\"{SURFACE}\"/>\
         <rect x=\"{}\" y=\"{y}\" width=\"{filled:.1}\" height=\"14\" rx=\"7\" fill=\"{accent}\"/>\
         <rect x=\"{:.1}\" y=\"{}\" width=\"3\" height=\"34\" fill=\"{BRIGHT}\"/>",
        band.left,
        band.width,
        band.left,
        tick - 1.5,
        y - 10.0
    );
    text(
        svg,
        tick,
        y - 18.0,
        15.0,
        BRIGHT,
        "middle",
        "",
        "letter-spacing=\"2\"",
        "ENOUGH",
    );
}

/// The people milestones as a ladder, every rung reached lit and the new one glowing.
fn draw_ladder(svg: &mut String, moment: &Moment, band: &Band, accent: &str) {
    let steps: &[&str] = if moment.kind == Kind::People {
        &[
            "100", "250", "500", "1,000", "2,500", "5,000", "10,000", "25,000", "50,000", "100,000",
        ]
    } else {
        &["1,000", "10,000", "100,000", "1,000,000"]
    };
    let reached = steps
        .iter()
        .position(|step| *step == moment.value)
        .unwrap_or(0);
    let gap = band.width / (steps.len() - 1) as f64;
    let y = band.top + band.height - 10.0;
    let _ = write!(
        svg,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"2\" fill=\"{BORDER}\"/>\
         <rect x=\"{}\" y=\"{}\" width=\"{:.1}\" height=\"2\" fill=\"{accent}\"/>",
        band.left,
        y - 1.0,
        band.width,
        band.left,
        y - 1.0,
        gap * reached as f64
    );
    for (index, step) in steps.iter().enumerate() {
        let x = band.left + gap * index as f64;
        let (fill, radius) = match index.cmp(&reached) {
            std::cmp::Ordering::Less => (accent, 5.0),
            std::cmp::Ordering::Equal => (accent, 9.0),
            std::cmp::Ordering::Greater => (BORDER, 5.0),
        };
        if index == reached {
            let _ = write!(
                svg,
                "<circle cx=\"{x:.1}\" cy=\"{y}\" r=\"22\" fill=\"{accent}\" opacity=\"0.16\"/>"
            );
        }
        let _ = write!(
            svg,
            "<circle cx=\"{x:.1}\" cy=\"{y}\" r=\"{radius}\" fill=\"{fill}\"/>"
        );
        let color = if index <= reached { TEXT } else { DIM };
        let anchor = if index == 0 {
            "start"
        } else if index == steps.len() - 1 {
            "end"
        } else {
            "middle"
        };
        text(svg, x, y - 24.0, 15.0, color, anchor, "", "", step);
    }
}

fn icon_svg(moment: &Moment) -> String {
    let accent = tone_hex(moment.kind.tone());
    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{ICON_SIZE}\" height=\"{ICON_SIZE}\" viewBox=\"0 0 {ICON_SIZE} {ICON_SIZE}\">\
         <defs><radialGradient id=\"g\" cx=\"0.3\" cy=\"0.2\" r=\"1\">\
         <stop offset=\"0\" stop-color=\"{accent}\" stop-opacity=\"0.55\"/>\
         <stop offset=\"1\" stop-color=\"{accent}\" stop-opacity=\"0.08\"/>\
         </radialGradient></defs>\
         <rect width=\"{ICON_SIZE}\" height=\"{ICON_SIZE}\" rx=\"60\" fill=\"{BG}\"/>\
         <rect width=\"{ICON_SIZE}\" height=\"{ICON_SIZE}\" rx=\"60\" fill=\"url(#g)\"/>\
         <rect x=\"3\" y=\"3\" width=\"{}\" height=\"{}\" rx=\"57\" fill=\"none\" stroke=\"{accent}\" stroke-opacity=\"0.6\" stroke-width=\"6\"/>",
        ICON_SIZE - 6.0,
        ICON_SIZE - 6.0
    );
    let (glyph, size, color) = match (&moment.kind, &moment.country) {
        (Kind::NewCountry, Some(code)) => (code.to_ascii_uppercase(), 118.0, BRIGHT.to_string()),
        (kind, _) => (kind.glyph().to_string(), 150.0, accent.clone()),
    };
    text(
        &mut svg,
        ICON_SIZE / 2.0,
        ICON_SIZE / 2.0 + size * 0.36,
        size,
        &color,
        "middle",
        "bold",
        "",
        &glyph,
    );
    svg.push_str("</svg>");
    svg
}

fn yearbook_svg(moments: &[Moment], year: i32) -> String {
    let mut kept: Vec<&Moment> = moments
        .iter()
        .filter(|moment| moment.kind.keeps() && !moment.test)
        .filter(|moment| moment.local_time().is_some_and(|time| time.year() == year))
        .collect();
    kept.sort_by_key(|moment| moment.at);

    let header = 330.0;
    let mut body = 0.0;
    let mut month = None;
    for moment in &kept {
        let this = moment.local_time().map(|time| time.month());
        if this != month {
            body += YEARBOOK_MONTH;
            month = this;
        }
        body += YEARBOOK_ROW
            + if moment.note.is_some() {
                YEARBOOK_NOTE
            } else {
                0.0
            };
    }
    if kept.is_empty() {
        body = 120.0;
    }
    let height = header + body + 140.0;
    let money = tone_hex(Tone::Money);
    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height}\" viewBox=\"0 0 {WIDTH} {height}\">\
         <defs><radialGradient id=\"glow\" cx=\"0.85\" cy=\"0\" r=\"0.9\" gradientUnits=\"objectBoundingBox\">\
         <stop offset=\"0\" stop-color=\"{money}\" stop-opacity=\"0.22\"/>\
         <stop offset=\"1\" stop-color=\"{money}\" stop-opacity=\"0\"/>\
         </radialGradient></defs>\
         <rect width=\"100%\" height=\"100%\" fill=\"{BG}\"/>\
         <rect width=\"{WIDTH}\" height=\"900\" fill=\"url(#glow)\"/>\
         <rect x=\"0\" y=\"0\" width=\"{WIDTH}\" height=\"4\" fill=\"{money}\"/>"
    );
    text(
        &mut svg,
        MARGIN,
        82.0,
        17.0,
        DIM,
        "start",
        "",
        "letter-spacing=\"4\"",
        "RC-TUI · YEARBOOK",
    );
    text(
        &mut svg,
        MARGIN - 8.0,
        238.0,
        168.0,
        &money,
        "start",
        "bold",
        "",
        &year.to_string(),
    );
    let count = kept.len();
    let subtitle = match count {
        0 => "No moments yet. They are on their way.".to_string(),
        1 => "One moment worth remembering.".to_string(),
        _ => format!("{count} moments worth remembering."),
    };
    text(
        &mut svg, MARGIN, 290.0, 26.0, TEXT, "start", "", "", &subtitle,
    );

    let mut y = header;
    let mut month = None;
    for moment in &kept {
        let time = moment.local_time();
        let this = time.map(|time| time.month());
        if this != month {
            month = this;
            y += YEARBOOK_MONTH;
            let label = time
                .map(|time| time.format("%B").to_string().to_uppercase())
                .unwrap_or_default();
            text(
                &mut svg,
                MARGIN,
                y - 16.0,
                16.0,
                DIM,
                "start",
                "",
                "letter-spacing=\"4\"",
                &label,
            );
            let _ = write!(
                svg,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"1\" fill=\"{BORDER}\"/>",
                MARGIN + 160.0,
                y - 22.0,
                WIDTH - 2.0 * MARGIN - 160.0
            );
        }
        let accent = tone_hex(moment.kind.tone());
        let date = time
            .map(|time| time.format("%-d %b").to_string())
            .unwrap_or_default();
        text(
            &mut svg,
            MARGIN,
            y + 24.0,
            18.0,
            DIM,
            "start",
            "",
            "",
            &date,
        );
        text(
            &mut svg,
            MARGIN + 96.0,
            y + 25.0,
            24.0,
            &accent,
            "start",
            "",
            "",
            moment.kind.glyph(),
        );
        text(
            &mut svg,
            MARGIN + 134.0,
            y + 24.0,
            24.0,
            BRIGHT,
            "start",
            "bold",
            "",
            &clip(&moment.title, 44),
        );
        text(
            &mut svg,
            WIDTH - MARGIN,
            y + 24.0,
            24.0,
            &accent,
            "end",
            "bold",
            "",
            &clip(&crate::moments::plain(&moment.value), 18),
        );
        text(
            &mut svg,
            MARGIN + 134.0,
            y + 54.0,
            17.0,
            TEXT,
            "start",
            "",
            "",
            &clip(&crate::moments::plain(&moment.line), 86),
        );
        y += YEARBOOK_ROW;
        if let Some(note) = &moment.note {
            serif(
                &mut svg,
                MARGIN + 134.0,
                y + 4.0,
                18.0,
                DIM,
                &clip(&format!("“{note}”"), 84),
            );
            y += YEARBOOK_NOTE;
        }
    }
    let made = Local::now().format("%-d %B %Y").to_string();
    text(
        &mut svg,
        MARGIN,
        height - 64.0,
        16.0,
        DIM,
        "start",
        "",
        "letter-spacing=\"2\"",
        &format!("✦ rc-tui · made {made}"),
    );
    svg.push_str("</svg>");
    svg
}

/// A country's ISO code in an outlined tile, standing in for the flag that librsvg cannot paint in colour.
fn draw_stamp(svg: &mut String, x: f64, y: f64, size: f64, code: &str, accent: &str) {
    let width = 190.0;
    let _ = write!(
        svg,
        "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{width}\" height=\"{size:.1}\" rx=\"22\" fill=\"{accent}\" fill-opacity=\"0.12\" stroke=\"{accent}\" stroke-width=\"4\"/>"
    );
    text(
        svg,
        x + width / 2.0,
        y + size * 0.68,
        size * 0.52,
        BRIGHT,
        "middle",
        "bold",
        "letter-spacing=\"6\"",
        code,
    );
}

#[allow(clippy::too_many_arguments)]
fn text(
    svg: &mut String,
    x: f64,
    y: f64,
    size: f64,
    fill: &str,
    anchor: &str,
    weight: &str,
    extra: &str,
    content: &str,
) {
    let weight = if weight.is_empty() {
        String::new()
    } else {
        format!(" font-weight=\"{weight}\"")
    };
    let _ = write!(
        svg,
        "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"{MONO}\" font-size=\"{size:.1}\" fill=\"{fill}\" text-anchor=\"{anchor}\"{weight} {extra}>{}</text>",
        escape(content)
    );
}

fn serif(svg: &mut String, x: f64, y: f64, size: f64, fill: &str, content: &str) {
    let _ = write!(
        svg,
        "<text x=\"{x:.1}\" y=\"{y:.1}\" font-family=\"{SERIF}\" font-style=\"italic\" font-size=\"{size:.1}\" fill=\"{fill}\">{}</text>",
        escape(content)
    );
}

/// The largest size up to `wanted` at which `content` fits `room` pixels of monospace.
fn fit(content: &str, wanted: f64, room: f64) -> f64 {
    let columns = unicode_width::UnicodeWidthStr::width(content).max(1) as f64;
    wanted.min(room / (columns * CHAR_WIDTH)).max(18.0)
}

fn wrap(content: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in content.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > columns {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn clip(content: &str, columns: usize) -> String {
    if content.chars().count() <= columns {
        return content.to_string();
    }
    let cut: String = content.chars().take(columns.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

fn escape(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hero_text_shrinks_to_fit_the_card() {
        assert_eq!(fit("€182", 168.0, 1056.0), 168.0);
        let long = fit("Master of Inventory", 168.0, 1056.0);
        assert!(long < 168.0 && long * 0.6 * 19.0 <= 1056.0 + 0.01);
    }

    #[test]
    fn wraps_on_words_and_escapes_markup() {
        assert_eq!(
            wrap("one two three four", 9),
            vec!["one two", "three", "four"]
        );
        assert_eq!(escape("a < b & \"c\""), "a &lt; b &amp; &quot;c&quot;");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(
            crate::moments::plain("🇯🇵 Japan, from 🇺🇸 US"),
            "Japan, from US"
        );
    }

    #[test]
    fn never_replaces_an_existing_picture() {
        let dir = std::env::temp_dir().join(format!("rc-tui-card-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("day.png"), b"x").unwrap();
        assert_eq!(free_path(&dir, "day", "png"), dir.join("day-2.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_card_kind_draws_a_complete_svg() {
        let mut moment = Moment::sample("EUR", 1_792_972_800_000);
        for kind in [
            Kind::BestDay,
            Kind::Enough,
            Kind::People,
            Kind::AppPeople,
            Kind::FiveStars,
            Kind::NewCountry,
        ] {
            moment.kind = kind;
            moment.quote = Some("Calm & <kind>".to_string());
            moment.country = Some("JP".to_string());
            let svg = card_svg(&moment);
            assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
            assert!(!svg.contains("<kind>"));
            let icon = icon_svg(&moment);
            assert!(icon.ends_with("</svg>"));
        }
        let book = yearbook_svg(&[moment], 2026);
        assert!(book.contains("2026"));
    }

    /// Writes every card kind and a yearbook as PNGs for eyeballing: `RC_TUI_CARD_DIR=dir cargo test cards_to_disk -- --ignored`.
    #[test]
    #[ignore]
    fn cards_to_disk() {
        let Ok(dir) = std::env::var("RC_TUI_CARD_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let now = chrono::Utc::now().timestamp_millis();
        let mut all = Vec::new();
        let mut best = Moment::sample("EUR", now);
        best.test = false;
        best.app = Some("Solar Beam".to_string());
        best.line.push_str(" Solar Beam brought €120 of it.");
        all.push(best.clone());
        let mut month = best.clone();
        month.kind = Kind::BestMonth;
        month.title = "Best 28 days ever".to_string();
        month.value = "€2,410".to_string();
        month.line = "€2,410 in the last 28 days, more than any 28 days before. The previous best was €1,980.".to_string();
        month.series = (0..30)
            .map(|day| 1500.0 + day as f64 * 30.0 + ((day as f64) / 3.0).sin() * 60.0)
            .collect();
        month.mark = Some(1980.0);
        month.app = None;
        month.note = Some("The night-mode update shipped on Monday.".to_string());
        all.push(month);
        let mut enough = best.clone();
        enough.kind = Kind::Enough;
        enough.title = "Enough".to_string();
        enough.value = "€3,012".to_string();
        enough.line = "The last 28 days paid €3,012, past the €3,000 you call enough. Everything beyond it is room to breathe.".to_string();
        enough.mark = Some(3000.0);
        enough.series.clear();
        enough.app = None;
        all.push(enough);
        let mut people = best.clone();
        people.kind = Kind::People;
        people.title = "1,000 people".to_string();
        people.value = "1,000".to_string();
        people.line =
            "For the first time, more than 1,000 people used your apps in 28 days.".to_string();
        people.series.clear();
        people.app = None;
        all.push(people);
        let mut country = best.clone();
        country.kind = Kind::NewCountry;
        country.title = "First purchase from Japan".to_string();
        country.value = crate::countries::label("JP");
        country.country = Some("JP".to_string());
        country.line =
            "Someone in Japan bought Solar Beam. Your apps have now sold in 16 countries."
                .to_string();
        country.series.clear();
        all.push(country);
        let mut stars = best.clone();
        stars.kind = Kind::FiveStars;
        stars.title = "Five stars".to_string();
        stars.value = "★★★★★".to_string();
        stars.line = "A five-star review of Psywave: “Finally a calm one”".to_string();
        stars.quote = Some("I use it every night before I sleep. It is the only app that makes my evenings quieter instead of louder. Thank you for making it.".to_string());
        stars.app = Some("Psywave".to_string());
        stars.series.clear();
        all.push(stars);
        let mut first = best.clone();
        first.kind = Kind::FirstSale;
        first.title = "First sale".to_string();
        first.value = "Master of Inventory".to_string();
        first.line = "Master of Inventory made its first sale: $4.99 from 🇺🇸 United States. Someone paid for something you made.".to_string();
        first.app = Some("Master of Inventory".to_string());
        first.series.clear();
        all.push(first);
        for moment in &all {
            let path = render(moment, &dir).unwrap();
            icon(moment, &dir.join("icons")).unwrap();
            println!("{}", path.display());
        }
        println!(
            "{}",
            yearbook(&all, Local::now().year(), &dir).unwrap().display()
        );
    }
}
