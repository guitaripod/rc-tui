use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::app::{App, AppOptions, Motion, Source};
use crate::{api, theme, ui};

const CELL_WIDTH: f64 = 11.0;
const CELL_HEIGHT: f64 = 23.0;
const FONT_SIZE: f64 = 18.0;

/// Builds the 17-project demo dashboard with animation off and waits until every card is loaded.
fn demo_app(enough: Option<f64>) -> App {
    let mut app = App::new(AppOptions {
        sources: api::DEMO_PROJECTS
            .iter()
            .enumerate()
            .map(|(index, name)| Source::demo(*name, format!("proj_demo_{}", index + 1)))
            .collect(),
        currency: "EUR".to_string(),
        interval: Duration::from_secs(300),
        chart_ttl: Duration::from_secs(900),
        motion: Motion::Off,
        wall: false,
        pins: Vec::new(),
        ui_scale: 0,
        ui_page: 0,
        feed_path: None,
        feed_seen_at: None,
        enough,
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while (app.cards.is_empty() || app.live_count() < app.project_count())
        && Instant::now() < deadline
    {
        app.tick();
        std::thread::sleep(Duration::from_millis(10));
    }
    app
}

/// Renders one frame of the app at the given size and writes it to `path` as an SVG.
fn render(app: &mut App, width: u16, height: u16, path: &Path) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui::draw(app, frame)).unwrap();
    std::fs::write(path, svg(terminal.backend().buffer())).unwrap();
}

/// Converts a cell buffer to an SVG, painting half and full blocks as rectangles so charts have no font gaps.
fn svg(buffer: &Buffer) -> String {
    let area = buffer.area;
    let width = area.width as f64 * CELL_WIDTH;
    let height = area.height as f64 * CELL_HEIGHT;
    let background = hex(theme::bg(), theme::bg());
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         font-family=\"FiraCode Nerd Font Mono\" font-size=\"{FONT_SIZE}\">\
         <rect width=\"100%\" height=\"100%\" fill=\"{background}\"/>"
    );
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buffer[(x, y)];
            let left = x as f64 * CELL_WIDTH;
            let top = y as f64 * CELL_HEIGHT;
            if cell.bg != Color::Reset && cell.bg != theme::bg() {
                let _ = write!(
                    out,
                    "<rect x=\"{left}\" y=\"{top}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    CELL_WIDTH + 0.5,
                    CELL_HEIGHT + 0.5,
                    hex(cell.bg, theme::bg())
                );
            }
            let fill = hex(cell.fg, theme::text());
            let half = CELL_HEIGHT / 2.0;
            let block = match cell.symbol() {
                "█" => Some((top, CELL_HEIGHT)),
                "▀" => Some((top, half)),
                "▄" => Some((top + half, half)),
                _ => None,
            };
            if let Some((block_top, block_height)) = block {
                let _ = write!(
                    out,
                    "<rect x=\"{left}\" y=\"{block_top}\" width=\"{}\" height=\"{}\" fill=\"{fill}\"/>",
                    CELL_WIDTH + 0.5,
                    block_height + 0.5
                );
                continue;
            }
            let symbol = cell.symbol();
            if symbol.trim().is_empty() {
                continue;
            }
            let weight = if cell.modifier.contains(Modifier::BOLD) {
                " font-weight=\"bold\""
            } else {
                ""
            };
            let _ = write!(
                out,
                "<text x=\"{left}\" y=\"{}\" fill=\"{fill}\"{weight}>{}</text>",
                top + CELL_HEIGHT * 0.75,
                escape(symbol)
            );
        }
    }
    out.push_str("</svg>");
    out
}

fn hex(color: Color, fallback: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Reset => hex(fallback, Color::Rgb(0, 0, 0)),
        _ => hex(fallback, Color::Rgb(0, 0, 0)),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Writes the README and poster states as SVGs: `RC_TUI_SNAPSHOT_DIR=dir cargo test snapshots -- --ignored`.
#[test]
#[ignore]
fn snapshots() {
    let Ok(dir) = std::env::var("RC_TUI_SNAPSHOT_DIR") else {
        return;
    };
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    theme::apply(theme::Mode::Dark);

    let mut app = demo_app(Some(3000.0));
    render(&mut app, 200, 54, &dir.join("home-200x54.svg"));
    render(&mut app, 140, 44, &dir.join("home-140x44.svg"));
    render(&mut app, 110, 34, &dir.join("home-110x34.svg"));
    render(&mut app, 90, 28, &dir.join("home-90x28.svg"));
    app.select_scope(2);
    render(&mut app, 200, 54, &dir.join("scope-200x54.svg"));
    app.select_scope(0);
    app.open_detail();
    render(&mut app, 200, 54, &dir.join("detail-200x54.svg"));

    app.detail = None;
    app.ui_scale = 1;
    render(&mut app, 200, 54, &dir.join("paged-200x54.svg"));
    app.ui_scale = 0;
    app.help = true;
    render(&mut app, 200, 54, &dir.join("help-200x54.svg"));
    app.help = false;
    app.open_activity();
    render(&mut app, 200, 54, &dir.join("feed-200x54.svg"));
    app.activity.open = false;
    app.open_moments(None);
    render(&mut app, 200, 54, &dir.join("moments-200x54.svg"));
    render(&mut app, 90, 28, &dir.join("moments-90x28.svg"));
    app.moments.open = false;

    let mut plain = demo_app(None);
    plain.wall = true;
    render(&mut plain, 200, 54, &dir.join("wall-200x54.svg"));

    theme::apply(theme::Mode::Light);
    let mut light = demo_app(Some(2000.0));
    render(&mut light, 200, 54, &dir.join("light-200x54.svg"));
    theme::apply(theme::Mode::Dark);
}
