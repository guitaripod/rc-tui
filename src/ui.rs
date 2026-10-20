use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use chrono::{Days, TimeZone};

use crate::activity::{Entry, EntryKind, Filter, Health, Range};
use crate::api::{DOCS_API_KEYS, Failure, SCOPE_CHARTS, SCOPE_OVERVIEW, SCOPE_PROJECTS, redact};
use crate::app::{App, Card, RowState, Status, Trend, Window};
use crate::import::{Import, Stage as ImportStage};
use crate::setup::{Setup, Stage};
use crate::theme::{
    BAD, BG, BORDER, BORDER_STRONG, BRIGHT, DIM, GOOD, MONEY, RETENTION, SELECTION, SURFACE, TEXT,
    TRIAL, WARN, shade,
};

pub use crate::theme::accent;

pub fn draw(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let now = Instant::now();

    if app.detail.is_some() {
        draw_detail(app, frame, area, now);
    } else {
        draw_dashboard(app, frame, area, now);
    }

    let delta = app.frame_delta();
    app.fx.process_effects(delta, frame.buffer_mut(), area);

    if app.picker.is_some() {
        draw_picker(app, frame, area, now);
    }
    if app.help {
        draw_help(frame, area);
    }
    if app.health.is_some() {
        draw_health(app, frame, area, now);
    }
}

fn draw_dashboard(app: &mut App, frame: &mut Frame, area: Rect, now: Instant) {
    if app.wall {
        let body = Rect {
            y: area.y + 1,
            height: area.height.saturating_sub(2),
            ..area
        };
        draw_cards(app, frame, body, now);
        draw_wall_hint(app, frame, area, now);
        return;
    }
    let [header, rule, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(app, frame, header, now);
    draw_rule(frame, rule);
    draw_body(app, frame, body, now);
    draw_footer(app, frame, footer, now);
}

fn draw_header(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(34)]).areas(area);

    let mut spans = vec![
        Span::styled(
            " RevenueCat",
            Style::new().fg(DIM).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    ", Style::new()),
        Span::styled(
            app.scope_label().to_uppercase(),
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(id) = app.scope_id() {
        spans.push(Span::styled(format!("  {id}"), Style::new().fg(DIM)));
    }
    if app.scope == 0 && app.project_count() > 1 {
        spans.push(Span::styled(
            format!("    {} projects", app.project_count()),
            Style::new().fg(DIM),
        ));
    }
    spans.push(Span::styled(
        format!("    {}", app.currency),
        Style::new().fg(DIM),
    ));
    if app.ui_scale > 0 {
        spans.push(Span::styled(
            format!("    {} / page", app.per_page()),
            Style::new().fg(DIM),
        ));
    }
    if app.demo {
        spans.push(Span::styled("    demo", Style::new().fg(WARN)));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), left);

    let (indicator, color) = if matches!(app.status, Status::Fetching) {
        ("updating".to_string(), DIM)
    } else if let Status::Error(_) = app.status {
        ("error".to_string(), BAD)
    } else if !app.has_data() {
        ("loading".to_string(), DIM)
    } else if app.stale(now) {
        let text = if app.partial() {
            format!("{}/{} live · stale", app.live_count(), app.project_count())
        } else {
            "stale".to_string()
        };
        (text, WARN)
    } else if app.partial() {
        (
            format!("{}/{} live", app.live_count(), app.project_count()),
            WARN,
        )
    } else {
        ("live".to_string(), GOOD)
    };
    let page = if app.ui_scale > 0 {
        format!("{}/{}   ·   ", app.page + 1, app.page_count())
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled(page, Style::new().fg(DIM)),
        Span::styled(
            format!("{}    ", chrono::Local::now().format("%H:%M")),
            Style::new().fg(DIM),
        ),
        Span::styled("● ", Style::new().fg(color)),
        Span::styled(indicator, Style::new().fg(color)),
        Span::styled(" ", Style::new()),
    ]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Right), right);
}

fn draw_rule(frame: &mut Frame, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(BORDER),
        )),
        area,
    );
}

fn column_count(width: u16, height: u16, count: usize) -> usize {
    let mut columns = (width / 44).clamp(1, 3) as usize;
    while columns < 3 && (height as usize) / count.div_ceil(columns) < 6 {
        columns += 1;
    }
    columns
}

fn draw_body(app: &mut App, frame: &mut Frame, area: Rect, now: Instant) {
    if app.activity.open {
        draw_activity(app, frame, area, now);
        return;
    }
    if app.wall {
        draw_cards(app, frame, area, now);
        return;
    }
    let rail_width = if area.width >= 172 && area.height >= 43 {
        Some(((area.width as f32 * 0.285) as u16).clamp(48, 64))
    } else {
        None
    };
    if let Some(rail_width) = rail_width {
        let [cards, _, rail] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(rail_width),
        ])
        .areas(area);
        draw_cards(app, frame, cards, now);
        draw_activity_rail(app, frame, rail, now);
        return;
    }
    let shelf_height = if area.width >= 96 && area.height >= 40 {
        ((area.height as f32 * 0.22) as u16).clamp(7, 10)
    } else if area.width >= 96 && area.height >= 34 {
        6
    } else {
        0
    };
    if shelf_height > 0 {
        let [cards, shelf] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(shelf_height)]).areas(area);
        draw_cards(app, frame, cards, now);
        draw_activity_shelf(app, frame, shelf, now);
    } else if area.width >= 96 && area.height > 1 {
        let [cards, ticker] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        draw_cards(app, frame, cards, now);
        draw_activity_ticker(app, frame, ticker, now);
    } else {
        draw_cards(app, frame, area, now);
    }
}

fn feed_now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn feed_health(app: &App) -> (String, Color) {
    let label = app.activity.health_label(feed_now_ms());
    let color = match app.activity.health {
        Health::Live => GOOD,
        Health::Stale => WARN,
        Health::Offline | Health::NotConnected | Health::Error => BAD,
    };
    (label, color)
}

fn feed_scope(app: &App) -> String {
    app.scope_label().to_uppercase()
}

fn feed_glyph(entry: &Entry) -> (&'static str, Color) {
    match entry.kind() {
        EntryKind::Purchase => {
            let event = entry
                .purchase
                .as_ref()
                .map_or("", |purchase| purchase.event.as_str());
            match event {
                "renewal" => ("⟳", RETENTION),
                "trial_conversion" => ("◐", TRIAL),
                "revenue" => ("~", DIM),
                _ => ("+", MONEY),
            }
        }
        EntryKind::Review => {
            let stars = entry.stars().unwrap_or(0);
            ("★", if stars <= 2 { BAD } else { WARN })
        }
        EntryKind::Rating => ("☆", WARN),
        EntryKind::Unknown => ("·", DIM),
    }
}

fn feed_stars(stars: i64) -> String {
    let stars = stars.clamp(0, 5) as usize;
    format!("{}{}", "★".repeat(stars), "☆".repeat(5 - stars))
}

fn feed_value(entry: &Entry) -> (String, Color) {
    match entry.kind() {
        EntryKind::Purchase => {
            let Some(purchase) = &entry.purchase else {
                return ("—".to_string(), DIM);
            };
            let text = format_metric(purchase.amount, "$", &purchase.currency);
            let prefix = if purchase.estimated { "~+" } else { "+" };
            (
                format!("{prefix}{text}"),
                if purchase.estimated { DIM } else { MONEY },
            )
        }
        EntryKind::Review => {
            let stars = entry.stars().unwrap_or(0);
            (feed_stars(stars), if stars <= 2 { BAD } else { WARN })
        }
        EntryKind::Rating => match entry.rating.as_ref() {
            Some(rating) => match rating.stars {
                Some(stars) => (feed_stars(stars), if stars <= 2 { BAD } else { WARN }),
                None => (format!("avg {:.1}", rating.average), DIM),
            },
            None => ("—".to_string(), DIM),
        },
        EntryKind::Unknown => ("—".to_string(), DIM),
    }
}

fn feed_body(entry: &Entry, width: usize) -> String {
    let body = match entry.kind() {
        EntryKind::Purchase => {
            let product = entry
                .purchase
                .as_ref()
                .map_or("", |purchase| purchase.product.as_str());
            if product.is_empty() || entry.app.to_lowercase().contains(&product.to_lowercase()) {
                entry.app.clone()
            } else {
                format!("{} · {}", entry.app, product)
            }
        }
        EntryKind::Review => {
            let excerpt = entry.review_excerpt(width.saturating_sub(entry.app.len() + 6).max(8));
            if excerpt.is_empty() {
                entry.app.clone()
            } else {
                format!("{} · “{}”", entry.app, excerpt)
            }
        }
        EntryKind::Rating => format!("{} · new rating", entry.app),
        EntryKind::Unknown => entry.app.clone(),
    };
    truncate_display(&body, width)
}

fn feed_stamp(entry: &Entry) -> String {
    entry.datetime().map_or_else(
        || "--:--".to_string(),
        |date| date.format("%H:%M").to_string(),
    )
}

fn feed_day(entry: &Entry) -> String {
    entry
        .datetime()
        .map_or_else(String::new, |date| date.date_naive().to_string())
}

fn feed_day_label(entry: &Entry, now_ms: i64) -> String {
    let Some(date) = entry.datetime() else {
        return "UNKNOWN".to_string();
    };
    let today = chrono::Local
        .timestamp_millis_opt(now_ms)
        .single()
        .map(|now| now.date_naive());
    let Some(today) = today else {
        return date.format("%d %b").to_string().to_uppercase();
    };
    let day = date.date_naive();
    if day == today {
        "TODAY".to_string()
    } else if day == today.pred_opt().unwrap_or(today) {
        "YESTERDAY".to_string()
    } else {
        date.format("%d %b").to_string().to_uppercase()
    }
}

fn feed_line(entry: &Entry, width: u16, selected: bool) -> Line<'static> {
    let background = if selected { SELECTION } else { BG };
    let (glyph, glyph_color) = feed_glyph(entry);
    let (value, value_color) = feed_value(entry);
    let fixed = 28;
    let body_width = (width as usize).saturating_sub(fixed);
    let body = feed_body(entry, body_width);
    let body_padding = " ".repeat(body_width.saturating_sub(body.chars().count()));
    Line::from(vec![
        Span::styled(
            format!(" {}  ", feed_stamp(entry)),
            Style::new().fg(DIM).bg(background),
        ),
        Span::styled(
            format!("{glyph}  "),
            Style::new().fg(glyph_color).bg(background),
        ),
        Span::styled(
            format!("{body}{body_padding}"),
            Style::new().fg(TEXT).bg(background),
        ),
        Span::styled(
            format!("  {value:>10}"),
            Style::new().fg(value_color).bg(background),
        ),
        Span::styled(
            format!("  {:>2}", entry.country),
            Style::new().fg(DIM).bg(background),
        ),
    ])
}

fn feed_review_pair(entry: &Entry, width: u16, selected: bool) -> [Line<'static>; 2] {
    let background = if selected { SELECTION } else { BG };
    let (glyph, glyph_color) = feed_glyph(entry);
    let (value, value_color) = feed_value(entry);
    let fixed = 28;
    let body_width = (width as usize).saturating_sub(fixed);
    let app = truncate_display(&entry.app, body_width);
    let app_padding = " ".repeat(body_width.saturating_sub(app.chars().count()));
    let quote_width = (width as usize).saturating_sub(12);
    let quote = entry.review_excerpt(quote_width);
    [
        Line::from(vec![
            Span::styled(
                format!(" {}  ", feed_stamp(entry)),
                Style::new().fg(DIM).bg(background),
            ),
            Span::styled(
                format!("{glyph}  "),
                Style::new().fg(glyph_color).bg(background),
            ),
            Span::styled(
                format!("{app}{app_padding}"),
                Style::new().fg(TEXT).bg(background),
            ),
            Span::styled(
                format!("  {value:>10}"),
                Style::new().fg(value_color).bg(background),
            ),
            Span::styled(
                format!("  {:>2}", entry.country),
                Style::new().fg(DIM).bg(background),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                format!("             “{quote}”"),
                Style::new().fg(DIM).bg(background),
            ),
            Span::styled(
                format!("  {:>2}", entry.country),
                Style::new().fg(DIM).bg(background),
            ),
        ]),
    ]
}

fn draw_feed_rows(
    frame: &mut Frame,
    area: Rect,
    entries: &[&Entry],
    selected: Option<usize>,
    now_ms: i64,
) {
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No activity in this range.",
                Style::new().fg(DIM),
            ))),
            area,
        );
        return;
    }
    let visible = area.height as usize;
    let selected_index = selected;
    let offset = selected_index
        .unwrap_or(0)
        .saturating_sub(visible.saturating_sub(1));
    let mut row = 0usize;
    let mut last_day = String::new();
    for (index, entry) in entries.iter().enumerate().skip(offset) {
        if row >= visible {
            break;
        }
        let day = feed_day(entry);
        if day != last_day {
            if row >= visible {
                break;
            }
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    feed_day_label(entry, now_ms),
                    Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                ))),
                Rect {
                    x: area.x,
                    y: area.y + row as u16,
                    width: area.width,
                    height: 1,
                },
            );
            row += 1;
            last_day = day;
            if row >= visible {
                break;
            }
        }
        let highlighted = selected_index == Some(index);
        let line = feed_line(entry, area.width, highlighted);
        let background = if highlighted { SELECTION } else { BG };
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(background)),
            Rect {
                x: area.x,
                y: area.y + row as u16,
                width: area.width,
                height: 1,
            },
        );
        row += 1;
    }
    if offset > 0 && area.height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("{offset} earlier · j/k"),
                Style::new().fg(DIM),
            ))),
            Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            },
        );
    }
}

fn draw_activity_ticker(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let entries = app.activity_entries_all(Range::D7);
    let (health, health_color) = feed_health(app);
    let latest = entries.first();
    let unread = app.activity_unread();
    let mut spans = vec![Span::styled(
        if unread > 0 { "● " } else { "· " },
        Style::new().fg(if unread > 0 { MONEY } else { DIM }),
    )];
    if let Some(entry) = latest {
        spans.push(Span::styled(
            format!("{}  ", feed_stamp(entry)),
            Style::new().fg(DIM),
        ));
        let (glyph, color) = feed_glyph(entry);
        spans.push(Span::styled(format!("{glyph}  "), Style::new().fg(color)));
        spans.push(Span::styled(
            truncate_display(&feed_body(entry, 60), 60),
            Style::new().fg(TEXT),
        ));
        let (value, value_color) = feed_value(entry);
        spans.push(Span::styled(
            format!("  {value}"),
            Style::new().fg(value_color),
        ));
    } else {
        spans.push(Span::styled(
            "activity starts with the next sale or review",
            Style::new().fg(DIM),
        ));
    }
    spans.push(Span::styled(
        format!("   {health}   f ACTIVITY",),
        Style::new().fg(health_color),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let _ = now;
}

fn draw_activity_shelf(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    if area.height < 2 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(BORDER_STRONG),
        )),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );
    frame.render_widget(
        Block::default().style(Style::new().bg(SURFACE)),
        Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        },
    );
    let entries = app.activity_entries_all(Range::D7);
    let (health, health_color) = feed_health(app);
    let unread = app.activity_unread();
    let header_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: 1,
    };
    let mut header = vec![
        Span::styled(
            "ACTIVITY",
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::new()),
        Span::styled(
            if unread > 0 { "● " } else { "· " },
            Style::new().fg(if unread > 0 { MONEY } else { DIM }),
        ),
        Span::styled(
            if unread > 0 {
                format!("{unread} NEW")
            } else {
                "LIVE".to_string()
            },
            Style::new().fg(if unread > 0 { MONEY } else { DIM }),
        ),
    ];
    if entries.len() > area.height.saturating_sub(2) as usize {
        header.push(Span::styled(
            format!(
                "  +{}",
                entries.len() - area.height.saturating_sub(2) as usize
            ),
            Style::new().fg(DIM),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(header)), header_area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(health, Style::new().fg(health_color)),
            Span::styled("   f FULL FEED", Style::new().fg(DIM)),
        ]))
        .alignment(Alignment::Right),
        Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: 1,
        },
    );
    let row_area = Rect {
        x: area.x + 2,
        y: area.y + 2,
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(3),
    };
    draw_feed_rows(frame, row_area, &entries, None, feed_now_ms());
    let _ = now;
}

fn draw_pulse(frame: &mut Frame, area: Rect, entries: &[&Entry], now_ms: i64) {
    let mut bins = vec![(0usize, 0usize); 24];
    for entry in entries {
        let age = now_ms.saturating_sub(entry.timestamp()).max(0) / 3_600_000;
        if age > 23 {
            continue;
        }
        let index = 23 - age;
        if entry.is_money() {
            bins[index as usize].0 += 1;
        } else if entry.is_review() {
            bins[index as usize].1 += 1;
        }
    }
    let width = area.width as usize;
    let bin_width = if width >= 48 { 2 } else { 1 };
    let top = area.y;
    let bottom = area.y + 1;
    for (index, (money, reviews)) in bins.iter().enumerate() {
        let x = area.x + (index * bin_width) as u16;
        let style_money = Style::new().fg(MONEY);
        let style_reviews = Style::new().fg(WARN);
        if *reviews > 0 {
            frame.buffer_mut().set_string(x, top, "█", style_reviews);
            if bin_width > 1 {
                frame
                    .buffer_mut()
                    .set_string(x + 1, top, "█", style_reviews);
            }
        }
        if *money > 0 {
            frame.buffer_mut().set_string(x, bottom, "█", style_money);
            if bin_width > 1 {
                frame
                    .buffer_mut()
                    .set_string(x + 1, bottom, "█", style_money);
            }
        }
        if *money == 0 && *reviews == 0 {
            frame
                .buffer_mut()
                .set_string(x, bottom, "·", Style::new().fg(BORDER));
        }
    }
}

fn draw_activity_rail(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    if area.height < 8 || area.width < 20 {
        return;
    }
    frame.render_widget(Block::default().style(Style::new().bg(SURFACE)), area);
    if area.x > 0 {
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.x - 1,
                y,
                "│",
                Style::new().fg(BORDER_STRONG).bg(BG),
            );
        }
    }
    let entries = app.activity_entries_all(Range::D7);
    let (health, health_color) = feed_health(app);
    let unread = app.activity_unread();
    let inner_x = area.x + 2;
    let inner_w = area.width.saturating_sub(4);
    let header_y = area.y;
    let header = vec![
        Span::styled(
            "ACTIVITY",
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::new()),
        Span::styled(
            if unread > 0 { "● " } else { "· " },
            Style::new().fg(if unread > 0 { MONEY } else { DIM }),
        ),
        Span::styled(
            if unread > 0 {
                format!("{unread} NEW")
            } else {
                "LIVE".to_string()
            },
            Style::new().fg(if unread > 0 { MONEY } else { DIM }),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(header)),
        Rect {
            x: inner_x,
            y: header_y,
            width: inner_w,
            height: 1,
        },
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("f", Style::new().fg(DIM))))
            .alignment(Alignment::Right),
        Rect {
            x: inner_x,
            y: header_y,
            width: inner_w,
            height: 1,
        },
    );

    let now_ms = feed_now_ms();
    let pulse_entries = app.activity_entries_all(Range::H24);
    let mut y = header_y + 1;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "PULSE 24H",
                Style::new().fg(DIM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   {} EVENTS", pulse_entries.len()),
                Style::new().fg(DIM),
            ),
        ])),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    y += 1;
    draw_pulse(
        frame,
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 2,
        },
        &pulse_entries,
        now_ms,
    );
    y += 3;
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner_w as usize),
            Style::new().fg(BORDER),
        )),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    y += 1;

    let attention = app.activity_attention();
    let mut attention_id = None;
    if let Some(entry) = attention {
        attention_id = Some(entry.id.clone());
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "ATTENTION · 1",
                Style::new().fg(DIM).add_modifier(Modifier::BOLD),
            ))),
            Rect {
                x: inner_x,
                y,
                width: inner_w,
                height: 1,
            },
        );
        y += 1;
        let (stars, color) = feed_value(entry);
        let stars = if entry.stars().is_some() {
            stars
        } else {
            "new rating".to_string()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!("{stars}  "), Style::new().fg(color)),
                Span::styled(
                    truncate_display(&entry.app, inner_w.saturating_sub(20) as usize),
                    Style::new().fg(BAD).add_modifier(Modifier::BOLD),
                ),
            ])),
            Rect {
                x: inner_x,
                y,
                width: inner_w,
                height: 1,
            },
        );
        y += 1;
        if entry.kind() == EntryKind::Review {
            let quote = entry.review_excerpt(inner_w.saturating_sub(4) as usize);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!("“{quote}”"),
                    Style::new().fg(TEXT),
                ))),
                Rect {
                    x: inner_x,
                    y,
                    width: inner_w,
                    height: 1,
                },
            );
            y += 1;
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(
                        "{} · written review · {}",
                        entry.country,
                        age_label(now_ms, entry.received_at)
                    ),
                    Style::new().fg(DIM),
                ))),
                Rect {
                    x: inner_x,
                    y,
                    width: inner_w,
                    height: 1,
                },
            );
            y += 1;
        }
    }

    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner_w as usize),
            Style::new().fg(BORDER),
        )),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    y += 1;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("LIVE", Style::new().fg(DIM).add_modifier(Modifier::BOLD)),
            Span::styled("  ", Style::new()),
            Span::styled(
                if unread > 0 {
                    format!("{unread} NEW")
                } else {
                    "UP TO DATE".to_string()
                },
                Style::new().fg(if unread > 0 { MONEY } else { DIM }),
            ),
        ])),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    y += 1;
    let avail = area.bottom().saturating_sub(y + 1);
    let mut row = 0u16;
    let mut last_day = String::new();
    let attention_key = attention_id.as_deref();
    for entry in entries
        .iter()
        .filter(|entry| Some(entry.id.as_str()) != attention_key)
    {
        if row >= avail {
            break;
        }
        let day = feed_day(entry);
        if day != last_day {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    feed_day_label(entry, now_ms),
                    Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                ))),
                Rect {
                    x: inner_x,
                    y: y + row,
                    width: inner_w,
                    height: 1,
                },
            );
            row += 1;
            last_day = day;
            if row >= avail {
                break;
            }
        }
        if entry.kind() == EntryKind::Review && row + 1 < avail {
            let pair = feed_review_pair(entry, inner_w, false);
            for line in pair {
                frame.render_widget(
                    Paragraph::new(line),
                    Rect {
                        x: inner_x,
                        y: y + row,
                        width: inner_w,
                        height: 1,
                    },
                );
                row += 1;
            }
        } else {
            frame.render_widget(
                Paragraph::new(feed_line(entry, inner_w, false)),
                Rect {
                    x: inner_x,
                    y: y + row,
                    width: inner_w,
                    height: 1,
                },
            );
            row += 1;
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("storebot ", Style::new().fg(DIM)),
            Span::styled(health, Style::new().fg(health_color)),
            Span::styled("   f FULL FEED", Style::new().fg(DIM)),
        ])),
        Rect {
            x: inner_x,
            y: area.bottom().saturating_sub(1),
            width: inner_w,
            height: 1,
        },
    );
    let _ = now;
}

fn draw_activity(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let now_ms = feed_now_ms();
    let [header, rule, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(area);
    let scope = feed_scope(app);
    let mut left = vec![
        Span::styled(
            " ACTIVITY",
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::new().fg(DIM)),
        Span::styled(scope, Style::new().fg(TEXT)),
    ];
    let (health, health_color) = feed_health(app);
    left.push(Span::styled(
        format!("   ● {health}"),
        Style::new().fg(health_color),
    ));
    let unread = app.activity_unread();
    if unread > 0 {
        left.push(Span::styled(
            format!("   {unread} NEW"),
            Style::new().fg(MONEY),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), header);
    let ranges = Range::ALL;
    let mut right = Vec::new();
    for range in ranges {
        let active = app.activity.range == range;
        right.push(Span::styled(
            format!(" {} ", range.label()),
            Style::new()
                .fg(if active { BRIGHT } else { DIM })
                .bg(if active { SURFACE } else { BG })
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    }
    right.push(Span::styled("   ", Style::new()));
    for filter in Filter::ALL {
        let active = app.activity.filter == filter;
        right.push(Span::styled(
            format!(" {} ", filter.label()),
            Style::new()
                .fg(if active { BRIGHT } else { DIM })
                .add_modifier(if active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(Alignment::Right),
        header,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(rule.width as usize),
            Style::new().fg(BORDER_STRONG),
        )),
        rule,
    );
    let entries = app.activity_entries(app.activity.range);
    let selected = app.activity.selected_index(&entries);
    if area.width >= 150 {
        let [list, gap, detail_area] = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Length(2),
            Constraint::Percentage(38),
        ])
        .areas(body);
        draw_activity_list(frame, list, &entries, selected, now_ms);
        frame.render_widget(Block::default().style(Style::new().bg(SURFACE)), gap);
        draw_activity_detail(frame, detail_area, entries.get(selected).copied(), now_ms);
    } else if body.height >= 12 {
        let [list, gap, detail_area] = Layout::vertical([
            Constraint::Fill(3),
            Constraint::Length(1),
            Constraint::Fill(2),
        ])
        .areas(body);
        draw_activity_list(frame, list, &entries, selected, now_ms);
        draw_activity_detail(frame, detail_area, entries.get(selected).copied(), now_ms);
        let _ = gap;
    } else {
        draw_activity_list(frame, body, &entries, selected, now_ms);
    }
    let _ = now;
}

fn draw_activity_list(
    frame: &mut Frame,
    area: Rect,
    entries: &[&Entry],
    selected: usize,
    now_ms: i64,
) {
    let visible = area.height as usize;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    let mut row = 0usize;
    let mut last_day = String::new();
    for (index, entry) in entries.iter().enumerate().skip(offset) {
        if row >= visible {
            break;
        }
        let day = feed_day(entry);
        if day != last_day {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    feed_day_label(entry, now_ms),
                    Style::new().fg(DIM).add_modifier(Modifier::BOLD),
                ))),
                Rect {
                    x: area.x,
                    y: area.y + row as u16,
                    width: area.width,
                    height: 1,
                },
            );
            row += 1;
            last_day = day;
            if row >= visible {
                break;
            }
        }
        let background = if index == selected { SELECTION } else { BG };
        frame.render_widget(
            Paragraph::new(feed_line(entry, area.width, index == selected))
                .style(Style::new().bg(background)),
            Rect {
                x: area.x,
                y: area.y + row as u16,
                width: area.width,
                height: 1,
            },
        );
        row += 1;
    }
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No activity in this range.",
                Style::new().fg(DIM),
            ))),
            area,
        );
    }
}

fn draw_activity_detail(frame: &mut Frame, area: Rect, entry: Option<&Entry>, now_ms: i64) {
    let Some(entry) = entry else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Select an activity item.",
                Style::new().fg(DIM),
            ))),
            area,
        );
        return;
    };
    let source = if entry.source.is_empty() {
        String::new()
    } else {
        format!(" · {}", entry.source.to_uppercase())
    };
    let kind = match entry.kind() {
        EntryKind::Purchase => format!("PURCHASE{source}"),
        EntryKind::Review => format!("WRITTEN REVIEW{source}"),
        EntryKind::Rating => format!("RATING ONLY{source}"),
        EntryKind::Unknown => format!("ACTIVITY{source}"),
    };
    let mut lines = vec![Line::from(Span::styled(
        kind,
        Style::new().fg(DIM).add_modifier(Modifier::BOLD),
    ))];
    match entry.kind() {
        EntryKind::Purchase => {
            if let Some(purchase) = &entry.purchase {
                let (value, color) = feed_value(entry);
                lines.push(Line::from(Span::styled(
                    value,
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                )));
                if !purchase.product.is_empty() {
                    lines.push(Line::from(Span::styled(
                        purchase.product.clone(),
                        Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
                    )));
                }
                lines.push(Line::from(Span::styled(
                    entry.app.clone(),
                    Style::new().fg(TEXT),
                )));
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} · {} · {}",
                        purchase.event.replace('_', " "),
                        entry.country,
                        feed_stamp(entry)
                    ),
                    Style::new().fg(DIM),
                )));
                if purchase.first_ever || purchase.first_today {
                    let mut tags = Vec::new();
                    if purchase.first_ever {
                        tags.push("FIRST EVER");
                    }
                    if purchase.first_today {
                        tags.push("FIRST TODAY");
                    }
                    lines.push(Line::from(Span::styled(
                        tags.join(" · "),
                        Style::new().fg(WARN),
                    )));
                }
                if purchase.estimated {
                    lines.push(Line::from(Span::styled(
                        "App-level delta; no per-sale detail.",
                        Style::new().fg(DIM),
                    )));
                }
                if !purchase.transaction_id.is_empty() {
                    lines.push(Line::from(Span::styled(
                        format!("transaction {}", purchase.transaction_id),
                        Style::new().fg(DIM),
                    )));
                }
            }
        }
        EntryKind::Review => {
            let Some(review) = &entry.review else {
                return;
            };
            let (stars, color) = feed_value(entry);
            lines.push(Line::from(Span::styled(
                stars,
                Style::new().fg(color).add_modifier(Modifier::BOLD),
            )));
            if !review.title.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("“{}”", review.title),
                    Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
                )));
            }
            lines.push(Line::from(Span::styled(
                format!(
                    "{} · {} · {}",
                    entry.app,
                    entry.country,
                    age_label(now_ms, entry.received_at)
                ),
                Style::new().fg(DIM),
            )));
            if !review.author.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("— {}", review.author),
                    Style::new().fg(DIM),
                )));
            }
            if !review.body.is_empty() {
                lines.push(Line::default());
                for line in review.body.lines().take(20) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::new().fg(TEXT),
                    )));
                }
            }
        }
        EntryKind::Rating => {
            if let Some(rating) = &entry.rating {
                let (stars, color) = feed_value(entry);
                lines.push(Line::from(Span::styled(
                    stars,
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} · {} · {}",
                        entry.app,
                        entry.country,
                        age_label(now_ms, entry.received_at)
                    ),
                    Style::new().fg(DIM),
                )));
                lines.push(Line::from(Span::styled(
                    format!(
                        "{} new rating · average {:.1}",
                        rating.count, rating.average
                    ),
                    Style::new().fg(TEXT),
                )));
            }
        }
        EntryKind::Unknown => {}
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn age_label(now_ms: i64, then_ms: i64) -> String {
    let seconds = (now_ms - then_ms).max(0) / 1000;
    match seconds {
        0..=2 => "now".to_string(),
        3..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        _ => format!("{}h ago", seconds / 3600),
    }
}

fn truncate_display(text: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    if max == 0 {
        return String::new();
    }
    let mut width = 0;
    let mut out = String::new();
    for character in text.chars() {
        let char_width = character.width().unwrap_or(0);
        if width + char_width > max.saturating_sub(1) {
            out.push('…');
            break;
        }
        width += char_width;
        out.push(character);
    }
    out
}

fn draw_cards(app: &mut App, frame: &mut Frame, area: Rect, now: Instant) {
    if app.cards.is_empty() {
        let lines = match &app.status {
            Status::Error(failure) if !app.has_data() => error_lines(failure),
            _ => vec![Line::from(Span::styled(
                "connecting to RevenueCat…",
                Style::new().fg(DIM),
            ))],
        };
        let height = wrapped_height(&lines, area.width.saturating_sub(4)).min(area.height);
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .alignment(Alignment::Center),
            center_block(area, height),
        );
        return;
    }

    let [_, grid] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let [_, grid, _] = Layout::horizontal([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(grid);

    let visible = app.visible_indices();
    let columns = match app.ui_scale {
        1 => {
            if grid.width >= 90 {
                3
            } else {
                1
            }
        }
        2 => {
            if grid.width >= 70 {
                2
            } else {
                1
            }
        }
        3 => 1,
        _ => column_count(grid.width, grid.height, visible.len()),
    };
    let rows = visible.len().div_ceil(columns.max(1));
    if rows == 0 || grid.height == 0 {
        return;
    }

    let row_areas = Layout::vertical(vec![Constraint::Fill(1); rows])
        .spacing(1)
        .split(grid);
    let currency = app.currency.clone();
    let partial = app.scope == 0 && app.partial();
    let scoped_state = app
        .scope
        .checked_sub(1)
        .and_then(|index| app.views.get(index))
        .map(|view| view.state(now, app.stale_after()));
    let focus_visible = app.focus_visible(now);
    app.rects.clear();
    for (position, index) in visible.iter().copied().enumerate() {
        let row = position / columns;
        let column = position % columns;
        if row >= row_areas.len() {
            break;
        }
        let cells = Layout::horizontal(vec![Constraint::Fill(1); columns])
            .spacing(1)
            .split(row_areas[row]);
        if let Some(cell) = cells.get(column)
            && let Some(card) = app.cards.get(index)
        {
            draw_card(
                frame,
                card,
                *cell,
                &mut app.rects,
                now,
                CardView {
                    currency: &currency,
                    partial,
                    scoped_state,
                    focused: focus_visible && app.focus == index,
                },
            );
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueSize {
    Big,
    Medium,
    Compact,
}

struct CardView<'a> {
    currency: &'a str,
    partial: bool,
    scoped_state: Option<RowState>,
    focused: bool,
}

fn draw_card(
    frame: &mut Frame,
    card: &Card,
    area: Rect,
    rects: &mut HashMap<String, Rect>,
    now: Instant,
    view: CardView<'_>,
) {
    let CardView {
        currency,
        partial,
        scoped_state,
        focused,
    } = view;
    rects.insert(card.id.clone(), area);

    let accent = accent(&card.id);
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled("● ", Style::new().fg(accent)),
        Span::styled(
            card.name.to_uppercase(),
            Style::new().fg(DIM).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);
    let border = if focused { BRIGHT } else { BORDER };
    let block = Block::bordered()
        .border_style(Style::new().fg(border))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let value = card.value.value(now);
    let text = number_text(value);
    let big = big_digits(&text);
    let medium = medium_digits(&text);
    let fits = |rows: &Option<Vec<String>>| {
        rows.as_ref().is_some_and(|rows| {
            rows.first().map_or(0, |row| row.chars().count()) + 2 <= inner.width as usize
        })
    };
    let size = if fits(&big) && inner.height >= 16 && inner.width >= 22 {
        ValueSize::Big
    } else if fits(&medium) && inner.height >= 12 && inner.width >= 16 {
        ValueSize::Medium
    } else {
        ValueSize::Compact
    };
    let value_height = match size {
        ValueSize::Big => 11,
        ValueSize::Medium => 7,
        ValueSize::Compact => 3,
    }
    .min(inner.height);
    let [value_area, chart_area] =
        Layout::vertical([Constraint::Length(value_height), Constraint::Fill(1)]).areas(inner);

    let mut meta = vec![Span::raw("  ")];
    if size != ValueSize::Compact
        && let Some(symbol) = money_symbol(card, currency)
    {
        meta.push(Span::styled(format!("{symbol} "), Style::new().fg(DIM)));
    }
    meta.push(Span::styled(card_period(card), Style::new().fg(DIM)));
    if partial {
        meta.push(Span::styled("*", Style::new().fg(WARN)));
    }
    if let Some(chip) = scoped_state.and_then(|state| match state {
        RowState::Failed => Some(("failed · last known", BAD)),
        RowState::Stale => Some(("stale", WARN)),
        RowState::Live => None,
    }) {
        meta.push(Span::styled("   ·   ", Style::new().fg(BORDER)));
        meta.push(Span::styled(chip.0, Style::new().fg(chip.1)));
    }
    if let Some(spans) = trend_spans(card, card.trend, currency, inner.width < 46) {
        meta.push(Span::styled("   ·   ", Style::new().fg(BORDER)));
        meta.extend(spans);
    }

    match size {
        ValueSize::Big | ValueSize::Medium => {
            let rows = if size == ValueSize::Big {
                big.expect("checked above")
            } else {
                medium.expect("checked above")
            };
            let mut lines = Vec::with_capacity(rows.len() + 3);
            lines.push(Line::default());
            for row in &rows {
                lines.push(Line::from(Span::styled(
                    format!("  {row}"),
                    Style::new().fg(BRIGHT),
                )));
            }
            if size == ValueSize::Big {
                lines.push(Line::default());
            }
            lines.push(Line::from(meta));
            frame.render_widget(Paragraph::new(lines), value_area);
        }
        ValueSize::Compact => {
            let value_line = vec![
                Span::raw("  "),
                Span::styled(
                    format_metric(value, &card.unit, currency),
                    Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
                ),
            ];
            frame.render_widget(
                Paragraph::new(vec![
                    Line::default(),
                    Line::from(value_line),
                    Line::from(meta),
                ]),
                value_area,
            );
        }
    }

    let values = card.series.sample(now);
    let chart = Rect {
        x: chart_area.x.saturating_add(2),
        y: chart_area.y.saturating_add(1),
        width: chart_area.width.saturating_sub(4),
        height: chart_area.height.saturating_sub(1),
    };
    if values.len() >= 2 && chart.height >= 1 {
        let style = if matches!(card.id.as_str(), "mrr" | "active_subscriptions") {
            ChartStyle::Band
        } else {
            ChartStyle::Area
        };
        draw_chart(frame, chart, &values, accent, style, false);
    } else if chart.height >= 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no chart data",
                Style::new().fg(DIM),
            ))),
            chart,
        );
    }
}

fn money_symbol(card: &Card, currency: &str) -> Option<String> {
    if card.unit == "$" || card.unit.eq_ignore_ascii_case(currency) {
        Some(currency_symbol(currency).trim().to_string())
    } else {
        None
    }
}

fn card_period(card: &Card) -> String {
    match card.id.as_str() {
        "active_trials" | "active_subscriptions" => "Right now".to_string(),
        "mrr" => "Monthly recurring".to_string(),
        "active_users" => "Active in 28 days".to_string(),
        _ => period_label(&card.period),
    }
}

fn trend_spans(
    card: &Card,
    trend: Option<Trend>,
    currency: &str,
    short: bool,
) -> Option<Vec<Span<'static>>> {
    let trend = trend?;
    if trend.ratio.is_finite() && trend.ratio.abs() < 0.005 && trend.change.abs() < 0.5 {
        return Some(vec![Span::styled(
            if short { "flat" } else { "flat vs last week" },
            Style::new().fg(DIM),
        )]);
    }
    let (arrow, color) = if trend.change >= 0.0 {
        ("▲", GOOD)
    } else {
        ("▼", BAD)
    };
    let value = if !trend.ratio.is_finite() {
        match card.id.as_str() {
            "revenue" | "mrr" => format_metric(trend.change.abs().round(), "$", currency),
            _ => format!("{}", trend.change.abs().round() as i64),
        }
    } else {
        match card.id.as_str() {
            "revenue" => format_metric(trend.change.abs().round(), "$", currency),
            "new_customers" => format!("{}", trend.change.abs().round() as i64),
            _ => format!("{:.0}%", trend.ratio.abs() * 100.0),
        }
    };
    let mut spans = vec![Span::styled(
        format!("{arrow} {value}"),
        Style::new().fg(color),
    )];
    if !short {
        spans.push(Span::styled(" vs last week", Style::new().fg(DIM)));
    }
    Some(spans)
}

fn number_text(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let value = value.abs();
    let rounded = value.round();
    if (value - rounded).abs() < 0.05 {
        format!("{sign}{}", rounded as i64)
    } else if value < 100.0 {
        format!("{sign}{value:.1}")
    } else {
        format!("{sign}{}", rounded as i64)
    }
}

const BIG_FONT: [[&str; 7]; 12] = [
    [
        "█████",
        "█   █",
        "█   █",
        "█   █",
        "█   █",
        "█   █",
        "█████",
    ],
    [
        "   █ ",
        "  ██ ",
        "   █ ",
        "   █ ",
        "   █ ",
        "   █ ",
        "  ███",
    ],
    [
        "█████",
        "    █",
        "    █",
        "█████",
        "█    ",
        "█    ",
        "█████",
    ],
    [
        "█████",
        "    █",
        "    █",
        "█████",
        "    █",
        "    █",
        "█████",
    ],
    [
        "█   █",
        "█   █",
        "█   █",
        "█████",
        "    █",
        "    █",
        "    █",
    ],
    [
        "█████",
        "█    ",
        "█    ",
        "█████",
        "    █",
        "    █",
        "█████",
    ],
    [
        "█████",
        "█    ",
        "█    ",
        "█████",
        "█   █",
        "█   █",
        "█████",
    ],
    [
        "█████",
        "    █",
        "    █",
        "    █",
        "    █",
        "    █",
        "    █",
    ],
    [
        "█████",
        "█   █",
        "█   █",
        "█████",
        "█   █",
        "█   █",
        "█████",
    ],
    [
        "█████",
        "█   █",
        "█   █",
        "█████",
        "    █",
        "    █",
        "█████",
    ],
    ["  ", "  ", "  ", "  ", "  ", "  ", "██"],
    ["   ", "   ", "   ", "█████", "   ", "   ", "   "],
];

const MEDIUM_FONT: [[&str; 5]; 12] = [
    ["███", "█ █", "█ █", "█ █", "███"],
    [" █ ", "██ ", " █ ", " █ ", "███"],
    ["███", "  █", "███", "█  ", "███"],
    ["███", "  █", "███", "  █", "███"],
    ["█ █", "█ █", "███", "  █", "  █"],
    ["███", "█  ", "███", "  █", "███"],
    ["███", "█  ", "███", "█ █", "███"],
    ["███", "  █", "  █", "  █", "  █"],
    ["███", "█ █", "███", "█ █", "███"],
    ["███", "█ █", "███", "  █", "███"],
    [" ", " ", " ", " ", "█"],
    ["   ", "   ", "███", "   ", "   "],
];

fn glyph_index(character: char) -> Option<usize> {
    match character {
        '0'..='9' => Some((character as u8 - b'0') as usize),
        '.' => Some(10),
        '-' => Some(11),
        _ => None,
    }
}

fn big_digits(text: &str) -> Option<Vec<String>> {
    let glyphs: Vec<usize> = text.chars().map(glyph_index).collect::<Option<Vec<_>>>()?;
    if glyphs.is_empty() {
        return None;
    }
    let mut rows = vec![String::new(); 7];
    for (position, glyph) in glyphs.iter().enumerate() {
        for (row, line) in rows.iter_mut().enumerate() {
            if position > 0 {
                line.push_str("  ");
            }
            line.push_str(BIG_FONT[*glyph][row]);
        }
    }
    Some(rows)
}

fn medium_digits(text: &str) -> Option<Vec<String>> {
    let glyphs: Vec<usize> = text.chars().map(glyph_index).collect::<Option<Vec<_>>>()?;
    if glyphs.is_empty() {
        return None;
    }
    let mut rows = vec![String::new(); 5];
    for (position, glyph) in glyphs.iter().enumerate() {
        for (row, line) in rows.iter_mut().enumerate() {
            if position > 0 {
                line.push(' ');
            }
            line.push_str(MEDIUM_FONT[*glyph][row]);
        }
    }
    Some(rows)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChartStyle {
    Area,
    Band,
}

fn draw_chart(
    frame: &mut Frame,
    area: Rect,
    values: &[f64],
    accent: Color,
    style: ChartStyle,
    guides: bool,
) {
    let rows = area.height as usize;
    if area.width == 0 || rows == 0 || values.len() < 2 {
        return;
    }
    let sub_rows = rows * 2;
    let fill = shade(accent, if style == ChartStyle::Band { 0.22 } else { 0.4 });
    let (low, high) = match style {
        ChartStyle::Band => {
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let pad = ((max - min) * 0.5).max(max.abs() * 0.10).max(0.5);
            (min - pad, max + pad)
        }
        ChartStyle::Area => {
            let min = values.iter().copied().fold(0.0_f64, f64::min);
            let max = values.iter().copied().fold(0.0_f64, f64::max);
            if (max - min).abs() < f64::EPSILON {
                (min - 1.0, min + 1.0)
            } else {
                (min.min(0.0), if max <= 0.0 { 0.0 } else { max * 1.08 })
            }
        }
    };
    let span = (high - low).max(f64::EPSILON);
    let zero = if style == ChartStyle::Area {
        (((0.0 - low) / span) * (sub_rows.saturating_sub(1)) as f64)
            .round()
            .clamp(0.0, sub_rows as f64 - 1.0) as usize
    } else {
        0
    };

    if guides {
        for fraction in [0.25, 0.5, 0.75] {
            let x = ((area.width as f64 - 1.0) * fraction).round() as u16;
            for y in area.top()..area.bottom() {
                let cell = &mut frame.buffer_mut()[(area.x + x, y)];
                cell.set_symbol("│").set_fg(shade(BORDER, 1.4));
            }
        }
    }

    let width = area.width.saturating_sub(1).max(1) as f64;
    for column in 0..area.width {
        let position = column as f64 / width * (values.len() - 1) as f64;
        let value = sample(values, position);
        let sub = (((value - low) / span) * (sub_rows.saturating_sub(1)) as f64)
            .round()
            .clamp(0.0, sub_rows as f64 - 1.0) as usize;
        let (from, to) = match style {
            ChartStyle::Band => (0, sub),
            ChartStyle::Area => (zero.min(sub), zero.max(sub)),
        };
        paint_column(frame.buffer_mut(), area, column, from, to, fill);
        paint_edge(frame.buffer_mut(), area, column, sub, accent);
    }

    if guides && style == ChartStyle::Area && low < 0.0 && high > 0.0 {
        let y = area.bottom() - 1 - (zero / 2) as u16;
        for x in area.left()..area.right() {
            let cell = &mut frame.buffer_mut()[(x, y)];
            if cell.symbol() == " " {
                cell.set_symbol("·").set_fg(DIM);
            }
        }
    }
}

fn paint_column(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    column: u16,
    from: usize,
    to: usize,
    fill: Color,
) {
    let rows = area.height as usize;
    for row in 0..rows {
        let lower = row * 2;
        let upper = lower + 1;
        let lower_filled = (from..=to).contains(&lower);
        let upper_filled = (from..=to).contains(&upper);
        if !lower_filled && !upper_filled {
            continue;
        }
        let symbol = match (lower_filled, upper_filled) {
            (true, true) => "█",
            (true, false) => "▄",
            _ => "▀",
        };
        let y = area.bottom() - 1 - row as u16;
        buffer[(area.x + column, y)].set_symbol(symbol).set_fg(fill);
    }
}

fn paint_edge(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    column: u16,
    sub: usize,
    accent: Color,
) {
    let row = sub / 2;
    if row >= area.height as usize {
        return;
    }
    let y = area.bottom() - 1 - row as u16;
    let cell = &mut buffer[(area.x + column, y)];
    if cell.symbol() == " " {
        cell.set_symbol(if sub.is_multiple_of(2) { "▄" } else { "▀" });
    }
    cell.set_fg(accent);
}

fn sample(values: &[f64], position: f64) -> f64 {
    let low = position.floor().max(0.0) as usize;
    let low = low.min(values.len() - 1);
    let high = (low + 1).min(values.len() - 1);
    let fraction = (position - low as f64).clamp(0.0, 1.0);
    values[low] * (1.0 - fraction) + values[high] * fraction
}

fn draw_footer(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(38)]).areas(area);

    if app.activity.open {
        let (health, color) = feed_health(app);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " j/k move   Tab filter   [ ] range   f close   ? help",
                Style::new().fg(DIM),
            ))),
            left,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("storebot {health} "),
                Style::new().fg(color),
            )))
            .alignment(Alignment::Right),
            right,
        );
        return;
    }

    let hints = now.saturating_duration_since(app.last_input) < Duration::from_secs(10);
    let left_line = if let Some(notice) = app.notice_active(now) {
        Line::from(Span::styled(
            format!(
                " {}",
                truncate(notice, area.width.saturating_sub(2) as usize)
            ),
            Style::new().fg(WARN),
        ))
    } else if hints {
        let keys = if app.demo {
            " q quit   r refresh   p projects   f activity   ? help"
        } else {
            " q quit   r refresh   p projects   f activity   Enter detail   ? help"
        };
        Line::from(Span::styled(keys, Style::new().fg(DIM)))
    } else if app.stale(now) {
        Line::from(Span::styled(
            format!(
                " stale · last update {} · r retry",
                app.last_updated
                    .map_or("never".to_string(), |then| ago(now, then))
            ),
            Style::new().fg(WARN),
        ))
    } else if let Some((_, view)) = app.failed_views().into_iter().next() {
        let headline = view
            .failure
            .as_ref()
            .map_or("unknown error".to_string(), |failure| failure.headline());
        Line::from(Span::styled(
            format!(
                " ! {} of {} projects failed — {}: {}",
                app.failed_count(),
                app.project_count(),
                view.label,
                truncate(&headline, 70)
            ),
            Style::new().fg(WARN),
        ))
    } else if app.demo {
        Line::from(Span::styled(
            " demo data · no API calls",
            Style::new().fg(DIM),
        ))
    } else {
        Line::default()
    };
    frame.render_widget(Paragraph::new(left_line), left);

    let mut parts = Vec::new();
    if let Some(updated) = app.last_updated {
        parts.push(format!("updated {}", ago(now, updated)));
    }
    if !matches!(app.status, Status::Fetching) {
        parts.push(format!(
            "next in {}",
            countdown(app.next_refresh.saturating_duration_since(now))
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{} ", parts.join("   ·   ")),
            Style::new().fg(DIM),
        )))
        .alignment(Alignment::Right),
        right,
    );
}

fn draw_wall_hint(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    if now.saturating_duration_since(app.last_input) > Duration::from_secs(4) {
        return;
    }
    let line = Line::from(Span::styled(
        " p projects   r refresh   ? help",
        Style::new().fg(DIM),
    ));
    frame.render_widget(
        Paragraph::new(line).alignment(Alignment::Right),
        Rect {
            y: area.bottom().saturating_sub(1),
            height: 1,
            ..area
        },
    );
}

fn draw_picker(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let Some(picker) = &app.picker else {
        return;
    };
    let entries = app.picker_entries();
    let total = app.project_count();
    let width = area.width.saturating_sub(10).clamp(40, 82);
    let height = ((total as u16) + 8)
        .min(area.height.saturating_sub(4))
        .max(12);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(BORDER_STRONG))
        .style(Style::new().bg(SURFACE))
        .title(Line::from(vec![
            Span::styled(
                " Projects ",
                Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if picker.query.trim().is_empty() {
                    format!("{total} ")
                } else {
                    format!("{} / {total} ", entries.len())
                },
                Style::new().fg(DIM),
            ),
        ]));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let [query_area, list_area, help_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  / ", Style::new().fg(DIM)),
            Span::styled(picker.query.clone(), Style::new().fg(BRIGHT)),
            Span::styled("▌", Style::new().fg(BRIGHT)),
        ])),
        query_area,
    );

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No projects match that filter.",
                Style::new().fg(DIM),
            ))),
            list_area,
        );
    }

    let visible = list_area.height.saturating_sub(1) as usize;
    let offset = picker.selected.saturating_sub(visible.saturating_sub(1));
    for (index, entry) in entries.iter().enumerate().skip(offset).take(visible) {
        let selected = index == picker.selected;
        let row = Rect {
            x: list_area.x,
            y: list_area.y + (index - offset) as u16,
            width: list_area.width,
            height: 1,
        };
        let (dot, dot_color) = match entry.state {
            RowState::Live => ("●", GOOD),
            RowState::Stale => ("·", WARN),
            RowState::Failed => ("!", BAD),
        };
        let background = if selected { SELECTION } else { BG };
        let foreground = if selected { BRIGHT } else { TEXT };
        let line = Line::from(vec![
            Span::styled(
                if selected { " ▶ " } else { "   " },
                Style::new()
                    .fg(accent_for_state(entry.state))
                    .bg(background),
            ),
            Span::styled(format!("{dot} "), Style::new().fg(dot_color).bg(background)),
            Span::styled(
                format!("{:<30}", truncate(&entry.label, 30)),
                Style::new()
                    .fg(foreground)
                    .bg(background)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", truncate(&entry.detail, 24)),
                Style::new().fg(DIM).bg(background),
            ),
        ]);
        frame.render_widget(Paragraph::new(line).style(Style::new().bg(background)), row);
    }

    let hidden = entries.len().saturating_sub(offset + visible);
    if hidden > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  ↓ {hidden} more"),
                Style::new().fg(DIM),
            ))),
            Rect {
                y: list_area.bottom().saturating_sub(1),
                height: 1,
                ..list_area
            },
        );
    }

    let help = if let Some(view) = app.confirm_remove() {
        let label = entries
            .iter()
            .find(|entry| entry.view == Some(view))
            .map_or("this project", |entry| entry.label.as_str());
        format!("  Press Ctrl+D again to remove {label} · any other key cancels")
    } else {
        "  ↑/↓  Enter  type filter  ^F pin  ^E edit  ^D remove  Esc".to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate(&help, inner.width as usize),
            Style::new().fg(if app.confirm_remove().is_some() {
                BAD
            } else {
                DIM
            }),
        ))),
        help_area,
    );
    let _ = now;
}

fn accent_for_state(state: RowState) -> Color {
    match state {
        RowState::Live => accent(""),
        RowState::Stale => WARN,
        RowState::Failed => BAD,
    }
}

fn draw_health(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let rows = app.health_rows();
    let selected = app.health.unwrap_or(0);
    let width = area.width.saturating_sub(12).clamp(50, 110);
    let height = ((rows.len() as u16) + 5)
        .min(area.height.saturating_sub(4))
        .max(8);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(BORDER_STRONG))
        .style(Style::new().bg(SURFACE))
        .title(Line::from(vec![
            Span::styled(
                " Project health ",
                Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}/{} live ", app.live_count(), app.project_count()),
                Style::new().fg(if app.partial() { WARN } else { DIM }),
            ),
        ]));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [list_area, help_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);

    let visible = list_area.height as usize;
    let offset = selected.saturating_sub(visible.saturating_sub(1));
    for (index, row) in rows.iter().enumerate().skip(offset).take(visible) {
        let highlighted = index == selected;
        let background = if highlighted { SELECTION } else { BG };
        let (dot, color) = match row.state {
            RowState::Live => ("●", GOOD),
            RowState::Stale => ("·", WARN),
            RowState::Failed => ("!", BAD),
        };
        let detail = if let Some(failure) = &row.failure {
            format!(
                "{} · {}",
                failure.headline(),
                row.last_success
                    .map_or("never".to_string(), |then| ago(now, then))
            )
        } else {
            format!(
                "{} · updated {}",
                row.detail,
                row.last_success
                    .map_or("never".to_string(), |then| ago(now, then))
            )
        };
        let line = Line::from(vec![
            Span::styled(
                if highlighted { " ▶ " } else { "   " },
                Style::new().fg(color).bg(background),
            ),
            Span::styled(format!("{dot} "), Style::new().fg(color).bg(background)),
            Span::styled(
                format!("{:<28}", truncate(&row.label, 28)),
                Style::new()
                    .fg(BRIGHT)
                    .bg(background)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate(&detail, inner.width.saturating_sub(36) as usize),
                Style::new()
                    .fg(if row.failure.is_some() { BAD } else { DIM })
                    .bg(background),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(background)),
            Rect {
                x: list_area.x,
                y: list_area.y + (index - offset) as u16,
                width: list_area.width,
                height: 1,
            },
        );
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  ↑/↓ move · r retry project · R retry all · Enter open · Esc close",
            Style::new().fg(DIM),
        ))),
        help_area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(12).clamp(50, 100);
    let height = 28.min(area.height.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(BORDER_STRONG))
        .style(Style::new().bg(SURFACE))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("keys and data", Style::new().fg(DIM)),
        ]));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let lines = vec![
        dim_line("  Dashboard"),
        key_line("q / Esc", "quit"),
        key_line("r", "refresh now (forces charts)"),
        key_line("p", "project picker"),
        key_line("f", "open the activity feed"),
        key_line("← → / ↑ ↓ / Tab", "move card focus"),
        key_line("Enter", "metric detail"),
        key_line("!", "project health"),
        key_line("w", "wall mode (no chrome)"),
        key_line("x", "export the current scope as CSV"),
        key_line("?", "this help"),
        key_line("+ / -", "UI scale: auto, 3, 2 or 1 cards per page"),
        key_line("[ / ]", "previous / next page"),
        Line::default(),
        dim_line("  Detail"),
        key_line("Esc", "back"),
        key_line("↑ / ↓", "previous / next metric"),
        key_line("t", "7 / 28 / 90 days"),
        key_line("j / k", "move the project table"),
        key_line("Enter", "scope the dashboard to the selected project"),
        Line::default(),
        dim_line("  Activity"),
        key_line("f / Esc / q", "close activity"),
        key_line("j / k", "move selection"),
        key_line("Tab", "all / purchases / reviews"),
        key_line("[ / ]", "24h / 7d / 28d"),
        key_line("g / G", "first / last"),
        Line::default(),
        dim_line("  Cards: RevenueCat overview values. Sparklines: 28 daily points."),
        dim_line("  Trends: last 7 days vs the previous 7. * marks a partial total."),
        dim_line("  UI scale, page, wall, pins and the last project persist in config.toml."),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn key_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("    {key:<20}"), Style::new().fg(BRIGHT)),
        Span::styled(description.to_string(), Style::new().fg(DIM)),
    ])
}

fn draw_detail(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let Some(detail) = &app.detail else {
        return;
    };
    let Some(card) = app.cards.iter().find(|card| card.id == detail.metric_id) else {
        return;
    };
    let area = Rect {
        x: area.x + 2,
        width: area.width.saturating_sub(4),
        ..area
    };
    let [_, header, stats, rule, chart_area, _, table_area, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(2),
        Constraint::Length(1),
        Constraint::Fill(3),
        Constraint::Length(1),
    ])
    .areas(area);

    let mut header_spans = vec![
        Span::styled("← ", Style::new().fg(DIM)),
        Span::styled(
            card.name.to_uppercase(),
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("    ", Style::new()),
        Span::styled(
            format_metric(card.value.value(now), &card.unit, &app.currency),
            Style::new().fg(BRIGHT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("   {}", card_period(card)), Style::new().fg(DIM)),
    ];
    if let Some(spans) = trend_spans(card, card.trend, &app.currency, false) {
        header_spans.push(Span::styled("   ·   ", Style::new().fg(BORDER)));
        header_spans.extend(spans);
    }
    let window_line = Line::from(
        [Window::D7, Window::D28, Window::D90]
            .iter()
            .flat_map(|window| {
                let active = detail.window == *window;
                [
                    Span::styled(
                        format!(" {}d ", window.days()),
                        Style::new()
                            .fg(if active { BG } else { DIM })
                            .bg(if active { accent(&card.id) } else { BG }),
                    ),
                    Span::raw(" "),
                ]
            })
            .collect::<Vec<_>>(),
    );
    let [header_left, header_right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(22)]).areas(header);
    frame.render_widget(Paragraph::new(Line::from(header_spans)), header_left);
    frame.render_widget(
        Paragraph::new(window_line).alignment(Alignment::Right),
        header_right,
    );

    let values = app.detail_values();
    let stats_line = if values.is_empty() {
        Line::from(Span::styled(
            if detail.loading {
                "loading the selected window…".to_string()
            } else if let Some(error) = &detail.error {
                format!(
                    "{} — {}",
                    error.headline(),
                    error.detail().unwrap_or_default()
                )
            } else {
                "no chart data for this window".to_string()
            },
            Style::new().fg(if detail.error.is_some() { BAD } else { DIM }),
        ))
    } else {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        Line::from(vec![
            Span::styled(
                format!("{}   ·   ", detail.window.label()),
                Style::new().fg(DIM),
            ),
            Span::styled(
                format!(
                    "min {}   ·   max {}   ·   avg {}",
                    format_metric(min, &card.unit, &app.currency),
                    format_metric(max, &card.unit, &app.currency),
                    format_metric(avg, &card.unit, &app.currency)
                ),
                Style::new().fg(DIM),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(stats_line), stats);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(BORDER),
        )),
        rule,
    );

    if values.len() >= 2 {
        let style = if matches!(card.id.as_str(), "mrr" | "active_subscriptions") {
            ChartStyle::Band
        } else {
            ChartStyle::Area
        };
        let [plot, dates] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(chart_area);
        draw_chart(frame, plot, &values, accent(&card.id), style, true);
        draw_dates(frame, dates, values.len());
    } else if chart_area.height > 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                if detail.loading {
                    "loading…"
                } else {
                    "no chart data"
                },
                Style::new().fg(DIM),
            ))),
            center_block(chart_area, 1),
        );
    }

    let rows = app.detail_rows();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no per-project data",
                Style::new().fg(DIM),
            ))),
            table_area,
        );
    } else {
        let header = Line::from(vec![
            Span::styled(
                format!("{:<32}", "PROJECT"),
                Style::new().fg(DIM).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:>16}", "VALUE"), Style::new().fg(DIM)),
            Span::styled(format!("{:>8}", "SHARE"), Style::new().fg(DIM)),
            Span::styled(format!("{:>10}", "7D"), Style::new().fg(DIM)),
            Span::styled(format!("  {:<8}", "STATE"), Style::new().fg(DIM)),
        ]);
        frame.render_widget(Paragraph::new(header), table_area);
        let visible = table_area.height.saturating_sub(1) as usize;
        let selected = detail.selected.min(rows.len().saturating_sub(1));
        let offset = selected.saturating_sub(visible.saturating_sub(1));
        for (index, row) in rows.iter().enumerate().skip(offset).take(visible) {
            let highlighted = index == selected;
            let background = if highlighted { SELECTION } else { BG };
            let (trend, color) = detail_trend(row.trend);
            let state = match row.state {
                RowState::Live => ("live", GOOD),
                RowState::Stale => ("stale", WARN),
                RowState::Failed => ("failed", BAD),
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{:<32}", truncate(&row.label, 32)),
                    Style::new().fg(BRIGHT).bg(background),
                ),
                Span::styled(
                    format!(
                        "{:>16}",
                        format_metric(row.value, &card.unit, &app.currency)
                    ),
                    Style::new().fg(TEXT).bg(background),
                ),
                Span::styled(
                    format!("{:>8}", format!("{:.0}%", row.share * 100.0)),
                    Style::new().fg(DIM).bg(background),
                ),
                Span::styled(
                    format!("{trend:>10}"),
                    Style::new().fg(color).bg(background),
                ),
                Span::styled(
                    format!("  {:<8}", state.0),
                    Style::new().fg(state.1).bg(background),
                ),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(Style::new().bg(background)),
                Rect {
                    x: table_area.x,
                    y: table_area.y + 1 + (index - offset) as u16,
                    width: table_area.width,
                    height: 1,
                },
            );
        }
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Esc back   ·   ↑/↓ metric   ·   t window   ·   j/k project   ·   Enter scope   ·   r refresh",
            Style::new().fg(DIM),
        ))),
        footer,
    );
}

fn detail_trend(trend: Option<Trend>) -> (String, Color) {
    let Some(trend) = trend else {
        return ("—".to_string(), DIM);
    };
    if !trend.ratio.is_finite() {
        return ("new".to_string(), GOOD);
    }
    if trend.ratio.abs() < 0.005 {
        return ("flat".to_string(), DIM);
    }
    let arrow = if trend.change >= 0.0 { "▲" } else { "▼" };
    (
        format!("{arrow} {:.0}%", trend.ratio.abs() * 100.0),
        if trend.change >= 0.0 { GOOD } else { BAD },
    )
}

fn draw_dates(frame: &mut Frame, area: Rect, points: usize) {
    if points < 2 || area.height == 0 || area.width < 12 {
        return;
    }
    let end = chrono::Utc::now().date_naive();
    let labels = 4.min(points);
    let buffer = frame.buffer_mut();
    for step in 0..labels {
        let index = if labels == 1 {
            0
        } else {
            step * (points - 1) / (labels - 1)
        };
        let date = end
            .checked_sub_days(Days::new((points - 1 - index) as u64))
            .unwrap_or(end);
        let text = date.format("%b %d").to_string();
        let x = ((area.width as f64 - text.len() as f64) * (index as f64 / (points - 1) as f64))
            .round()
            .clamp(0.0, area.width as f64 - text.len() as f64) as u16;
        buffer.set_string(area.x + x, area.y, &text, Style::new().fg(DIM));
    }
}

pub(crate) fn format_metric(value: f64, unit: &str, currency: &str) -> String {
    if unit == "$" || unit.eq_ignore_ascii_case(currency) {
        let symbol = currency_symbol(currency);
        if value.fract().abs() < 0.005 {
            format!("{symbol}{value:.0}")
        } else {
            format!("{symbol}{value:.2}")
        }
    } else {
        format!("{}", value.round() as i64)
    }
}

fn currency_symbol(code: &str) -> String {
    match code.to_ascii_uppercase().as_str() {
        "USD" => "$".into(),
        "EUR" => "€".into(),
        "GBP" => "£".into(),
        "JPY" | "CNY" => "¥".into(),
        "KRW" => "₩".into(),
        "BRL" => "R$".into(),
        "MXN" => "MX$".into(),
        "CAD" => "C$".into(),
        "AUD" => "A$".into(),
        "NZD" => "NZ$".into(),
        "SEK" => "kr ".into(),
        "PLN" => "zł ".into(),
        other => format!("{other} "),
    }
}

fn period_label(period: &str) -> String {
    match period {
        "P0D" => "In total".to_string(),
        "P28D" => "Last 28 days".to_string(),
        other => other.to_string(),
    }
}

fn spinner(now: Instant) -> char {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let index = (now.elapsed().as_millis() / 90) as usize % FRAMES.len();
    FRAMES[index]
}

fn countdown(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn ago(now: Instant, then: Instant) -> String {
    let seconds = now.saturating_duration_since(then).as_secs();
    match seconds {
        0..=1 => "just now".to_string(),
        2..=59 => format!("{seconds}s ago"),
        60..=3599 => format!("{}m ago", seconds / 60),
        _ => format!("{}h ago", seconds / 3600),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(max.saturating_sub(1))
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    format!("{}…", &text[..cut])
}

fn center_block(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Rect {
        y: area.y + (area.height.saturating_sub(height)) / 2,
        height,
        ..area
    }
}

fn wrapped_height(lines: &[Line], width: u16) -> u16 {
    let width = width.max(1) as usize;
    lines
        .iter()
        .map(|line| {
            let len = line.width();
            len.div_ceil(width).max(1) as u16
        })
        .sum()
}

pub fn draw_import(state: &Import, frame: &mut Frame) {
    let now = Instant::now();
    let area = frame.area();
    let block = Block::bordered()
        .border_style(Style::new().fg(BORDER))
        .style(Style::new().bg(BG))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("import projects", Style::new().fg(DIM)),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [intro_area, body_area, footer_area] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let mut intro = vec![
        dim_line(
            "  Paste one v2 secret key per line, then Ctrl+D — every key is checked in one pass.",
        ),
        dim_line(
            "  Prefix a key with its project id when it lacks Project Configuration: Read: proj1abc sk_…",
        ),
        dim_line("  A key covers every app in its project: one key per project is enough."),
    ];
    if let Some(notice) = &state.notice {
        intro.push(Line::from(Span::styled(
            format!("  {notice}"),
            Style::new().fg(WARN),
        )));
    }
    frame.render_widget(Paragraph::new(intro), intro_area);

    let help = match state.stage {
        ImportStage::Editing => {
            let editor = Block::bordered()
                .border_style(Style::new().fg(BORDER))
                .title(Span::styled(
                    format!(" {} key(s) ", state.key_count()),
                    Style::new().fg(DIM),
                ));
            let inner = editor.inner(body_area);
            frame.render_widget(editor, body_area);
            let text = format!("{}▌", state.text);
            let lines = text.lines().count().max(1) as u16;
            let offset = lines.saturating_sub(inner.height);
            frame.render_widget(Paragraph::new(text).scroll((offset, 0)), inner);
            "Ctrl+D check all keys · Ctrl+U clear · paste with Ctrl+Shift+V · Esc cancel"
        }
        ImportStage::Validating => {
            let body = vec![
                Line::default(),
                Line::from(Span::styled(
                    format!(
                        "  {} checking {} key(s) with RevenueCat…",
                        spinner(now),
                        state.key_count()
                    ),
                    Style::new().fg(TEXT),
                )),
                Line::default(),
                dim_line("  One overview request per key; chart data loads after you save."),
            ];
            frame.render_widget(Paragraph::new(body), body_area);
            "please wait"
        }
        ImportStage::Results => {
            let mut body = Vec::new();
            let room = body_area.height.saturating_sub(4) as usize;
            for outcome in state.results.iter().take(room) {
                match (&outcome.project, &outcome.failure) {
                    (Some(project), _) => body.push(Line::from(vec![
                        Span::styled("  ✓ ", Style::new().fg(GOOD)),
                        Span::styled(
                            truncate(&outcome.label, 28),
                            Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("  {}", project.id), Style::new().fg(DIM)),
                        Span::styled("  metrics ✓ charts ✓", Style::new().fg(GOOD)),
                    ])),
                    (None, Some(failure)) => body.push(Line::from(vec![
                        Span::styled("  ✗ ", Style::new().fg(BAD)),
                        Span::styled(
                            format!("{:<28} ", truncate(&outcome.label, 28)),
                            Style::new().fg(TEXT),
                        ),
                        Span::styled(truncate(&failure.headline(), 42), Style::new().fg(BAD)),
                        Span::styled(
                            match outcome.charts {
                                Some(true) => "  metrics ✗",
                                Some(false) => "  charts ✗",
                                None => "",
                            },
                            Style::new().fg(BAD),
                        ),
                    ])),
                    (None, None) => {}
                }
            }
            body.push(Line::default());
            body.push(Line::from(Span::styled(
                format!(
                    "  {} ready to save · {} failed",
                    state.ready(),
                    state.failures()
                ),
                Style::new().fg(if state.failures() == 0 { GOOD } else { WARN }),
            )));
            if state.failures() > 0 {
                if let Some(failure) = state
                    .results
                    .iter()
                    .find_map(|outcome| outcome.failure.as_ref())
                {
                    for hint in failure.hints().into_iter().take(2) {
                        body.push(dim_line(&format!("  → {hint}")));
                    }
                }
                body.push(dim_line(
                    "  Esc to edit the failed lines and press Ctrl+D again, or Enter to save the rest.",
                ));
            }
            frame.render_widget(Paragraph::new(body), body_area);
            "Enter save ready projects · Esc back to the editor"
        }
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {help}"),
            Style::new().fg(DIM),
        ))),
        footer_area,
    );
}

pub fn draw_setup(setup: &Setup, frame: &mut Frame) {
    let area = frame.area();
    let block = Block::bordered()
        .border_style(Style::new().fg(BORDER))
        .style(Style::new().bg(BG))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("setup", Style::new().fg(DIM)),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (content, help) = match setup.stage {
        Stage::ListingProjects => (
            vec![
                Line::default(),
                heading("Connecting to RevenueCat"),
                Line::default(),
                Line::from(Span::styled(
                    format!("  Listing projects for {} …", redact(&setup.api_key)),
                    Style::new().fg(DIM),
                )),
                Line::from(Span::styled(
                    "  This usually takes a second or two.",
                    Style::new().fg(DIM),
                )),
                Line::default(),
            ],
            "Ctrl+C quit",
        ),
        Stage::Projects => {
            let mut lines = vec![Line::default(), heading("Pick a project"), Line::default()];
            for (index, project) in setup.projects.iter().enumerate() {
                let selected = index == setup.selected;
                let style = if selected {
                    Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(DIM)
                };
                let marker = if selected { "▶" } else { "·" };
                lines.push(Line::from(Span::styled(
                    format!("  {marker} {}  ({})", project.name, project.id),
                    style,
                )));
            }
            (lines, "↑/↓ or j/k move · Enter select · Esc back")
        }
        Stage::Connected => {
            let label = setup
                .project_name
                .clone()
                .unwrap_or_else(|| setup.project_id.clone());
            let lines = vec![
                Line::default(),
                heading("Connected"),
                Line::default(),
                Line::from(Span::styled(
                    format!("  ✓ {label}  ({})", setup.project_id),
                    Style::new().fg(GOOD).add_modifier(Modifier::BOLD),
                )),
                Line::default(),
                dim_line("  Saved when you finish; keys stay in a mode-600 file on this machine."),
                Line::default(),
                Line::from(Span::styled(
                    "  Add another project?",
                    Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
                )),
                dim_line("  y = yes (paste the next key)   ·   n / Enter = finish"),
                Line::default(),
            ];
            (lines, "y add another · Enter finish · Esc finish")
        }
        _ => setup_form(setup),
    };

    let [content_area, help_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(
        Paragraph::new(content).wrap(Wrap { trim: false }),
        content_area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {help}"),
            Style::new().fg(DIM),
        ))),
        help_area,
    );
}

fn setup_form(setup: &Setup) -> (Vec<Line<'static>>, &'static str) {
    let mut lines = vec![
        Line::default(),
        heading("Connect rc-tui to RevenueCat"),
        Line::default(),
    ];
    if setup.stage == Stage::ApiKey && !setup.api_key.trim().is_empty() {
        lines.push(dim_line(&format!(
            "  Editing {} — Enter checks metrics and charts; Ctrl+U replaces the key.",
            setup.project_name.as_deref().unwrap_or(&setup.project_id)
        )));
        lines.push(Line::default());
    }
    if setup.stage == Stage::ApiKey && setup.api_key.trim().is_empty() {
        lines.push(dim_line(
            "  1. In the dashboard: project → API keys → “+ New secret API key”.",
        ));
        lines.push(dim_line(&format!(
            "     Grant it {SCOPE_OVERVIEW} and {SCOPE_CHARTS} —"
        )));
        lines.push(dim_line(&format!(
            "     add {SCOPE_PROJECTS} to auto-pick the project."
        )));
        lines.push(dim_line(&format!(
            "  2. Paste the key below.  Docs: {DOCS_API_KEYS}"
        )));
        lines.push(dim_line(
            "  Adding several projects? Quit and run `rc-tui --import` to paste them all at once.",
        ));
        lines.push(Line::default());
    }
    let key_display = masked_key(&setup.api_key);
    let currency_display = setup.currency_value();
    lines.push(field_line(
        "API key",
        &key_display,
        setup.stage == Stage::ApiKey,
    ));
    lines.push(field_line(
        "Project ID",
        &setup.project_id,
        setup.stage == Stage::ProjectId,
    ));
    lines.push(field_line(
        "Currency",
        &currency_display,
        setup.stage == Stage::Currency,
    ));
    lines.push(Line::default());

    if let Some(notice) = &setup.notice {
        lines.push(Line::from(Span::styled(
            format!("  {notice}"),
            Style::new().fg(WARN),
        )));
        lines.push(Line::default());
    }
    append_error(&mut lines, &setup.error);

    let help = match setup.stage {
        Stage::ApiKey => {
            "Enter submit · Tab next field · Ctrl+L list projects · Ctrl+U clear · Ctrl+C quit"
        }
        Stage::ProjectId => {
            "Enter continue · Tab next field · Ctrl+L list projects · Ctrl+U clear · Esc back"
        }
        _ => "Enter launch rc-tui · Esc back · Ctrl+U clear · Ctrl+C quit",
    };
    (lines, help)
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {text}"),
        Style::new().fg(TEXT).add_modifier(Modifier::BOLD),
    ))
}

fn dim_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), Style::new().fg(DIM)))
}

fn field_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let label_style = if active {
        Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(DIM)
    };
    let value_style = if active {
        Style::new().fg(TEXT)
    } else {
        Style::new().fg(DIM)
    };
    Line::from(vec![
        Span::styled(format!("  {label:<11}"), label_style),
        Span::styled(
            format!("{value}{}", if active { "▌" } else { " " }),
            value_style,
        ),
    ])
}

fn masked_key(key: &str) -> String {
    if key.trim().is_empty() {
        "(paste a sk_… key)".to_string()
    } else {
        redact(key)
    }
}

fn error_lines(failure: &Failure) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    push_failure(&mut lines, failure);
    lines
}

fn append_error(lines: &mut Vec<Line>, error: &Option<Failure>) {
    if let Some(failure) = error {
        push_failure(lines, failure);
    }
}

fn push_failure(lines: &mut Vec<Line>, failure: &Failure) {
    lines.push(Line::from(Span::styled(
        format!("  ✗ {}", failure.headline()),
        Style::new().fg(BAD).add_modifier(Modifier::BOLD),
    )));
    if let Some(detail) = failure.detail() {
        lines.push(Line::from(Span::styled(
            format!("    {detail}"),
            Style::new().fg(BAD),
        )));
    }
    for hint in failure.hints() {
        lines.push(Line::from(Span::styled(
            format!("    → {hint}"),
            Style::new().fg(DIM),
        )));
    }
    if let Some(doc_url) = &failure.doc_url {
        lines.push(Line::from(Span::styled(
            format!("    docs: {doc_url}"),
            Style::new().fg(DIM).add_modifier(Modifier::UNDERLINED),
        )));
    }
    lines.push(Line::default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn demo_activity_renders_rail_shelf_and_full_view() {
        let mut app = App::new(crate::app::AppOptions {
            sources: vec![crate::app::Source::demo("One", "proj_demo_1")],
            currency: "EUR".to_string(),
            interval: Duration::from_secs(300),
            chart_ttl: Duration::from_secs(900),
            motion: crate::app::Motion::Full,
            wall: false,
            pins: Vec::new(),
            ui_scale: 0,
            ui_page: 0,
            feed_path: None,
            feed_seen_at: None,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.cards.is_empty() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut large = Terminal::new(TestBackend::new(200, 54)).unwrap();
        large.draw(|frame| draw(&mut app, frame)).unwrap();
        let rail: String = large
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rail.contains("PULSE 24H"));
        assert!(rail.contains("MagicWeather"));
        assert!(rail.contains("FULL FEED"));

        let mut shelf = Terminal::new(TestBackend::new(140, 44)).unwrap();
        shelf.draw(|frame| draw(&mut app, frame)).unwrap();
        let shelf_text: String = shelf
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(shelf_text.contains("ACTIVITY"));
        assert!(shelf_text.contains("MagicWeather"));

        let mut mid = Terminal::new(TestBackend::new(172, 46)).unwrap();
        mid.draw(|frame| draw(&mut app, frame)).unwrap();
        let mid_text: String = mid
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(mid_text.contains("PULSE 24H"));

        let mut tiny = Terminal::new(TestBackend::new(100, 30)).unwrap();
        tiny.draw(|frame| draw(&mut app, frame)).unwrap();
        let tiny_text: String = tiny
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(tiny_text.contains("f ACTIVITY"));

        app.open_activity();
        app.activity.selected_id = Some("demo:review:1".to_string());
        let mut full = Terminal::new(TestBackend::new(200, 54)).unwrap();
        full.draw(|frame| draw(&mut app, frame)).unwrap();
        let full_text: String = full
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(full_text.contains("WRITTEN REVIEW"));
        assert!(full_text.contains("Finally an app that respects battery"));
        assert!(full_text.contains("storebot"));
    }

    #[test]
    fn demo_frame_contains_cards_and_sparklines() {
        let mut app = App::new(crate::app::AppOptions {
            sources: vec![crate::app::Source::demo("One", "proj1")],
            currency: "EUR".to_string(),
            interval: Duration::from_secs(300),
            chart_ttl: Duration::from_secs(900),
            motion: crate::app::Motion::Full,
            wall: false,
            pins: Vec::new(),
            ui_scale: 0,
            ui_page: 0,
            feed_path: None,
            feed_seen_at: None,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.cards.is_empty() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.cards.len(), 6);
        assert!(matches!(app.status, Status::Idle));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| draw(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();

        assert!(text.contains("ACTIVE TRIALS"));
        assert!(text.contains("ACTIVE SUBSCRIPTIONS"));
        assert!(text.contains("MRR"));
        assert!(text.contains("REVENUE"));
        assert!(text.contains("NEW CUSTOMERS"));
        assert!(text.contains("ACTIVE CUSTOMERS"));
        assert!(text.contains("Last 28 days"));
        assert!(text.contains("● live"));
        assert!(text.contains("next in"));
        assert!(
            text.chars().any(|c| matches!(c, '▄' | '▀' | '█')),
            "no block chart cells rendered"
        );
    }

    #[test]
    fn setup_form_renders_the_wizard() {
        use crate::api::FailureKind;

        let mut setup = Setup::new("https://api.revenuecat.com/v2", "", "", "EUR");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| draw_setup(&setup, frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Connect rc-tui to RevenueCat"));
        assert!(text.contains("API key"));
        assert!(text.contains("Project ID"));
        assert!(text.contains("Currency"));
        assert!(text.contains("New secret API key"));

        setup.validation_failed(Failure {
            kind: FailureKind::Unauthorized,
            status: Some(401),
            message: "Invalid API key.".to_string(),
            doc_url: None,
            context: None,
            retry_after: None,
        });
        terminal.draw(|frame| draw_setup(&setup, frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("rejected"));
        assert!(text.contains("sk_"));
    }

    #[test]
    fn import_screen_shows_paste_instructions_and_results() {
        let mut state = Import::new();
        state.text = "sk_one\nproj_abc sk_two".to_string();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| draw_import(&state, frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Paste one v2 secret key per line"));
        assert!(text.contains("2 key(s)"));

        state.stage = ImportStage::Results;
        state.finish(vec![
            crate::import::Outcome {
                order: 0,
                label: "MagicWeather".to_string(),
                project: Some(crate::config::ProjectConfig::new(
                    "proj1",
                    "sk_one",
                    Some("MagicWeather".into()),
                )),
                failure: None,
                charts: Some(true),
            },
            crate::import::Outcome {
                order: 1,
                label: "line 2".to_string(),
                project: None,
                failure: Some(Failure::local("No sk_ key on this line")),
                charts: None,
            },
        ]);
        terminal.draw(|frame| draw_import(&state, frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("MagicWeather"));
        assert!(text.contains("1 ready to save"));
        assert!(text.contains("1 failed"));
        assert!(text.contains("Enter save ready projects"));
    }

    #[test]
    fn connected_screen_offers_the_next_project() {
        let mut setup = Setup::new("https://api.revenuecat.com/v2", "sk_abc", "proj1", "EUR")
            .with_name(Some("MagicWeather".to_string()));
        setup.connected();

        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw_setup(&setup, frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Connected"));
        assert!(text.contains("MagicWeather"));
        assert!(text.contains("Add another project?"));
    }
}
