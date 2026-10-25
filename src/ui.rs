use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

use chrono::{Days, TimeZone};

use crate::activity::{Celebration, CelebrationLevel, Entry, EntryKind, Filter, Health, Range};
use crate::api::{DOCS_API_KEYS, Failure, SCOPE_CHARTS, SCOPE_OVERVIEW, SCOPE_PROJECTS, redact};
use crate::app::{App, Card, Motion, PEOPLE, RowState, Status, Trend, Window, card_rank, is_flow};
use crate::import::{Import, Stage as ImportStage};
use crate::setup::{Setup, Stage};
use crate::theme::{self, shade};

pub use crate::theme::accent;

fn paint_background(frame: &mut Frame) {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().bg(theme::bg()));
}

pub fn draw(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let now = Instant::now();
    paint_background(frame);

    if app.detail.is_some() {
        draw_detail(app, frame, area, now);
    } else {
        draw_dashboard(app, frame, area, now);
    }

    if app.activity.open
        && let Some(celebration) = app.activity.current_celebration(now)
    {
        draw_celebration(frame, area, celebration, now, app.motion);
    }

    let delta = app.frame_delta();
    app.fx.process_effects(delta, frame.buffer_mut(), area);

    if app.picker.is_some() {
        draw_picker(app, frame, area, now);
    }
    if app.moments.open {
        draw_moments(app, frame, area);
        if let Some(celebration) = app.activity.current_celebration(now) {
            draw_celebration(frame, area, celebration, now, app.motion);
        }
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
    let gap = if area.width < 110 { "  " } else { "    " };

    let mut spans = vec![
        Span::styled(
            " RevenueCat",
            Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(gap, Style::new()),
        Span::styled(
            app.scope_label().to_uppercase(),
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(id) = app.scope_id() {
        spans.push(Span::styled(
            format!("  {id}"),
            Style::new().fg(theme::dim()),
        ));
    }
    if app.scope == 0 && app.project_count() > 1 {
        spans.push(Span::styled(
            format!("{gap}{} projects", app.project_count()),
            Style::new().fg(theme::dim()),
        ));
    }
    spans.push(Span::styled(
        format!("{gap}{}", app.currency),
        Style::new().fg(theme::dim()),
    ));
    if app.ui_scale > 0 {
        spans.push(Span::styled(
            format!("{gap}{} / page", app.per_page()),
            Style::new().fg(theme::dim()),
        ));
    }
    if app.demo {
        spans.push(Span::styled(
            format!("{gap}demo"),
            Style::new().fg(theme::warn()),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), left);

    let (indicator, color) = if matches!(app.status, Status::Fetching) {
        ("updating".to_string(), theme::dim())
    } else if let Status::Error(_) = app.status {
        ("error".to_string(), theme::bad())
    } else if !app.has_data() {
        ("loading".to_string(), theme::dim())
    } else if app.stale(now) {
        let text = if app.partial() {
            format!("{}/{} live · stale", app.live_count(), app.project_count())
        } else {
            "stale".to_string()
        };
        (text, theme::warn())
    } else if app.partial() {
        (
            format!("{}/{} live", app.live_count(), app.project_count()),
            theme::warn(),
        )
    } else {
        ("live".to_string(), theme::good())
    };
    let page = if app.ui_scale > 0 {
        format!("{}/{}   ·   ", app.page + 1, app.page_count())
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::styled(page, Style::new().fg(theme::dim())),
        Span::styled(
            format!("{}    ", chrono::Local::now().format("%H:%M")),
            Style::new().fg(theme::dim()),
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
            Style::new().fg(theme::border()),
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
    } else if area.width >= 96 && area.height >= 30 {
        8
    } else {
        0
    };
    if shelf_height > 0 {
        let [cards, shelf] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(shelf_height)]).areas(area);
        draw_cards(app, frame, cards, now);
        draw_activity_shelf(app, frame, shelf, now);
    } else if area.width >= 96 && area.height > 1 {
        let ticker_height = if area.width >= 96 && area.height >= 18 {
            2
        } else {
            1
        };
        let [cards, ticker] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(ticker_height)]).areas(area);
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
        Health::Live => theme::good(),
        Health::Stale => theme::warn(),
        Health::Offline | Health::NotConnected | Health::Error => theme::bad(),
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
                "renewal" => ("⟳", theme::retention()),
                "trial_conversion" => ("◐", theme::trial()),
                "revenue" => ("~", theme::dim()),
                _ => ("+", theme::money()),
            }
        }
        EntryKind::Review => {
            let stars = entry.stars().unwrap_or(0);
            (
                "★",
                if stars <= 2 {
                    theme::bad()
                } else {
                    theme::warn()
                },
            )
        }
        EntryKind::Rating => ("☆", theme::warn()),
        EntryKind::Unknown => ("·", theme::dim()),
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
                return ("—".to_string(), theme::dim());
            };
            let text = format_metric(purchase.amount, "$", &purchase.currency);
            let prefix = if purchase.estimated { "~+" } else { "+" };
            (
                format!("{prefix}{text}"),
                if purchase.estimated {
                    theme::dim()
                } else {
                    theme::money()
                },
            )
        }
        EntryKind::Review => {
            let stars = entry.stars().unwrap_or(0);
            (
                feed_stars(stars),
                if stars <= 2 {
                    theme::bad()
                } else {
                    theme::warn()
                },
            )
        }
        EntryKind::Rating => match entry.rating.as_ref() {
            Some(rating) => match rating.stars {
                Some(stars) => (
                    feed_stars(stars),
                    if stars <= 2 {
                        theme::bad()
                    } else {
                        theme::warn()
                    },
                ),
                None => (format!("avg {:.1}", rating.average), theme::dim()),
            },
            None => ("—".to_string(), theme::dim()),
        },
        EntryKind::Unknown => ("—".to_string(), theme::dim()),
    }
}

fn feed_accent(entry: &Entry) -> Color {
    match entry.kind() {
        EntryKind::Purchase => theme::money(),
        EntryKind::Review => {
            if entry.stars().unwrap_or(0) <= 2 {
                theme::bad()
            } else {
                theme::warn()
            }
        }
        EntryKind::Rating => theme::warn(),
        EntryKind::Unknown => theme::dim(),
    }
}

fn feed_event_label(entry: &Entry) -> &'static str {
    match entry
        .purchase
        .as_ref()
        .map(|purchase| purchase.event.as_str())
    {
        Some("renewal") => "renewal",
        Some("trial_conversion") => "trial conversion",
        Some("revenue") => "revenue movement",
        _ => "new purchase",
    }
}

fn feed_subtitle(entry: &Entry) -> String {
    match entry.kind() {
        EntryKind::Purchase => {
            let product = entry
                .purchase
                .as_ref()
                .map_or("", |purchase| purchase.product.as_str());
            if product.is_empty() || entry.app.to_lowercase().contains(&product.to_lowercase()) {
                feed_event_label(entry).to_string()
            } else {
                product.to_string()
            }
        }
        EntryKind::Review => {
            let review = entry.review.as_ref();
            let title = review.map_or("", |review| review.title.as_str());
            if !title.is_empty() {
                format!("“{title}”")
            } else {
                let body = review.map_or("", |review| review.body.as_str());
                format!("“{}”", excerpt(body, 120))
            }
        }
        EntryKind::Rating => {
            let count = entry.rating.as_ref().map_or(0, |rating| rating.count);
            if count > 1 {
                format!("{count} new ratings · no written review")
            } else {
                "new rating · no written review".to_string()
            }
        }
        EntryKind::Unknown => String::new(),
    }
}

fn feed_meta(entry: &Entry, now_ms: i64) -> String {
    let mut parts = Vec::new();
    match entry.kind() {
        EntryKind::Purchase => {
            parts.push(feed_event_label(entry).to_string());
            if let Some(purchase) = &entry.purchase {
                if purchase.first_ever {
                    parts.push("FIRST EVER".to_string());
                } else if purchase.first_today {
                    parts.push("FIRST TODAY".to_string());
                }
                if purchase.estimated {
                    parts.push("ESTIMATED".to_string());
                }
            }
        }
        EntryKind::Review => parts.push("written review".to_string()),
        EntryKind::Rating => parts.push("rating only".to_string()),
        EntryKind::Unknown => {}
    }
    if !entry.country.is_empty() {
        parts.push(entry.country.clone());
    }
    parts.push(age_label(now_ms, entry.timestamp()));
    parts.join("  ·  ")
}

fn feed_stamp(entry: &Entry) -> String {
    if entry.is_history() {
        return "ALL".to_string();
    }
    entry.datetime().map_or_else(
        || "--:--".to_string(),
        |date| date.format("%H:%M").to_string(),
    )
}

fn feed_day(entry: &Entry) -> String {
    if entry.is_history() {
        return "ALL-TIME".to_string();
    }
    entry
        .datetime()
        .map_or_else(String::new, |date| date.date_naive().to_string())
}

fn feed_day_label(entry: &Entry, now_ms: i64) -> String {
    if entry.is_history() {
        return "ALL-TIME".to_string();
    }
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

fn text_width(text: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    text.width()
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let width = width.max(4);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for word in text.split_whitespace() {
        let word_width = text_width(word);
        if current_width > 0 && current_width + 1 + word_width > width {
            lines.push(std::mem::take(&mut current));
            current_width = 0;
        }
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let mut chunk = String::new();
            let mut chunk_width = 0usize;
            for character in word.chars() {
                let char_width = character.width().unwrap_or(0);
                if chunk_width > 0 && chunk_width + char_width > width {
                    lines.push(std::mem::take(&mut chunk));
                    chunk_width = 0;
                }
                chunk.push(character);
                chunk_width += char_width;
            }
            current = chunk;
            current_width = chunk_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn excerpt(text: &str, max: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text;
    }
    let mut out: String = chars.into_iter().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

struct FeedCard {
    lines: Vec<Line<'static>>,
    height: u16,
    accent: Color,
    background: Color,
}

fn make_feed_card(entry: &Entry, width: u16, selected: bool) -> FeedCard {
    let background = if selected {
        theme::selection()
    } else {
        theme::surface()
    };
    let accent = feed_accent(entry);
    let inner_width = width.saturating_sub(4) as usize;
    if inner_width < 8 {
        return FeedCard {
            lines: vec![Line::from(Span::styled(
                entry.app.clone(),
                Style::new().fg(theme::text()).bg(background),
            ))],
            height: 1,
            accent,
            background,
        };
    }
    let (value, value_color) = feed_value(entry);
    let value_width = text_width(&value).max(5);
    let reserve = if inner_width >= 36 {
        value_width + 2
    } else {
        0
    };
    let lead_width = 10usize;
    let title_width = inner_width.saturating_sub(lead_width + reserve).max(8);
    let title_lines = wrap_text(&entry.app, title_width);
    let mut lines = Vec::new();
    for (index, part) in title_lines.iter().enumerate() {
        let prefix = if index == 0 {
            format!(" {}  ", feed_stamp(entry))
        } else {
            " ".repeat(lead_width)
        };
        let mut spans = vec![Span::styled(
            prefix,
            Style::new().fg(theme::dim()).bg(background),
        )];
        if index == 0 {
            let (glyph, glyph_color) = feed_glyph(entry);
            spans.push(Span::styled(
                format!("{glyph} "),
                Style::new().fg(glyph_color).bg(background),
            ));
        }
        spans.push(Span::styled(
            part.clone(),
            Style::new()
                .fg(if selected {
                    theme::bright()
                } else {
                    theme::text()
                })
                .bg(background),
        ));
        let padding = title_width.saturating_sub(text_width(part));
        if index == 0 && reserve > 0 {
            spans.push(Span::styled(
                format!("{}  ", " ".repeat(padding)),
                Style::new().bg(background),
            ));
            spans.push(Span::styled(
                value.clone(),
                Style::new().fg(value_color).bg(background),
            ));
        } else {
            spans.push(Span::styled(
                " ".repeat(padding + reserve),
                Style::new().bg(background),
            ));
        }
        lines.push(Line::from(spans));
    }
    let subtitle = feed_subtitle(entry);
    let subtitle_width = inner_width.saturating_sub(lead_width).max(8);
    for part in wrap_text(&subtitle, subtitle_width) {
        lines.push(Line::from(vec![
            Span::styled(" ".repeat(lead_width), Style::new().bg(background)),
            Span::styled(part, Style::new().fg(theme::text()).bg(background)),
        ]));
    }
    let meta = feed_meta(entry, feed_now_ms());
    let meta_width = inner_width
        .saturating_sub(lead_width + if reserve == 0 { value_width + 2 } else { 0 })
        .max(8);
    let meta_lines = wrap_text(&meta, meta_width);
    for (index, part) in meta_lines.iter().enumerate() {
        let mut spans = vec![
            Span::styled(" ".repeat(lead_width), Style::new().bg(background)),
            Span::styled(part.clone(), Style::new().fg(theme::dim()).bg(background)),
        ];
        if index == 0 && reserve == 0 {
            let padding = meta_width.saturating_sub(text_width(part));
            spans.push(Span::styled(
                format!("{}  ", " ".repeat(padding)),
                Style::new().bg(background),
            ));
            spans.push(Span::styled(
                value.clone(),
                Style::new().fg(value_color).bg(background),
            ));
        }
        lines.push(Line::from(spans));
    }
    let height = lines.len().min(u16::MAX as usize) as u16;
    FeedCard {
        lines,
        height,
        accent,
        background,
    }
}

fn draw_feed_card(frame: &mut Frame, area: Rect, entry: &Entry, selected: bool) {
    let card = make_feed_card(entry, area.width, selected);
    frame.render_widget(
        Block::default().style(Style::new().bg(card.background)),
        area,
    );
    for y in area.y..area.bottom() {
        frame.buffer_mut().set_string(
            area.x,
            y,
            "▎",
            Style::new().fg(card.accent).bg(card.background),
        );
    }
    for (offset, line) in card.lines.into_iter().enumerate() {
        if offset >= area.height as usize {
            break;
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: area.x + 2,
                y: area.y + offset as u16,
                width: area.width.saturating_sub(2),
                height: 1,
            },
        );
    }
}

fn draw_day_header(
    frame: &mut Frame,
    area: Rect,
    row: u16,
    entry: &Entry,
    count: usize,
    now_ms: i64,
) {
    let label = feed_day_label(entry, now_ms);
    let text = if count > 1 {
        format!("{label}  ·  {count}")
    } else {
        label
    };
    let mut spans = vec![Span::styled(
        text.clone(),
        Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
    )];
    let remaining = area.width.saturating_sub(text_width(&text) as u16 + 2) as usize;
    spans.push(Span::styled(
        format!("  {}", "─".repeat(remaining)),
        Style::new().fg(theme::border()),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect {
            x: area.x,
            y: area.y + row,
            width: area.width,
            height: 1,
        },
    );
}

fn draw_pulse(frame: &mut Frame, area: Rect, entries: &[&Entry], now_ms: i64) {
    if area.height < 2 || area.width == 0 {
        return;
    }
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
    let bin_width = (width / 24).max(1);
    let top = area.y;
    let bottom = area.y + 1;
    for (index, (money, reviews)) in bins.iter().enumerate() {
        let x = area.x + (index * bin_width) as u16;
        let style_money = Style::new().fg(theme::money());
        let style_reviews = Style::new().fg(theme::warn());
        if *reviews > 0 {
            for offset in 0..bin_width.min(2) {
                frame
                    .buffer_mut()
                    .set_string(x + offset as u16, top, "█", style_reviews);
            }
        }
        if *money > 0 {
            for offset in 0..bin_width.min(2) {
                frame
                    .buffer_mut()
                    .set_string(x + offset as u16, bottom, "█", style_money);
            }
        }
        if *money == 0 && *reviews == 0 {
            frame
                .buffer_mut()
                .set_string(x, bottom, "·", Style::new().fg(theme::border()));
        }
    }
}

fn pulse_summary(entries: &[&Entry]) -> String {
    let sales = entries.iter().filter(|entry| entry.is_money()).count();
    let reviews = entries.iter().filter(|entry| entry.is_review()).count();
    match (sales, reviews) {
        (0, 0) => "no events".to_string(),
        (sales, 0) => format!("{sales} sale{}", if sales == 1 { "" } else { "s" }),
        (0, reviews) => format!("{reviews} review{}", if reviews == 1 { "" } else { "s" }),
        (sales, reviews) => format!(
            "{sales} sale{} · {reviews} review{}",
            if sales == 1 { "" } else { "s" },
            if reviews == 1 { "" } else { "s" }
        ),
    }
}

fn draw_activity_ticker(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let entries = app.activity_entries_all(Range::D7);
    let (health, health_color) = feed_health(app);
    let unread = app.activity_unread();
    frame.render_widget(
        Block::default().style(Style::new().bg(theme::surface())),
        area,
    );
    if area.height == 0 {
        return;
    }
    let latest = entries.first();
    let mut lines = Vec::new();
    if let Some(entry) = latest {
        let (value, value_color) = feed_value(entry);
        let mut first = vec![
            Span::styled(
                if unread > 0 { "● " } else { "· " },
                Style::new().fg(if unread > 0 {
                    theme::money()
                } else {
                    theme::dim()
                }),
            ),
            Span::styled(
                format!("{}  ", feed_stamp(entry)),
                Style::new().fg(theme::dim()),
            ),
        ];
        let (glyph, glyph_color) = feed_glyph(entry);
        first.push(Span::styled(
            format!("{glyph}  "),
            Style::new().fg(glyph_color),
        ));
        first.push(Span::styled(
            entry.app.clone(),
            Style::new().fg(theme::text()),
        ));
        first.push(Span::styled(
            format!("  {value}"),
            Style::new().fg(value_color),
        ));
        lines.push(Line::from(first));
        if area.height > 1 {
            let rating_total = app.activity_rating_total();
            let suffix = if rating_total > 0 {
                format!("   {health}  ·  {rating_total} ratings  ·  f ACTIVITY")
            } else {
                format!("   {health}  ·  f ACTIVITY")
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("    {}  ·  {}", feed_subtitle(entry), entry.country),
                    Style::new().fg(theme::dim()),
                ),
                Span::styled(suffix, Style::new().fg(health_color)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "activity starts with the next sale or review",
            Style::new().fg(theme::dim()),
        )));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
    if let Some(celebration) = app.activity.current_celebration(now) {
        draw_celebration(frame, area, celebration, now, app.motion);
    }
}

fn draw_activity_shelf(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    if area.height < 2 {
        return;
    }
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(theme::border_strong()),
        )),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );
    frame.render_widget(
        Block::default().style(Style::new().bg(theme::surface())),
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
    let columns = ((area.width.saturating_sub(4) / 44).clamp(2, 4)) as usize;
    let shown = entries.len().min(columns);
    let header_area = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(4),
        height: 1,
    };
    let mut header = vec![
        Span::styled(
            "ACTIVITY",
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::new()),
    ];
    if unread > 0 {
        header.push(Span::styled("● ", Style::new().fg(theme::money())));
        header.push(Span::styled(
            format!("{unread} NEW"),
            Style::new().fg(theme::money()),
        ));
        header.push(Span::styled("   ", Style::new()));
    }
    header.push(Span::styled(health, Style::new().fg(health_color)));
    let rating_total = app.activity_rating_total();
    if rating_total > 0 {
        header.push(Span::styled(
            format!("   {rating_total} RATINGS"),
            Style::new().fg(theme::dim()),
        ));
    }
    if entries.len() > shown {
        header.push(Span::styled(
            format!("   +{} MORE", entries.len() - shown),
            Style::new().fg(theme::dim()),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(header)), header_area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "f FULL FEED",
            Style::new().fg(theme::dim()),
        )))
        .alignment(Alignment::Right),
        header_area,
    );
    let card_top = area.y + 2;
    let card_height = area.height.saturating_sub(3);
    if card_height < 2 || entries.is_empty() {
        return;
    }
    let inner_x = area.x + 2;
    let inner_width = area.width.saturating_sub(4);
    let gap = 1u16;
    let card_width = (inner_width.saturating_sub(gap * (columns as u16 - 1))) / columns as u16;
    for (index, entry) in entries.iter().take(columns).enumerate() {
        let x = inner_x + index as u16 * (card_width + gap);
        draw_feed_card(
            frame,
            Rect {
                x,
                y: card_top,
                width: card_width,
                height: card_height,
            },
            entry,
            false,
        );
        if index + 1 < columns {
            for y in card_top..card_top + card_height {
                frame.buffer_mut().set_string(
                    x + card_width,
                    y,
                    " ",
                    Style::new().bg(theme::surface()),
                );
            }
        }
    }
    if let Some(celebration) = app.activity.current_celebration(now) {
        draw_celebration(frame, area, celebration, now, app.motion);
    }
}

fn draw_attention(
    frame: &mut Frame,
    x: u16,
    y: &mut u16,
    width: u16,
    entry: &Entry,
    now_ms: i64,
    limit: u16,
) {
    let title = format!("{}  {}", feed_value(entry).0, entry.app);
    for line in wrap_text(&title, width.saturating_sub(2) as usize)
        .into_iter()
        .take(limit as usize)
    {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                line,
                Style::new().fg(theme::bad()).add_modifier(Modifier::BOLD),
            ))),
            Rect {
                x,
                y: *y,
                width,
                height: 1,
            },
        );
        *y += 1;
    }
    if *y < limit {
        let quote_width = width.saturating_sub(2) as usize;
        if let Some(review) = &entry.review {
            let quote = if review.title.is_empty() {
                excerpt(&review.body, 90)
            } else {
                review.title.clone()
            };
            for line in wrap_text(&format!("“{quote}”"), quote_width)
                .into_iter()
                .take(2)
            {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        line,
                        Style::new().fg(theme::text()),
                    ))),
                    Rect {
                        x,
                        y: *y,
                        width,
                        height: 1,
                    },
                );
                *y += 1;
            }
        }
    }
    if *y < limit {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    "{} · written review · {}",
                    entry.country,
                    age_label(now_ms, entry.timestamp())
                ),
                Style::new().fg(theme::dim()),
            ))),
            Rect {
                x,
                y: *y,
                width,
                height: 1,
            },
        );
        *y += 1;
    }
}

fn draw_activity_rail(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    if area.height < 8 || area.width < 20 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::new().bg(theme::surface())),
        area,
    );
    if area.x > 0 {
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.x - 1,
                y,
                "│",
                Style::new().fg(theme::border_strong()).bg(theme::bg()),
            );
        }
    }
    let entries = app.activity_entries_all(Range::D7);
    let (health, health_color) = feed_health(app);
    let unread = app.activity_unread();
    let rating_total = app.activity_rating_total();
    let inner_x = area.x + 2;
    let inner_w = area.width.saturating_sub(4);
    let now_ms = feed_now_ms();
    let mut y = area.y;
    let mut header = vec![
        Span::styled(
            "ACTIVITY",
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::new()),
    ];
    if unread > 0 {
        header.push(Span::styled("● ", Style::new().fg(theme::money())));
        header.push(Span::styled(
            format!("{unread} NEW"),
            Style::new().fg(theme::money()),
        ));
        header.push(Span::styled("  ·  ", Style::new().fg(theme::dim())));
    }
    header.push(Span::styled(health.clone(), Style::new().fg(health_color)));
    if rating_total > 0 {
        header.push(Span::styled("  ·  ", Style::new().fg(theme::dim())));
        header.push(Span::styled(
            format!("{rating_total} RATINGS"),
            Style::new().fg(theme::dim()),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(header)),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("f", Style::new().fg(theme::dim()))))
            .alignment(Alignment::Right),
        Rect {
            x: inner_x,
            y,
            width: inner_w,
            height: 1,
        },
    );
    y += 1;

    let pulse_entries = app.activity_entries_all(Range::H24);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "PULSE 24H",
                Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   {}", pulse_summary(&pulse_entries)),
                Style::new().fg(theme::dim()),
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
    y += 2;
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "00        06        12        18        NOW",
            Style::new().fg(theme::dim()),
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
        Paragraph::new(Span::styled(
            "─".repeat(inner_w as usize),
            Style::new().fg(theme::border()),
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
    if let Some(entry) = attention {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "NEEDS ATTENTION",
                    Style::new().fg(theme::bad()).add_modifier(Modifier::BOLD),
                ),
                Span::styled("   1", Style::new().fg(theme::dim())),
            ])),
            Rect {
                x: inner_x,
                y,
                width: inner_w,
                height: 1,
            },
        );
        y += 1;
        let limit = y.saturating_add(5).min(area.bottom().saturating_sub(4));
        draw_attention(frame, inner_x, &mut y, inner_w, entry, now_ms, limit);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(inner_w as usize),
                Style::new().fg(theme::border()),
            )),
            Rect {
                x: inner_x,
                y,
                width: inner_w,
                height: 1,
            },
        );
        y += 1;
    }

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "LATEST",
                Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if unread > 0 {
                    format!("   {} NEW", unread)
                } else {
                    "   UP TO DATE".to_string()
                },
                Style::new().fg(if unread > 0 {
                    theme::money()
                } else {
                    theme::dim()
                }),
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
    let attention_key = attention.map(|entry| entry.id.as_str());
    let footer_y = area.bottom().saturating_sub(1);
    let mut last_day = String::new();
    for entry in entries
        .iter()
        .filter(|entry| Some(entry.id.as_str()) != attention_key)
    {
        if y + 3 > footer_y {
            break;
        }
        let day = feed_day(entry);
        if day != last_day {
            let count = entries
                .iter()
                .filter(|candidate| feed_day(candidate) == day)
                .count();
            draw_day_header(
                frame,
                Rect {
                    x: inner_x,
                    y,
                    width: inner_w,
                    height: 1,
                },
                0,
                entry,
                count,
                now_ms,
            );
            y += 1;
            last_day = day;
        }
        let card = make_feed_card(entry, inner_w, false);
        if y + card.height + 1 > footer_y {
            break;
        }
        draw_feed_card(
            frame,
            Rect {
                x: inner_x,
                y,
                width: inner_w,
                height: card.height,
            },
            entry,
            false,
        );
        y += card.height;
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("storebot ", Style::new().fg(theme::dim())),
            Span::styled(health, Style::new().fg(health_color)),
            Span::styled("   f FULL FEED", Style::new().fg(theme::dim())),
        ])),
        Rect {
            x: inner_x,
            y: footer_y,
            width: inner_w,
            height: 1,
        },
    );
    if let Some(celebration) = app.activity.current_celebration(now) {
        draw_celebration(frame, area, celebration, now, app.motion);
    }
}

fn draw_summary(frame: &mut Frame, area: Rect, entries: &[&Entry], now_ms: i64, rating_total: i64) {
    if area.height < 2 {
        return;
    }
    let sales = entries.iter().filter(|entry| entry.is_money()).count();
    let reviews = entries.iter().filter(|entry| entry.is_review()).count();
    let sales_text = if sales > 0 {
        format!("{sales} sale{}", if sales == 1 { "" } else { "s" })
    } else {
        "no sales".to_string()
    };
    let summary = format!(
        "{sales_text}    ·    {reviews} review{}",
        if reviews == 1 { "" } else { "s" }
    );
    let mut spans = vec![
        Span::styled(
            "PULSE 24H",
            Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::new()),
        Span::styled(summary, Style::new().fg(theme::text())),
    ];
    let attention = entries
        .iter()
        .filter(|entry| entry.stars().is_some_and(|stars| stars <= 2))
        .count();
    if attention > 0 {
        spans.push(Span::styled(
            format!("    ·    {attention} needs attention"),
            Style::new().fg(theme::bad()),
        ));
    }
    if rating_total > 0 {
        spans.push(Span::styled(
            format!("    ·    {rating_total} all-time ratings"),
            Style::new().fg(theme::dim()),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        Rect { height: 1, ..area },
    );
    draw_pulse(
        frame,
        Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 2.min(area.height.saturating_sub(1)),
        },
        entries,
        now_ms,
    );
    if area.height >= 4 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "00        06        12        18        NOW",
                Style::new().fg(theme::dim()),
            ))),
            Rect {
                x: area.x,
                y: area.y + area.height - 1,
                width: area.width,
                height: 1,
            },
        );
    }
}

fn draw_activity(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let now_ms = feed_now_ms();
    let summary_height = if area.height >= 22 { 4 } else { 0 };
    let [header, rule, summary, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(summary_height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let scope = feed_scope(app);
    let entries = app.activity_entries(app.activity.range);
    let unread = app.activity_unread();
    let (health, health_color) = feed_health(app);
    let mut left = vec![
        Span::styled(
            " ACTIVITY",
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ·  ", Style::new().fg(theme::dim())),
        Span::styled(scope, Style::new().fg(theme::text())),
        Span::styled(
            format!("  ·  {} events", entries.len()),
            Style::new().fg(theme::dim()),
        ),
        Span::styled("   ● ", Style::new().fg(health_color)),
        Span::styled(health, Style::new().fg(health_color)),
    ];
    if unread > 0 {
        left.push(Span::styled(
            format!("   {unread} NEW"),
            Style::new().fg(theme::money()),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(left)), header);
    let mut right = Vec::new();
    for range in Range::ALL {
        let active = app.activity.range == range;
        right.push(Span::styled(
            format!(" {} ", range.label()),
            Style::new()
                .fg(if active {
                    theme::bright()
                } else {
                    theme::dim()
                })
                .bg(if active {
                    theme::surface()
                } else {
                    theme::bg()
                })
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
                .fg(if active {
                    theme::bright()
                } else {
                    theme::dim()
                })
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
            Style::new().fg(theme::border_strong()),
        )),
        rule,
    );
    if summary_height > 0 {
        let h24 = app.activity_entries_all(Range::H24);
        let rating_total = app.activity_rating_total();
        draw_summary(frame, summary, &h24, now_ms, rating_total);
    }
    let selected = app.activity.selected_index(&entries);
    if body.width >= 150 {
        let [list, gap, detail_area] = Layout::horizontal([
            Constraint::Percentage(62),
            Constraint::Length(2),
            Constraint::Percentage(38),
        ])
        .areas(body);
        draw_activity_list(frame, list, &entries, selected, now_ms);
        frame.render_widget(
            Block::default().style(Style::new().bg(theme::surface())),
            gap,
        );
        draw_activity_detail(frame, detail_area, entries.get(selected).copied(), now_ms);
    } else if body.height >= 12 {
        let [list, gap, detail_area] = Layout::vertical([
            Constraint::Fill(3),
            Constraint::Length(1),
            Constraint::Fill(2),
        ])
        .areas(body);
        draw_activity_list(frame, list, &entries, selected, now_ms);
        frame.render_widget(Block::default().style(Style::new().bg(theme::bg())), gap);
        draw_activity_detail(frame, detail_area, entries.get(selected).copied(), now_ms);
    } else {
        draw_activity_list(frame, body, &entries, selected, now_ms);
    }
    let _ = now;
}

fn list_block_height(entries: &[&Entry], index: usize, width: u16) -> u16 {
    let entry = entries[index];
    let mut height = make_feed_card(entry, width, false).height;
    if index == 0 || feed_day(entries[index - 1]) != feed_day(entry) {
        height += 1;
    }
    height
}

fn draw_activity_list(
    frame: &mut Frame,
    area: Rect,
    entries: &[&Entry],
    selected: usize,
    now_ms: i64,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No activity in this range.",
                Style::new().fg(theme::dim()),
            ))),
            area,
        );
        return;
    }
    let width = area.width.saturating_sub(1);
    if area.width > 0 {
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.right().saturating_sub(1),
                y,
                "│",
                Style::new().fg(theme::border()),
            );
        }
    }
    let mut start = selected.min(entries.len().saturating_sub(1));
    let mut used = list_block_height(entries, start, width);
    while start > 0 {
        let previous = list_block_height(entries, start - 1, width);
        if used + previous > area.height {
            break;
        }
        used += previous;
        start -= 1;
    }
    let total: u16 = (0..entries.len())
        .map(|index| list_block_height(entries, index, width))
        .sum();
    let mut row = 0u16;
    let mut last_day = String::new();
    for (index, entry) in entries.iter().enumerate().skip(start) {
        if row >= area.height {
            break;
        }
        let day = feed_day(entry);
        if day != last_day {
            let count = entries
                .iter()
                .filter(|candidate| feed_day(candidate) == day)
                .count();
            draw_day_header(frame, Rect { width, ..area }, row, entry, count, now_ms);
            row += 1;
            last_day = day;
            if row >= area.height {
                break;
            }
        }
        let card = make_feed_card(entry, width, index == selected);
        if row + card.height > area.height {
            break;
        }
        draw_feed_card(
            frame,
            Rect {
                x: area.x,
                y: area.y + row,
                width,
                height: card.height,
            },
            entry,
            index == selected,
        );
        row += card.height;
    }
    if total > area.height {
        let track = area.height;
        let thumb = ((track as u32 * track as u32) / total.max(1) as u32).max(1) as u16;
        let max_top = track.saturating_sub(thumb);
        let top = if total > track {
            ((start as u32 * max_top as u32) / entries.len().max(1) as u32) as u16
        } else {
            0
        };
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.right().saturating_sub(1),
                y,
                "│",
                Style::new().fg(theme::border()),
            );
        }
        for y in 0..thumb {
            frame.buffer_mut().set_string(
                area.right().saturating_sub(1),
                area.y + top + y,
                "▐",
                Style::new().fg(theme::dim()),
            );
        }
    }
}

fn draw_detail_line(lines: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    for part in wrap_text(text, width) {
        lines.push(Line::from(Span::styled(part, style)));
    }
}

fn detail_field(lines: &mut Vec<Line<'static>>, label: &str, value: &str, width: usize) {
    let label_width = 10usize.min(width.saturating_sub(8));
    let value_width = width.saturating_sub(label_width).max(4);
    for (index, part) in wrap_text(value, value_width).into_iter().enumerate() {
        let prefix = if index == 0 {
            format!("{label:<label_width$}")
        } else {
            " ".repeat(label_width)
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::new().fg(theme::dim())),
            Span::styled(part, Style::new().fg(theme::text())),
        ]));
    }
}

fn draw_activity_detail(frame: &mut Frame, area: Rect, entry: Option<&Entry>, now_ms: i64) {
    frame.render_widget(
        Block::default().style(Style::new().bg(theme::surface())),
        area,
    );
    if area.height == 0 || area.width == 0 {
        return;
    }
    if area.width > 0 {
        for y in area.y..area.bottom() {
            frame.buffer_mut().set_string(
                area.x,
                y,
                "│",
                Style::new().fg(theme::border_strong()).bg(theme::surface()),
            );
        }
    }
    let Some(entry) = entry else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Select an activity item.",
                Style::new().fg(theme::dim()),
            ))),
            Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(4),
                height: 1,
            },
        );
        return;
    };
    let inner_width = area.width.saturating_sub(4) as usize;
    let mut lines = Vec::new();
    let source = if entry.source.is_empty() {
        String::new()
    } else {
        format!("  ·  {}", entry.source.to_uppercase())
    };
    let kind = match entry.kind() {
        EntryKind::Purchase => format!("PURCHASE{source}"),
        EntryKind::Review => format!("WRITTEN REVIEW{source}"),
        EntryKind::Rating => {
            if entry.rating.as_ref().is_some_and(|rating| rating.summary) {
                format!("RATING HISTORY{source}")
            } else {
                format!("RATING ONLY{source}")
            }
        }
        EntryKind::Unknown => format!("ACTIVITY{source}"),
    };
    lines.push(Line::from(Span::styled(
        kind,
        Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::default());
    let (value, value_color) = feed_value(entry);
    lines.push(Line::from(Span::styled(
        value,
        Style::new().fg(value_color).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::default());
    match entry.kind() {
        EntryKind::Purchase => {
            if let Some(purchase) = &entry.purchase {
                if !purchase.product.is_empty()
                    && !entry
                        .app
                        .to_lowercase()
                        .contains(&purchase.product.to_lowercase())
                {
                    draw_detail_line(
                        &mut lines,
                        &purchase.product,
                        inner_width,
                        Style::new()
                            .fg(theme::bright())
                            .add_modifier(Modifier::BOLD),
                    );
                }
                draw_detail_line(
                    &mut lines,
                    &entry.app,
                    inner_width,
                    Style::new().fg(theme::text()),
                );
                lines.push(Line::default());
                detail_field(&mut lines, "EVENT", feed_event_label(entry), inner_width);
                detail_field(&mut lines, "MARKET", &entry.country, inner_width);
                detail_field(
                    &mut lines,
                    "SOURCE",
                    if entry.source.is_empty() {
                        "storebot"
                    } else {
                        &entry.source
                    },
                    inner_width,
                );
                detail_field(&mut lines, "TIME", &feed_stamp(entry), inner_width);
                if !purchase.transaction_id.is_empty() {
                    detail_field(&mut lines, "TXN", &purchase.transaction_id, inner_width);
                }
                lines.push(Line::default());
                if purchase.first_ever || purchase.first_today {
                    let mut tags = Vec::new();
                    if purchase.first_ever {
                        tags.push("FIRST EVER");
                    }
                    if purchase.first_today {
                        tags.push("FIRST TODAY");
                    }
                    draw_detail_line(
                        &mut lines,
                        &tags.join("  ·  "),
                        inner_width,
                        Style::new().fg(theme::warn()),
                    );
                }
                if purchase.estimated {
                    draw_detail_line(
                        &mut lines,
                        "App-level delta; no per-sale detail.",
                        inner_width,
                        Style::new().fg(theme::dim()),
                    );
                }
            }
        }
        EntryKind::Review => {
            let Some(review) = &entry.review else {
                return;
            };
            if !review.title.is_empty() {
                draw_detail_line(
                    &mut lines,
                    &format!("“{}”", review.title),
                    inner_width,
                    Style::new()
                        .fg(theme::bright())
                        .add_modifier(Modifier::BOLD),
                );
            }
            lines.push(Line::default());
            detail_field(&mut lines, "MARKET", &entry.country, inner_width);
            if !review.author.is_empty() {
                detail_field(&mut lines, "AUTHOR", &review.author, inner_width);
            }
            detail_field(
                &mut lines,
                "RECEIVED",
                &age_label(now_ms, entry.timestamp()),
                inner_width,
            );
            detail_field(
                &mut lines,
                "SOURCE",
                if entry.source.is_empty() {
                    "storebot"
                } else {
                    &entry.source
                },
                inner_width,
            );
            if !review.body.is_empty() {
                lines.push(Line::default());
                for paragraph in review.body.lines() {
                    draw_detail_line(
                        &mut lines,
                        paragraph,
                        inner_width,
                        Style::new().fg(theme::text()),
                    );
                }
            }
        }
        EntryKind::Rating => {
            if let Some(rating) = &entry.rating {
                draw_detail_line(
                    &mut lines,
                    &entry.app,
                    inner_width,
                    Style::new().fg(theme::text()),
                );
                lines.push(Line::default());
                detail_field(
                    &mut lines,
                    "RATINGS",
                    &rating.count.to_string(),
                    inner_width,
                );
                detail_field(
                    &mut lines,
                    "AVERAGE",
                    &format!("{:.1}", rating.average),
                    inner_width,
                );
                detail_field(
                    &mut lines,
                    "SOURCE",
                    if entry.source.is_empty() {
                        "itunes"
                    } else {
                        &entry.source
                    },
                    inner_width,
                );
                if !rating.summary {
                    detail_field(&mut lines, "MARKET", &entry.country, inner_width);
                    detail_field(
                        &mut lines,
                        "RECEIVED",
                        &age_label(now_ms, entry.timestamp()),
                        inner_width,
                    );
                }
            }
        }
        EntryKind::Unknown => {}
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: area.height.saturating_sub(2),
        },
    );
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

fn blend_color(from: Color, to: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let from = match from {
        Color::Rgb(red, green, blue) => (red, green, blue),
        _ => match theme::bg() {
            Color::Rgb(red, green, blue) => (red, green, blue),
            _ => return to,
        },
    };
    let to = match to {
        Color::Rgb(red, green, blue) => (red, green, blue),
        _ => return from.into(),
    };
    Color::Rgb(
        (from.0 as f32 + (to.0 as f32 - from.0 as f32) * amount)
            .round()
            .clamp(0.0, 255.0) as u8,
        (from.1 as f32 + (to.1 as f32 - from.1 as f32) * amount)
            .round()
            .clamp(0.0, 255.0) as u8,
        (from.2 as f32 + (to.2 as f32 - from.2 as f32) * amount)
            .round()
            .clamp(0.0, 255.0) as u8,
    )
}

fn celebration_color(tone: crate::activity::CelebrationTone) -> Color {
    match tone {
        crate::activity::CelebrationTone::Money => theme::money(),
        crate::activity::CelebrationTone::People => theme::retention(),
        crate::activity::CelebrationTone::World => theme::acquisition(),
        crate::activity::CelebrationTone::Stars => theme::warn(),
        crate::activity::CelebrationTone::Alert => theme::bad(),
        crate::activity::CelebrationTone::Quiet => theme::retention(),
    }
}

fn draw_celebration(
    frame: &mut Frame,
    area: Rect,
    celebration: &Celebration,
    now: Instant,
    motion: Motion,
) {
    let Some(progress) = celebration.progress(now) else {
        return;
    };
    if area.height < 2 {
        return;
    }
    let major = celebration.level == CelebrationLevel::Major;
    let accent = celebration_color(celebration.tone);
    let decay = (1.0 - progress).powf(1.4);
    let active_alpha = if !major || motion == Motion::Off {
        0.0
    } else {
        0.16 * decay
    };
    if active_alpha > 0.0 {
        let sweep = area.x as f32 + progress * (area.width as f32 + 12.0) - 6.0;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let Some(cell) = frame.buffer_mut().cell_mut((x, y)) else {
                    continue;
                };
                let mut background = blend_color(cell.bg, accent, active_alpha);
                if motion == Motion::Full && progress < 0.62 && (x as f32 - sweep).abs() < 4.0 {
                    background = blend_color(cell.bg, accent, 0.34 * decay);
                }
                cell.set_bg(background);
            }
        }
    }
    let banner = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1.min(area.height),
    };
    frame.render_widget(Clear, banner);
    frame.render_widget(
        Block::default().style(Style::new().bg(blend_color(theme::surface(), accent, 0.38))),
        banner,
    );
    let value = match &celebration.entry {
        Some(entry) => feed_value(entry).0,
        None => celebration.value.clone(),
    };
    let lead = if celebration.entry.is_some() {
        " ✦ "
    } else {
        " "
    };
    let mut spans = vec![
        Span::styled(lead, Style::new().fg(accent).add_modifier(Modifier::BOLD)),
        Span::styled(
            celebration.headline.clone(),
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   ", Style::new()),
        Span::styled(value, Style::new().fg(accent).add_modifier(Modifier::BOLD)),
    ];
    if area.width >= 120 && !celebration.subject.is_empty() {
        spans.push(Span::styled("   ", Style::new()));
        spans.push(Span::styled(
            celebration.subject.clone(),
            Style::new().fg(theme::text()),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).wrap(Wrap { trim: false }),
        banner,
    );
}

const STEADY: f64 = 0.05;
const HERO_MIN_HEIGHT: u16 = 20;
const SUPPORT_MIN_WIDTH: u16 = 17;
const GARDEN_COLUMN_WIDTH: u16 = 40;

fn draw_cards(app: &mut App, frame: &mut Frame, area: Rect, now: Instant) {
    if app.cards.is_empty() {
        let lines = match &app.status {
            Status::Error(failure) if !app.has_data() => error_lines(failure),
            _ => vec![Line::from(Span::styled(
                "connecting to RevenueCat…",
                Style::new().fg(theme::dim()),
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

    app.rects.clear();
    if app.ui_scale == 0 && draw_home(app, frame, grid, now) {
        return;
    }

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
    let mut cells = Vec::new();
    for (position, index) in visible.iter().copied().enumerate() {
        let row = position / columns;
        let column = position % columns;
        if row >= row_areas.len() {
            break;
        }
        let row_cells = Layout::horizontal(vec![Constraint::Fill(1); columns])
            .spacing(1)
            .split(row_areas[row]);
        if let Some(cell) = row_cells.get(column) {
            cells.push((index, *cell, false));
        }
    }
    draw_card_set(app, frame, &cells, now);
}

/// Draws the cards at their cells; `hero` cards get the previous-period ghost and the large type.
fn draw_card_set(app: &mut App, frame: &mut Frame, cells: &[(usize, Rect, bool)], now: Instant) {
    let currency = app.currency.clone();
    let partial = app.scope == 0 && app.partial();
    let scoped = app.scope > 0 || app.project_count() == 1;
    let scoped_state = app
        .scope
        .checked_sub(1)
        .and_then(|index| app.views.get(index))
        .map(|view| view.state(now, app.stale_after()));
    let focus_visible = app.focus_visible(now);
    for (index, cell, hero) in cells.iter().copied() {
        if let Some(card) = app.cards.get(index) {
            draw_card(
                frame,
                card,
                cell,
                &mut app.rects,
                now,
                CardView {
                    currency: &currency,
                    partial,
                    scoped,
                    scoped_state,
                    focused: focus_visible && app.focus == index,
                    hero,
                },
            );
        }
    }
}

/// The auto layout: a sentence about people and money, the enough line, People and Revenue as heroes,
/// the subscription details as a quieter row, and every app below when there is room.
fn draw_home(app: &mut App, frame: &mut Frame, area: Rect, now: Instant) -> bool {
    let (hero, support): (Vec<usize>, Vec<usize>) =
        (0..app.cards.len()).partition(|index| card_rank(&app.cards[*index].id) <= 1);
    if hero.is_empty() || area.height < 12 || area.width < 40 {
        return false;
    }

    let headline = if area.height >= 18 {
        headline_lines(app, now, area.height >= 22)
    } else {
        Vec::new()
    };
    let headline_height = if headline.is_empty() {
        0
    } else {
        wrapped_height(&headline, area.width) + 1
    };
    let enough_height = if enough_applies(app) && area.height >= 20 {
        2
    } else {
        0
    };
    let widest = support
        .len()
        .min((area.width / SUPPORT_MIN_WIDTH).max(1) as usize)
        .max(1);
    let support_rows = support.len().div_ceil(widest) as u16;
    let support_columns = support.len().div_ceil(support_rows.max(1) as usize).max(1);
    let support_card = if area.height >= 40 {
        9
    } else if area.height >= 28 {
        7
    } else {
        5
    };
    let support_height = support_rows * (support_card + 1);
    let fixed = headline_height + enough_height + support_height;
    if area.height < fixed + 8 {
        return false;
    }

    let garden = app.garden_rows();
    let garden_columns = (area.width / GARDEN_COLUMN_WIDTH).clamp(1, 4) as usize;
    let garden_needed = if app.scope == 0 && garden.len() > 1 {
        2 + garden.len().div_ceil(garden_columns) as u16
    } else {
        0
    };
    let garden_room = area.height.saturating_sub(fixed + HERO_MIN_HEIGHT);
    let garden_height = if garden_needed > 0 && garden_room >= 5 {
        garden_needed.min(garden_room)
    } else {
        0
    };

    let [
        headline_area,
        enough_area,
        hero_area,
        support_area,
        garden_area,
    ] = Layout::vertical([
        Constraint::Length(headline_height),
        Constraint::Length(enough_height),
        Constraint::Fill(1),
        Constraint::Length(support_height),
        Constraint::Length(garden_height),
    ])
    .areas(area);

    if !headline.is_empty() {
        frame.render_widget(
            Paragraph::new(headline).wrap(Wrap { trim: true }),
            Rect {
                x: headline_area.x + 1,
                width: headline_area.width.saturating_sub(2),
                ..headline_area
            },
        );
    }
    if enough_height > 0 {
        draw_enough(app, frame, enough_area, now);
    }

    let mut cells: Vec<(usize, Rect, bool)> = Vec::new();
    let hero_cells = Layout::horizontal(vec![Constraint::Fill(1); hero.len()])
        .spacing(1)
        .split(hero_area);
    for (index, cell) in hero.iter().zip(hero_cells.iter()) {
        cells.push((*index, *cell, true));
    }
    if support_height > 0 {
        let support_area = Rect {
            y: support_area.y + 1,
            height: support_area.height.saturating_sub(1),
            ..support_area
        };
        let rows = Layout::vertical(vec![Constraint::Fill(1); support_rows as usize])
            .spacing(1)
            .split(support_area);
        for (row, chunk) in support.chunks(support_columns).enumerate() {
            let Some(row_area) = rows.get(row) else {
                break;
            };
            let row_cells = Layout::horizontal(vec![Constraint::Fill(1); support_columns])
                .spacing(1)
                .split(*row_area);
            for (index, cell) in chunk.iter().zip(row_cells.iter()) {
                cells.push((*index, *cell, false));
            }
        }
    }
    draw_card_set(app, frame, &cells, now);

    if garden_height > 0 {
        draw_garden(app, frame, garden_area, &garden, garden_columns);
    }
    true
}

fn enough_applies(app: &App) -> bool {
    app.enough.is_some() && (app.scope == 0 || app.project_count() == 1)
}

fn people_word(count: f64) -> &'static str {
    if count.round() as i64 == 1 {
        "person"
    } else {
        "people"
    }
}

/// The home sentence: how many people your work reached and what they paid, then how that compares.
fn headline_lines(app: &App, now: Instant, compare: bool) -> Vec<Line<'static>> {
    let people = app.card(&PEOPLE);
    let revenue = app.card(&["revenue"]);
    let purchases = app
        .card(&[crate::api::PURCHASES])
        .map(|card| card.value.value(now));
    let subject = if app.scope == 0 && app.project_count() > 1 {
        "your apps".to_string()
    } else {
        app.scope_label()
    };
    let words = Style::new().fg(theme::text());
    let number = |id: &str| Style::new().fg(accent(id)).add_modifier(Modifier::BOLD);

    let mut sentence: Vec<Span<'static>> = vec![];
    match (people, revenue) {
        (Some(people), revenue) => {
            let count = people.value.value(now);
            sentence.push(Span::styled(grouped(count), number(&people.id)));
            sentence.push(Span::styled(
                format!(" {} used ", people_word(count)),
                words,
            ));
            sentence.push(Span::styled(subject, words.add_modifier(Modifier::BOLD)));
            sentence.push(Span::styled(" in the last 28 days", words));
            if let Some(revenue) = revenue.filter(|card| card.value.target() > 0.0) {
                sentence.push(Span::styled(" and paid ", words));
                sentence.push(Span::styled(
                    grouped_money(revenue.value.value(now), &app.currency),
                    number("revenue"),
                ));
                if let Some(purchases) = purchases.filter(|count| *count >= 1.0) {
                    sentence.push(Span::styled(" across ", words));
                    sentence.push(Span::styled(grouped(purchases), number("revenue")));
                    sentence.push(Span::styled(
                        if purchases.round() as i64 == 1 {
                            " purchase"
                        } else {
                            " purchases"
                        },
                        words,
                    ));
                }
            }
            sentence.push(Span::styled(".", words));
        }
        (None, Some(revenue)) => {
            sentence.push(Span::styled(
                format!("{} ", capitalize(&subject)),
                words.add_modifier(Modifier::BOLD),
            ));
            sentence.push(Span::styled("earned ", words));
            sentence.push(Span::styled(
                grouped_money(revenue.value.value(now), &app.currency),
                number("revenue"),
            ));
            sentence.push(Span::styled(" in the last 28 days.", words));
        }
        (None, None) => return Vec::new(),
    }

    let mut lines = vec![Line::from(sentence)];
    if compare && let Some(line) = comparison_line(app) {
        lines.push(line);
    }
    if compare && let Some(line) = moment_line(app) {
        lines.push(line);
    }
    lines
}

/// The newest moment of the past week under the headline, in its own colour, with the key that lists them all.
fn moment_line(app: &App) -> Option<Line<'static>> {
    let now = chrono::Utc::now().timestamp_millis();
    let recent = crate::moments::recent(&app.moments.moments, now);
    let newest = recent.first()?;
    let accent = moment_color(newest.kind.tone());
    let mut spans = vec![
        Span::styled(
            format!("{} ", newest.kind.glyph()),
            Style::new().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            newest.title.clone(),
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" · ", Style::new().fg(theme::dim())),
        Span::styled(
            crate::moments::plain(&newest.value),
            Style::new().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" · {}", crate::moments::ago(newest.at, now)),
            Style::new().fg(theme::dim()),
        ),
    ];
    if recent.len() > 1 {
        spans.push(Span::styled(
            format!(" · {} more this week", recent.len() - 1),
            Style::new().fg(theme::dim()),
        ));
    }
    spans.push(Span::styled("   m moments", Style::new().fg(theme::dim())));
    Some(Line::from(spans))
}

/// Fades everything already drawn so a modal reads as the only thing on screen.
fn scrim(frame: &mut Frame, area: Rect) {
    let background = theme::bg();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let Some(cell) = frame.buffer_mut().cell_mut((x, y)) else {
                continue;
            };
            let foreground = blend_color(cell.fg, background, 0.7);
            let fill = blend_color(cell.bg, background, 0.6);
            cell.set_fg(foreground);
            cell.set_bg(fill);
        }
    }
}

fn moment_color(tone: crate::moments::Tone) -> Color {
    match tone {
        crate::moments::Tone::Money => theme::money(),
        crate::moments::Tone::People => theme::retention(),
        crate::moments::Tone::World => theme::acquisition(),
        crate::moments::Tone::Stars => theme::warn(),
        crate::moments::Tone::Alert => theme::bad(),
        crate::moments::Tone::Quiet => theme::dim(),
    }
}

/// The moments list: every record, milestone and first, newest first, with notes and a way to open the card.
fn draw_moments(app: &App, frame: &mut Frame, area: Rect) {
    scrim(frame, area);
    let width = area.width.saturating_sub(8).clamp(40, 112).min(area.width);
    let height = area.height.saturating_sub(4).clamp(8, 40).min(area.height);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let listed = app.moments.listed();
    let block = Block::bordered()
        .border_style(Style::new().fg(theme::border_strong()))
        .style(Style::new().bg(theme::surface()))
        .title(Line::from(vec![
            Span::styled(
                " ✦ Moments ",
                Style::new()
                    .fg(theme::bright())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{} ", listed.len()), Style::new().fg(theme::dim())),
        ]))
        .title_bottom(Line::from(Span::styled(
            " Enter open card   j/k move   m close ",
            Style::new().fg(theme::dim()),
        )));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if listed.is_empty() {
        let lines = vec![
            Line::default(),
            Line::from(Span::styled(
                "  No moments yet.",
                Style::new()
                    .fg(theme::bright())
                    .add_modifier(Modifier::BOLD),
            )),
            Line::default(),
            Line::from(Span::styled(
                "  Best days, enough reached, people milestones, first sales and new countries",
                Style::new().fg(theme::text()),
            )),
            Line::from(Span::styled(
                "  arrive here while the watcher runs in the background:",
                Style::new().fg(theme::text()),
            )),
            Line::default(),
            key_line("rc-tui watch --install", "start it now and at every login"),
            key_line("rc-tui moments --test", "see what a moment looks like"),
        ];
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
        return;
    }
    let now = chrono::Utc::now().timestamp_millis();
    let room = inner.width.saturating_sub(2) as usize;
    let selected = app.moments.selected.min(listed.len() - 1);
    let mut blocks: Vec<Vec<Line<'static>>> = Vec::new();
    for (index, moment) in listed.iter().enumerate() {
        let accent = moment_color(moment.kind.tone());
        let chosen = index == selected;
        let base = if chosen {
            Style::new().bg(theme::selection())
        } else {
            Style::new()
        };
        let date = moment
            .local_time()
            .map(|time| time.format("%-d %b").to_string())
            .unwrap_or_default();
        let value = crate::moments::plain(&moment.value);
        let left = format!(" {date:>6}  {} ", moment.kind.glyph());
        let title_room = room.saturating_sub(left.chars().count() + value.chars().count() + 2);
        let title = truncate(&moment.title, title_room);
        let gap = room
            .saturating_sub(left.chars().count() + title.chars().count() + value.chars().count())
            .max(1);
        let mut lines = vec![Line::from(vec![
            Span::styled(format!(" {date:>6}  "), base.fg(theme::dim())),
            Span::styled(
                format!("{} ", moment.kind.glyph()),
                base.fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(title, base.fg(theme::bright()).add_modifier(Modifier::BOLD)),
            Span::styled(" ".repeat(gap), base),
            Span::styled(value, base.fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled(" ", base),
        ])];
        let indent = "           ";
        let line = truncate(
            &crate::moments::plain(&moment.line),
            room.saturating_sub(indent.len()),
        );
        lines.push(Line::from(vec![
            Span::styled(indent, base),
            Span::styled(line, base.fg(theme::text())),
        ]));
        if let Some(note) = &moment.note {
            lines.push(Line::from(vec![
                Span::styled(indent, base),
                Span::styled(
                    truncate(&format!("“{note}”"), room.saturating_sub(indent.len())),
                    base.fg(theme::dim()).add_modifier(Modifier::ITALIC),
                ),
            ]));
        }
        if chosen {
            let mut meta = vec![crate::moments::ago(moment.at, now)];
            if let Some(app) = &moment.app {
                meta.push(app.clone());
            }
            meta.push(if moment.card.is_some() {
                "Enter opens the card".to_string()
            } else {
                "no card".to_string()
            });
            lines.push(Line::from(vec![
                Span::styled(indent, base),
                Span::styled(
                    truncate(&meta.join(" · "), room.saturating_sub(indent.len())),
                    base.fg(theme::dim()),
                ),
            ]));
        }
        if chosen {
            for line in &mut lines {
                let used = line.width();
                if used < room + 1 {
                    line.spans
                        .push(Span::styled(" ".repeat(room + 1 - used), base));
                }
            }
        }
        lines.push(Line::default());
        blocks.push(lines);
    }
    let rows = inner.height as usize;
    let mut start = 0;
    let used_to =
        |from: usize, to: usize| -> usize { blocks[from..=to].iter().map(Vec::len).sum() };
    while start < selected && used_to(start, selected) > rows {
        start += 1;
    }
    let lines: Vec<Line<'static>> = blocks[start..]
        .iter()
        .flatten()
        .take(rows)
        .cloned()
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Revenue and people against the 28 days before, in words: growth is lit, a dip is said plainly and quietly.
fn comparison_line(app: &App) -> Option<Line<'static>> {
    let quiet = Style::new().fg(theme::dim());
    let revenue = app.card(&["revenue"]);
    if let Some(card) = revenue
        && card.value.target() <= 0.0
        && card.trend.is_none()
    {
        return Some(Line::from(Span::styled(
            "No purchases yet. Every app starts here.",
            quiet,
        )));
    }
    let mut clauses: Vec<Vec<Span<'static>>> = Vec::new();
    let mut days = crate::api::COMPARE_DAYS;
    if let Some(trend) = revenue.and_then(|card| card.trend) {
        days = trend.days;
        clauses.push(change_clause(trend, "revenue", "more", "less"));
    }
    if let Some(trend) = app.card(&PEOPLE).and_then(|card| card.trend) {
        days = trend.days;
        clauses.push(change_clause(trend, "people", "more", "fewer"));
    }
    if clauses.is_empty() {
        return None;
    }
    let mut spans = vec![Span::styled(
        format!("Against the {days} days before: "),
        quiet,
    )];
    for (position, clause) in clauses.into_iter().enumerate() {
        if position > 0 {
            spans.push(Span::styled(", ", quiet));
        }
        spans.extend(clause);
    }
    spans.push(Span::styled(".", quiet));
    Some(Line::from(spans))
}

fn change_clause(
    trend: Trend,
    noun: &'static str,
    more: &'static str,
    less: &'static str,
) -> Vec<Span<'static>> {
    let quiet = Style::new().fg(theme::dim());
    if !trend.ratio.is_finite() {
        return vec![
            Span::styled(noun, quiet),
            Span::styled(" up from none", Style::new().fg(theme::good())),
        ];
    }
    if trend.ratio.abs() < STEADY {
        return vec![Span::styled(format!("{noun} steady"), quiet)];
    }
    let label = change_label(trend);
    if trend.change > 0.0 {
        let words = if label.ends_with('×') {
            format!(" the {noun}")
        } else {
            format!(" {more} {noun}")
        };
        vec![
            Span::styled(
                label,
                Style::new().fg(theme::good()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(words, quiet),
        ]
    } else {
        vec![
            Span::styled(label, quiet),
            Span::styled(format!(" {less} {noun}"), quiet),
        ]
    }
}

/// A relative change in words people feel: a doubling or more reads as a multiple, anything less as a percent.
fn change_label(trend: Trend) -> String {
    let factor = 1.0 + trend.ratio;
    if trend.ratio >= 1.0 && factor < 10.0 {
        format!("{factor:.1}×")
    } else if trend.ratio >= 1.0 {
        format!("{factor:.0}×")
    } else {
        format!("{:.0}%", trend.ratio.abs() * 100.0)
    }
}

fn capitalize(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// A line toward the revenue that covers your life; past it, the rest is room to make things.
fn draw_enough(app: &App, frame: &mut Frame, area: Rect, now: Instant) {
    let (Some(enough), Some(revenue)) = (app.enough, app.card(&["revenue"])) else {
        return;
    };
    let earned = revenue.value.value(now).max(0.0);
    let share = earned / enough;
    let reached = share >= 1.0;
    let ink = if reached {
        theme::good()
    } else {
        accent("revenue")
    };
    let label = if reached {
        " ENOUGH ✓ "
    } else {
        " ENOUGH   "
    };
    let detail = if reached {
        format!(
            "  {} of {} · {} beyond enough ",
            grouped_money(earned, &app.currency),
            grouped_money(enough, &app.currency),
            grouped_money(earned - enough, &app.currency)
        )
    } else {
        format!(
            "  {:.0}% · {} of {} ",
            (share * 100.0).floor(),
            grouped_money(earned, &app.currency),
            grouped_money(enough, &app.currency)
        )
    };
    let bar_width =
        (area.width as usize).saturating_sub(text_width(label) + text_width(&detail) + 1);
    let filled = ((share.min(1.0)) * bar_width as f64).round() as usize;
    let line = Line::from(vec![
        Span::styled(
            label,
            Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
        ),
        Span::styled("━".repeat(filled), Style::new().fg(ink)),
        Span::styled(
            "─".repeat(bar_width.saturating_sub(filled)),
            Style::new().fg(theme::border_strong()),
        ),
        Span::styled(
            detail,
            Style::new().fg(if reached {
                theme::good()
            } else {
                theme::text()
            }),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), Rect { height: 1, ..area });
}

/// Every app, most people first, with a bar for the people it reached and what they paid.
fn draw_garden(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    rows: &[crate::app::GardenRow],
    columns: usize,
) {
    if area.height < 3 || rows.is_empty() {
        return;
    }
    let area = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    let title = Line::from(vec![
        Span::styled(
            " YOUR APPS",
            Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("   {} · most people first", rows.len()),
            Style::new().fg(theme::dim()),
        ),
    ]);
    frame.render_widget(Paragraph::new(title), Rect { height: 1, ..area });

    let capacity = (area.height as usize - 1) * columns;
    let per_column = rows.len().min(capacity).div_ceil(columns);
    let overflow = rows.len().saturating_sub(capacity);
    let shown = if overflow > 0 {
        capacity - 1
    } else {
        rows.len()
    };
    let top = rows
        .iter()
        .filter_map(|row| row.people)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let column_areas = Layout::horizontal(vec![Constraint::Fill(1); columns])
        .spacing(2)
        .split(Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        });
    let people_ink = accent(PEOPLE[0]);
    let longest = rows
        .iter()
        .map(|row| text_width(&row.label))
        .max()
        .unwrap_or(6);
    for (position, row) in rows.iter().take(shown).enumerate() {
        let column = position / per_column.max(1);
        let Some(column_area) = column_areas.get(column) else {
            break;
        };
        let y = column_area.y + (position % per_column.max(1)) as u16;
        let width = column_area.width as usize;
        let numbers_width = 16;
        let name_width = longest
            .clamp(6, 18)
            .min(width.saturating_sub(numbers_width + 6));
        let bar_width = width
            .saturating_sub(name_width + numbers_width + 2)
            .clamp(2, 20);
        let people = row.people.unwrap_or(0.0);
        let halves = ((people / top).sqrt() * (bar_width * 2) as f64).round() as usize;
        let mut bar = "━".repeat(halves / 2);
        if halves % 2 == 1 || (people > 0.0 && halves == 0) {
            bar.push('╸');
        }
        let name_ink = match row.state {
            RowState::Live => theme::text(),
            RowState::Stale => theme::warn(),
            RowState::Failed => theme::bad(),
        };
        let revenue = row.revenue.unwrap_or(0.0);
        let line = Line::from(vec![
            Span::styled(
                format!(" {:<name_width$}", truncate(&row.label, name_width)),
                Style::new().fg(name_ink),
            ),
            Span::styled(format!(" {bar:<bar_width$}"), Style::new().fg(people_ink)),
            Span::styled(
                format!("{:>7}", row.people.map_or("—".to_string(), grouped)),
                Style::new().fg(theme::bright()),
            ),
            Span::styled(
                format!(
                    "{:>9}",
                    row.revenue.map_or("—".to_string(), |value| grouped_money(
                        value,
                        &app.currency
                    ))
                ),
                Style::new().fg(if revenue > 0.0 {
                    accent("revenue")
                } else {
                    theme::dim()
                }),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                y,
                height: 1,
                ..*column_area
            },
        );
    }
    if overflow > 0
        && let Some(column_area) = column_areas.last()
    {
        let y = column_area.y + (per_column.max(1) as u16).saturating_sub(1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                format!(" +{} more · p projects", overflow + 1),
                Style::new().fg(theme::dim()),
            )),
            Rect {
                y,
                height: 1,
                ..*column_area
            },
        );
    }
}

pub(crate) fn grouped(value: f64) -> String {
    let rounded = value.round() as i64;
    let digits = rounded.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    if rounded < 0 { format!("-{out}") } else { out }
}

pub(crate) fn grouped_money(value: f64, currency: &str) -> String {
    format!("{}{}", currency_symbol(currency), grouped(value))
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
    scoped: bool,
    scoped_state: Option<RowState>,
    focused: bool,
    hero: bool,
}

fn card_title(card: &Card) -> String {
    match card.id.as_str() {
        "active_users" | "active_customers" => "PEOPLE".to_string(),
        "revenue" => "REVENUE".to_string(),
        "new_customers" => "FOUND YOU".to_string(),
        crate::api::PURCHASES => "PURCHASES".to_string(),
        "active_subscriptions" => "SUBSCRIBERS".to_string(),
        "mrr" => "RECURRING".to_string(),
        "active_trials" => "IN TRIAL".to_string(),
        _ => card.name.to_uppercase(),
    }
}

/// The days a card's chart shows, plus the stretch before them for the ghost: flow metrics drop today's partial day.
fn chart_window(card: &Card, values: &[f64]) -> (Vec<f64>, Option<Vec<f64>>) {
    let days = crate::api::COMPARE_DAYS;
    let complete = if is_flow(&card.id) && values.len() > 2 {
        &values[..values.len() - 1]
    } else {
        values
    };
    let shown = days.min(complete.len());
    let split = complete.len() - shown;
    let ghost = (split >= shown && shown >= 2).then(|| smooth(&complete[split - shown..split]));
    (complete[split..].to_vec(), ghost)
}

/// A centred weekly mean, so the previous period reads as a level to beat rather than a second set of spikes.
fn smooth(values: &[f64]) -> Vec<f64> {
    (0..values.len())
        .map(|index| {
            let window = &values[index.saturating_sub(3)..(index + 4).min(values.len())];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
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
        scoped,
        scoped_state,
        focused,
        hero,
    } = view;
    rects.insert(card.id.clone(), area);

    let accent = accent(&card.id);
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled("● ", Style::new().fg(accent)),
        Span::styled(
            card_title(card),
            Style::new()
                .fg(if hero { theme::text() } else { theme::dim() })
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ]);
    let border = if focused {
        theme::bright()
    } else if hero {
        theme::border_strong()
    } else {
        theme::border()
    };
    let block = Block::bordered()
        .border_style(Style::new().fg(border))
        .title(title);
    let inner = block.inner(area);
    frame
        .buffer_mut()
        .set_style(area, Style::new().bg(theme::surface()));
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

    let mut meta = vec![
        Span::raw("  "),
        Span::styled(
            card_period(card, currency, scoped),
            Style::new().fg(theme::dim()),
        ),
    ];
    if partial {
        meta.push(Span::styled("*", Style::new().fg(theme::warn())));
    }
    if let Some(chip) = scoped_state.and_then(|state| match state {
        RowState::Failed => Some(("failed · last known", theme::bad())),
        RowState::Stale => Some(("stale", theme::warn())),
        RowState::Live => None,
    }) {
        meta.push(Span::styled("   ·   ", Style::new().fg(theme::border())));
        meta.push(Span::styled(chip.0, Style::new().fg(chip.1)));
    }
    let value_text = if money(card, currency) {
        grouped_money(value, currency)
    } else {
        grouped(value)
    };
    let used: usize = meta.iter().map(Span::width).sum::<usize>() + 8;
    let room = (inner.width as usize).saturating_sub(used) as u16;
    if size != ValueSize::Compact
        && let Some(spans) = trend_spans(card, card.trend, currency, room)
    {
        meta.push(Span::styled("   ·   ", Style::new().fg(theme::border())));
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
                    Style::new().fg(theme::bright()),
                )));
            }
            if size == ValueSize::Big {
                lines.push(Line::default());
            }
            lines.push(Line::from(meta));
            frame.render_widget(Paragraph::new(lines), value_area);
        }
        ValueSize::Compact => {
            let room = inner
                .width
                .saturating_sub(text_width(&value_text) as u16 + 4);
            let mut value_line = vec![
                Span::raw("  "),
                Span::styled(
                    value_text,
                    Style::new()
                        .fg(theme::bright())
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if let Some(spans) = trend_spans(card, card.trend, currency, room) {
                value_line.push(Span::raw("  "));
                value_line.extend(spans);
            }
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

    let (values, ghost) = chart_window(card, &card.series.sample(now));
    let chart = Rect {
        x: chart_area.x.saturating_add(2),
        y: chart_area.y.saturating_add(1),
        width: chart_area.width.saturating_sub(4),
        height: chart_area.height.saturating_sub(1),
    };
    let ghost = ghost.filter(|_| hero || chart.height >= 10);
    if values.len() >= 2 && chart.height >= 1 {
        let style = if matches!(card.id.as_str(), "mrr" | "active_subscriptions") {
            ChartStyle::Band
        } else {
            ChartStyle::Area
        };
        draw_chart(
            frame,
            chart,
            &values,
            ghost.as_deref(),
            accent,
            style,
            false,
        );
        if ghost.is_some() && chart.width >= 36 && chart.height >= 6 {
            let legend = format!("▀▀ the {} days before", crate::api::COMPARE_DAYS);
            let width = text_width(&legend) as u16;
            frame.render_widget(
                Paragraph::new(Span::styled(legend, Style::new().fg(ghost_ink()))),
                Rect {
                    x: chart.right().saturating_sub(width),
                    y: chart_area.y,
                    width,
                    height: 1,
                },
            );
        }
    } else if chart.height >= 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no chart data",
                Style::new().fg(theme::dim()),
            ))),
            chart,
        );
    }
}

fn money(card: &Card, currency: &str) -> bool {
    card.unit == "$" || card.unit.eq_ignore_ascii_case(currency)
}

fn card_period(card: &Card, currency: &str, scoped: bool) -> String {
    let symbol = currency_symbol(currency).trim().to_string();
    match card.id.as_str() {
        "active_users" | "active_customers" => if scoped {
            "used it · 28 days"
        } else {
            "used your apps · 28 days"
        }
        .to_string(),
        "revenue" => format!("{symbol} paid · 28 days"),
        "new_customers" => "new · 28 days".to_string(),
        crate::api::PURCHASES => "28 days".to_string(),
        "active_subscriptions" | "active_trials" => "right now".to_string(),
        "mrr" => format!("{symbol} a month"),
        _ if money(card, currency) => format!("{symbol} {}", period_label(&card.period)),
        _ => period_label(&card.period),
    }
}

/// Change against the stretch before, calm by design: growth is lit, steady and dips stay quiet, nothing is red.
/// The "vs 28 days before" tail is added only when the whole phrase fits in `room` cells.
fn trend_spans(
    card: &Card,
    trend: Option<Trend>,
    currency: &str,
    room: u16,
) -> Option<Vec<Span<'static>>> {
    let trend = trend?;
    let quiet = Style::new().fg(theme::dim());
    let lit = Style::new().fg(theme::good());
    let up = trend.change > 0.0;
    let (head, style) = if !trend.ratio.is_finite() {
        ("▲ new".to_string(), lit)
    } else if trend.ratio.abs() < STEADY {
        ("steady".to_string(), quiet)
    } else {
        let small_base = trend.previous.abs() < 10.0 && !PEOPLE.contains(&card.id.as_str());
        let value = if small_base && money(card, currency) {
            grouped_money(trend.change.abs(), currency)
        } else if small_base {
            grouped(trend.change.abs())
        } else {
            change_label(trend)
        };
        (
            format!("{} {value}", if up { "▲" } else { "▼" }),
            if up { lit } else { quiet },
        )
    };
    let suffix = format!(" vs {} days before", trend.days);
    let suffix = if text_width(&head) + text_width(&suffix) <= room as usize {
        suffix
    } else {
        String::new()
    };
    Some(vec![Span::styled(head, style), Span::styled(suffix, quiet)])
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

/// The previous period's line where no fill covers it.
fn ghost_ink() -> Color {
    shade(theme::dim(), 0.72)
}

fn draw_chart(
    frame: &mut Frame,
    area: Rect,
    values: &[f64],
    ghost: Option<&[f64]>,
    accent: Color,
    style: ChartStyle,
    guides: bool,
) {
    let rows = area.height as usize;
    if area.width == 0 || rows == 0 || values.len() < 2 {
        return;
    }
    let ghost = ghost.filter(|ghost| ghost.len() >= 2);
    let scale: Vec<f64> = values
        .iter()
        .chain(ghost.into_iter().flatten())
        .copied()
        .collect();
    let sub_rows = rows * 2;
    let fill = shade(accent, if style == ChartStyle::Band { 0.22 } else { 0.4 });
    let (low, high) = match style {
        ChartStyle::Band => {
            let min = scale.iter().copied().fold(f64::INFINITY, f64::min);
            let max = scale.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let pad = ((max - min) * 0.5).max(max.abs() * 0.10).max(0.5);
            (min - pad, max + pad)
        }
        ChartStyle::Area => {
            let min = scale.iter().copied().fold(0.0_f64, f64::min);
            let max = scale.iter().copied().fold(0.0_f64, f64::max);
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
                cell.set_symbol("│").set_fg(shade(theme::border(), 1.4));
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

    if let Some(ghost) = ghost {
        for column in 0..area.width {
            let position = column as f64 / width * (ghost.len() - 1) as f64;
            let value = sample(ghost, position);
            let sub = (((value - low) / span) * (sub_rows.saturating_sub(1)) as f64)
                .round()
                .clamp(0.0, sub_rows as f64 - 1.0) as usize;
            paint_ghost(frame.buffer_mut(), area, column, sub, fill);
        }
    }

    if guides && style == ChartStyle::Area && low < 0.0 && high > 0.0 {
        let y = area.bottom() - 1 - (zero / 2) as u16;
        for x in area.left()..area.right() {
            let cell = &mut frame.buffer_mut()[(x, y)];
            if cell.symbol() == " " {
                cell.set_symbol("·").set_fg(theme::dim());
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

/// Draws the previous period as a thin line: grey over empty cells, cut into the fill where you have grown past it.
fn paint_ghost(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    column: u16,
    sub: usize,
    fill: Color,
) {
    let row = sub / 2;
    if row >= area.height as usize {
        return;
    }
    let y = area.bottom() - 1 - row as u16;
    let cell = &mut buffer[(area.x + column, y)];
    let symbol = if sub.is_multiple_of(2) { "▄" } else { "▀" };
    match cell.symbol() {
        " " => {
            cell.set_symbol(symbol).set_fg(ghost_ink());
        }
        "█" => {
            cell.set_symbol(symbol)
                .set_fg(blend_color(fill, theme::surface(), 0.6))
                .set_bg(fill);
        }
        _ => {}
    }
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

    if app.moments.open {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " j/k move   Enter open card   m close   ? help",
                Style::new().fg(theme::dim()),
            ))),
            left,
        );
        return;
    }

    if app.activity.open {
        let (health, color) = feed_health(app);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " j/k move   Tab filter   [ ] range   f close   ? help",
                Style::new().fg(theme::dim()),
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
            Style::new().fg(theme::warn()),
        ))
    } else if hints {
        let keys = match (app.demo, left.width) {
            (true, 56..) => " q quit   r refresh   p projects   f activity   ? help",
            (false, 82..) => {
                " q quit   r refresh   p projects   f activity   m moments   Enter detail   ? help"
            }
            (false, 70..) => {
                " q quit   r refresh   p projects   f activity   Enter detail   ? help"
            }
            (false, 63..) => " q quit  r refresh  p projects  f activity  Enter detail  ? help",
            _ => " q quit  r refresh  p projects  f activity  ? help",
        };
        Line::from(Span::styled(keys, Style::new().fg(theme::dim())))
    } else if app.stale(now) {
        Line::from(Span::styled(
            format!(
                " stale · last update {} · r retry",
                app.last_updated
                    .map_or("never".to_string(), |then| ago(now, then))
            ),
            Style::new().fg(theme::warn()),
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
            Style::new().fg(theme::warn()),
        ))
    } else if app.demo {
        Line::from(Span::styled(
            " demo data · no API calls",
            Style::new().fg(theme::dim()),
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
            Style::new().fg(theme::dim()),
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
        Style::new().fg(theme::dim()),
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
        .border_style(Style::new().fg(theme::border_strong()))
        .style(Style::new().bg(theme::surface()))
        .title(Line::from(vec![
            Span::styled(
                " Projects ",
                Style::new()
                    .fg(theme::bright())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if picker.query.trim().is_empty() {
                    format!("{total} ")
                } else {
                    format!("{} / {total} ", entries.len())
                },
                Style::new().fg(theme::dim()),
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
            Span::styled("  / ", Style::new().fg(theme::dim())),
            Span::styled(picker.query.clone(), Style::new().fg(theme::bright())),
            Span::styled("▌", Style::new().fg(theme::bright())),
        ])),
        query_area,
    );

    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  No projects match that filter.",
                Style::new().fg(theme::dim()),
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
            RowState::Live => ("●", theme::good()),
            RowState::Stale => ("·", theme::warn()),
            RowState::Failed => ("!", theme::bad()),
        };
        let background = if selected {
            theme::selection()
        } else {
            theme::bg()
        };
        let foreground = if selected {
            theme::bright()
        } else {
            theme::text()
        };
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
                Style::new().fg(theme::dim()).bg(background),
            ),
        ]);
        frame.render_widget(Paragraph::new(line).style(Style::new().bg(background)), row);
    }

    let hidden = entries.len().saturating_sub(offset + visible);
    if hidden > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  ↓ {hidden} more"),
                Style::new().fg(theme::dim()),
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
                theme::bad()
            } else {
                theme::dim()
            }),
        ))),
        help_area,
    );
    let _ = now;
}

fn accent_for_state(state: RowState) -> Color {
    match state {
        RowState::Live => accent(""),
        RowState::Stale => theme::warn(),
        RowState::Failed => theme::bad(),
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
        .border_style(Style::new().fg(theme::border_strong()))
        .style(Style::new().bg(theme::surface()))
        .title(Line::from(vec![
            Span::styled(
                " Project health ",
                Style::new()
                    .fg(theme::bright())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}/{} live ", app.live_count(), app.project_count()),
                Style::new().fg(if app.partial() {
                    theme::warn()
                } else {
                    theme::dim()
                }),
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
        let background = if highlighted {
            theme::selection()
        } else {
            theme::bg()
        };
        let (dot, color) = match row.state {
            RowState::Live => ("●", theme::good()),
            RowState::Stale => ("·", theme::warn()),
            RowState::Failed => ("!", theme::bad()),
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
                    .fg(theme::bright())
                    .bg(background)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate(&detail, inner.width.saturating_sub(36) as usize),
                Style::new()
                    .fg(if row.failure.is_some() {
                        theme::bad()
                    } else {
                        theme::dim()
                    })
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
            Style::new().fg(theme::dim()),
        ))),
        help_area,
    );
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let width = area.width.saturating_sub(12).clamp(50, 100);
    let lines = vec![
        dim_line("  Home"),
        key_line("q / Esc", "quit"),
        key_line("r", "refresh now (forces charts)"),
        key_line("p", "project picker"),
        key_line("f", "open the activity feed"),
        key_line("m", "moments: records, milestones and firsts"),
        key_line("← → / ↑ ↓ / Tab", "move card focus"),
        key_line("Enter", "metric detail"),
        key_line("!", "project health"),
        key_line("w", "wall mode (no chrome)"),
        key_line("T", "light / dark theme"),
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
        key_line("[ / ]", "24h / 7d / 28d / all"),
        key_line("g / G", "first / last"),
        Line::default(),
        dim_line("  Moments"),
        key_line("m / Esc", "close moments"),
        key_line("Enter", "open the moment's card"),
        Line::default(),
        dim_line("  People and revenue lead; subscriptions are one way to be paid, not the goal."),
        dim_line("  Trends: the last 28 complete days vs the 28 before, within 5% is steady."),
        dim_line(
            "  The faint line in People and Revenue is the 28 days before. * marks a partial total.",
        ),
        dim_line("  Set enough = <amount> in config.toml to draw your enough line."),
        dim_line(
            "  rc-tui watch --install turns records and firsts into moments, even with rc-tui closed.",
        ),
        dim_line(
            "  UI scale, page, theme, wall, pins and the last project persist in config.toml.",
        ),
    ];
    let height =
        (wrapped_height(&lines, width.saturating_sub(2)) + 2).min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(theme::border_strong()))
        .style(Style::new().bg(theme::surface()))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new()
                    .fg(theme::bright())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("keys and data", Style::new().fg(theme::dim())),
        ]));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn key_line(key: &str, description: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("    {key:<20}"), Style::new().fg(theme::bright())),
        Span::styled(description.to_string(), Style::new().fg(theme::dim())),
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
        Span::styled("← ", Style::new().fg(theme::dim())),
        Span::styled(
            card_title(card),
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("    ", Style::new()),
        Span::styled(
            format_metric(card.value.value(now), &card.unit, &app.currency),
            Style::new()
                .fg(theme::bright())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "   {}",
                card_period(
                    card,
                    &app.currency,
                    app.scope > 0 || app.project_count() == 1
                )
            ),
            Style::new().fg(theme::dim()),
        ),
    ];
    if let Some(spans) = trend_spans(card, card.trend, &app.currency, 120) {
        header_spans.push(Span::styled("   ·   ", Style::new().fg(theme::border())));
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
                            .fg(if active { theme::bg() } else { theme::dim() })
                            .bg(if active {
                                accent(&card.id)
                            } else {
                                theme::bg()
                            }),
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
            Style::new().fg(if detail.error.is_some() {
                theme::bad()
            } else {
                theme::dim()
            }),
        ))
    } else {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        Line::from(vec![
            Span::styled(
                format!("{}   ·   ", detail.window.label()),
                Style::new().fg(theme::dim()),
            ),
            Span::styled(
                format!(
                    "min {}   ·   max {}   ·   avg {}",
                    format_metric(min, &card.unit, &app.currency),
                    format_metric(max, &card.unit, &app.currency),
                    format_metric(avg, &card.unit, &app.currency)
                ),
                Style::new().fg(theme::dim()),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(stats_line), stats);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(area.width as usize),
            Style::new().fg(theme::border()),
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
        draw_chart(frame, plot, &values, None, accent(&card.id), style, true);
        draw_dates(frame, dates, values.len());
    } else if chart_area.height > 1 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                if detail.loading {
                    "loading…"
                } else {
                    "no chart data"
                },
                Style::new().fg(theme::dim()),
            ))),
            center_block(chart_area, 1),
        );
    }

    let rows = app.detail_rows();
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "no per-project data",
                Style::new().fg(theme::dim()),
            ))),
            table_area,
        );
    } else {
        let header = Line::from(vec![
            Span::styled(
                format!("{:<32}", "PROJECT"),
                Style::new().fg(theme::dim()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:>16}", "VALUE"), Style::new().fg(theme::dim())),
            Span::styled(format!("{:>8}", "SHARE"), Style::new().fg(theme::dim())),
            Span::styled(
                format!("{:>10}", format!("{}D", crate::api::COMPARE_DAYS)),
                Style::new().fg(theme::dim()),
            ),
            Span::styled(format!("  {:<8}", "STATE"), Style::new().fg(theme::dim())),
        ]);
        frame.render_widget(Paragraph::new(header), table_area);
        let visible = table_area.height.saturating_sub(1) as usize;
        let selected = detail.selected.min(rows.len().saturating_sub(1));
        let offset = selected.saturating_sub(visible.saturating_sub(1));
        for (index, row) in rows.iter().enumerate().skip(offset).take(visible) {
            let highlighted = index == selected;
            let background = if highlighted {
                theme::selection()
            } else {
                theme::bg()
            };
            let (trend, color) = detail_trend(row.trend);
            let state = match row.state {
                RowState::Live => ("live", theme::good()),
                RowState::Stale => ("stale", theme::warn()),
                RowState::Failed => ("failed", theme::bad()),
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("{:<32}", truncate(&row.label, 32)),
                    Style::new().fg(theme::bright()).bg(background),
                ),
                Span::styled(
                    format!(
                        "{:>16}",
                        format_metric(row.value, &card.unit, &app.currency)
                    ),
                    Style::new().fg(theme::text()).bg(background),
                ),
                Span::styled(
                    format!("{:>8}", format!("{:.0}%", row.share * 100.0)),
                    Style::new().fg(theme::dim()).bg(background),
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
            Style::new().fg(theme::dim()),
        ))),
        footer,
    );
}

fn detail_trend(trend: Option<Trend>) -> (String, Color) {
    let Some(trend) = trend else {
        return ("—".to_string(), theme::dim());
    };
    if !trend.ratio.is_finite() {
        return ("new".to_string(), theme::good());
    }
    if trend.ratio.abs() < STEADY {
        return ("steady".to_string(), theme::dim());
    }
    let up = trend.change > 0.0;
    (
        format!("{} {}", if up { "▲" } else { "▼" }, change_label(trend)),
        if up { theme::good() } else { theme::dim() },
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
        buffer.set_string(area.x + x, area.y, &text, Style::new().fg(theme::dim()));
    }
}

pub(crate) fn format_metric(value: f64, unit: &str, currency: &str) -> String {
    if unit == "$" || unit.eq_ignore_ascii_case(currency) {
        let symbol = currency_symbol(currency);
        let cents = (value.abs().fract() * 100.0).round() as i64;
        if cents == 0 || cents == 100 {
            format!("{symbol}{}", grouped(value))
        } else {
            let sign = if value < 0.0 { "-" } else { "" };
            format!("{sign}{symbol}{}.{cents:02}", grouped(value.abs().trunc()))
        }
    } else {
        grouped(value)
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
    paint_background(frame);
    let now = Instant::now();
    let area = frame.area();
    let block = Block::bordered()
        .border_style(Style::new().fg(theme::border()))
        .style(Style::new().bg(theme::bg()))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new().fg(theme::text()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("import projects", Style::new().fg(theme::dim())),
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
            Style::new().fg(theme::warn()),
        )));
    }
    frame.render_widget(Paragraph::new(intro), intro_area);

    let help = match state.stage {
        ImportStage::Editing => {
            let editor = Block::bordered()
                .border_style(Style::new().fg(theme::border()))
                .title(Span::styled(
                    format!(" {} key(s) ", state.key_count()),
                    Style::new().fg(theme::dim()),
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
                    Style::new().fg(theme::text()),
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
                        Span::styled("  ✓ ", Style::new().fg(theme::good())),
                        Span::styled(
                            truncate(&outcome.label, 28),
                            Style::new().fg(theme::text()).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("  {}", project.id), Style::new().fg(theme::dim())),
                        Span::styled("  metrics ✓ charts ✓", Style::new().fg(theme::good())),
                    ])),
                    (None, Some(failure)) => body.push(Line::from(vec![
                        Span::styled("  ✗ ", Style::new().fg(theme::bad())),
                        Span::styled(
                            format!("{:<28} ", truncate(&outcome.label, 28)),
                            Style::new().fg(theme::text()),
                        ),
                        Span::styled(
                            truncate(&failure.headline(), 42),
                            Style::new().fg(theme::bad()),
                        ),
                        Span::styled(
                            match outcome.charts {
                                Some(true) => "  metrics ✗",
                                Some(false) => "  charts ✗",
                                None => "",
                            },
                            Style::new().fg(theme::bad()),
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
                Style::new().fg(if state.failures() == 0 {
                    theme::good()
                } else {
                    theme::warn()
                }),
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
            Style::new().fg(theme::dim()),
        ))),
        footer_area,
    );
}

pub fn draw_setup(setup: &Setup, frame: &mut Frame) {
    paint_background(frame);
    let area = frame.area();
    let block = Block::bordered()
        .border_style(Style::new().fg(theme::border()))
        .style(Style::new().bg(theme::bg()))
        .title(Line::from(vec![
            Span::styled(
                " rc-tui ",
                Style::new().fg(theme::text()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("setup", Style::new().fg(theme::dim())),
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
                    Style::new().fg(theme::dim()),
                )),
                Line::from(Span::styled(
                    "  This usually takes a second or two.",
                    Style::new().fg(theme::dim()),
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
                    Style::new().fg(theme::text()).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(theme::dim())
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
                    Style::new().fg(theme::good()).add_modifier(Modifier::BOLD),
                )),
                Line::default(),
                dim_line("  Saved when you finish; keys stay in a mode-600 file on this machine."),
                Line::default(),
                Line::from(Span::styled(
                    "  Add another project?",
                    Style::new().fg(theme::text()).add_modifier(Modifier::BOLD),
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
            Style::new().fg(theme::dim()),
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
            Style::new().fg(theme::warn()),
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
        Style::new().fg(theme::text()).add_modifier(Modifier::BOLD),
    ))
}

fn dim_line(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::new().fg(theme::dim()),
    ))
}

fn field_line(label: &str, value: &str, active: bool) -> Line<'static> {
    let label_style = if active {
        Style::new().fg(theme::text()).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme::dim())
    };
    let value_style = if active {
        Style::new().fg(theme::text())
    } else {
        Style::new().fg(theme::dim())
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
        Style::new().fg(theme::bad()).add_modifier(Modifier::BOLD),
    )));
    if let Some(detail) = failure.detail() {
        lines.push(Line::from(Span::styled(
            format!("    {detail}"),
            Style::new().fg(theme::bad()),
        )));
    }
    for hint in failure.hints() {
        lines.push(Line::from(Span::styled(
            format!("    → {hint}"),
            Style::new().fg(theme::dim()),
        )));
    }
    if let Some(doc_url) = &failure.doc_url {
        lines.push(Line::from(Span::styled(
            format!("    docs: {doc_url}"),
            Style::new()
                .fg(theme::dim())
                .add_modifier(Modifier::UNDERLINED),
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
    fn feed_titles_wrap_without_truncation() {
        let title = "Tailscode: Coding Agent Remote — Extraordinary Edition";
        let lines = wrap_text(title, 24);
        assert_eq!(lines.join(" "), title);
        assert!(!lines.iter().any(|line| line.contains('…')));

        let entry = Entry {
            id: "sale:long".to_string(),
            kind: "purchase".to_string(),
            app: title.to_string(),
            purchase: Some(crate::activity::Purchase {
                amount: 149.0,
                currency: "USD".to_string(),
                product: "Tailscode Pro Annual".to_string(),
                first_ever: true,
                first_today: true,
                ..crate::activity::Purchase::default()
            }),
            ..Entry::default()
        };
        let card = make_feed_card(&entry, 44, false);
        let text: String = card
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect();
        assert!(!text.contains('…'));
        assert!(text.contains("Tailscode"));
        assert!(text.contains("Pro Annual"));
        assert!(text.contains("FIRST EVER"));
    }

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
            enough: None,
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
    fn light_mode_paints_the_whole_dashboard() {
        crate::theme::apply(crate::theme::Mode::Light);
        let mut app = App::new(crate::app::AppOptions {
            sources: vec![crate::app::Source::demo("One", "proj_demo_1")],
            currency: "EUR".to_string(),
            interval: Duration::from_secs(300),
            chart_ttl: Duration::from_secs(900),
            motion: crate::app::Motion::Off,
            wall: false,
            pins: Vec::new(),
            ui_scale: 0,
            ui_page: 0,
            feed_path: None,
            feed_seen_at: None,
            enough: None,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.cards.is_empty() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }

        let mut terminal = Terminal::new(TestBackend::new(200, 54)).unwrap();
        terminal.draw(|frame| draw(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let light = crate::theme::palette();
        assert!(
            buffer.content().iter().any(|cell| cell.bg == light.surface),
            "no light card surface was painted"
        );
        assert!(
            buffer.content().iter().any(|cell| cell.fg == light.bright),
            "no bright ink was painted"
        );
        assert!(
            buffer
                .content()
                .iter()
                .all(|cell| cell.bg != Color::Rgb(0x0b, 0x0d, 0x11)),
            "a dark-theme background leaked into light mode"
        );
        crate::theme::apply(crate::theme::Mode::Dark);
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
            enough: None,
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.cards.is_empty() && Instant::now() < deadline {
            app.tick();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(app.cards.len(), 7);
        assert!(matches!(app.status, Status::Idle));

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| draw(&mut app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();

        for title in [
            "PEOPLE",
            "REVENUE",
            "FOUND YOU",
            "PURCHASES",
            "SUBSCRIBERS",
            "RECURRING",
            "IN TRIAL",
        ] {
            assert!(text.contains(title), "missing {title}");
        }
        assert!(text.contains("people used One in the last 28 days and paid"));
        assert!(text.contains("Against the 28 days before:"));
        assert!(text.contains("€ paid · 28 days"));
        assert!(text.contains("● live"));
        assert!(text.contains("next in"));
        assert!(
            text.chars().any(|c| matches!(c, '▄' | '▀' | '█')),
            "no block chart cells rendered"
        );
    }

    fn home_app(sources: Vec<crate::app::Source>, enough: Option<f64>) -> App {
        let mut app = App::new(crate::app::AppOptions {
            sources,
            currency: "EUR".to_string(),
            interval: Duration::from_secs(300),
            chart_ttl: Duration::from_secs(900),
            motion: crate::app::Motion::Off,
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

    fn screen(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(app, frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn home_leads_with_people_and_revenue_and_shows_every_app() {
        let sources = ["Notes", "Habits", "Focus"]
            .iter()
            .enumerate()
            .map(|(index, name)| crate::app::Source::demo(*name, format!("proj{index}")))
            .collect();
        let mut app = home_app(sources, Some(1_000_000.0));
        let text = screen(&mut app, 200, 54);
        assert!(text.contains("people used your apps in the last 28 days and paid €"));
        assert!(text.contains("ENOUGH"));
        assert!(text.contains("of €1,000,000"));
        assert!(text.contains("YOUR APPS"));
        for name in ["Notes", "Habits", "Focus"] {
            assert!(text.contains(name), "garden is missing {name}");
        }
        assert!(text.contains("the 28 days before"));

        app.enough = Some(1.0);
        let text = screen(&mut app, 200, 54);
        assert!(text.contains("ENOUGH ✓"));
        assert!(text.contains("beyond enough"));

        app.select_scope(1);
        let text = screen(&mut app, 200, 54);
        assert!(
            !text.contains("ENOUGH"),
            "enough is a whole-life line, not per app"
        );
        assert!(!text.contains("YOUR APPS"));
    }

    #[test]
    fn home_fits_every_terminal_size_without_dropping_cards() {
        let mut app = home_app(vec![crate::app::Source::demo("One", "proj1")], Some(500.0));
        for (width, height) in [
            (200, 54),
            (140, 44),
            (110, 34),
            (90, 28),
            (80, 24),
            (60, 20),
        ] {
            let text = screen(&mut app, width, height);
            for title in ["PEOPLE", "REVENUE", "IN TRIAL"] {
                assert!(text.contains(title), "{width}x{height} is missing {title}");
            }
        }
    }

    #[test]
    fn trends_stay_calm_when_numbers_fall() {
        let mut app = home_app(vec![crate::app::Source::demo("One", "proj1")], None);
        for card in &mut app.cards {
            card.trend = Some(Trend {
                ratio: -0.4,
                change: -40.0,
                previous: 100.0,
                days: 28,
            });
        }
        let line = comparison_line(&app).expect("comparison");
        let words: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            words,
            "Against the 28 days before: 40% less revenue, 40% fewer people."
        );
        assert!(
            line.spans
                .iter()
                .all(|span| span.style.fg != Some(theme::bad()))
        );
        for card in &app.cards {
            let spans = trend_spans(card, card.trend, "EUR", 80).expect("trend");
            assert!(spans.iter().all(|span| span.style.fg != Some(theme::bad())));
        }
        let (label, color) = detail_trend(app.cards[0].trend);
        assert_eq!(label, "▼ 40%");
        assert_eq!(color, theme::dim());

        app.cards[0].trend = Some(Trend {
            ratio: 0.03,
            change: 3.0,
            previous: 100.0,
            days: 28,
        });
        assert_eq!(detail_trend(app.cards[0].trend).0, "steady");

        let growth = |ratio: f64| Trend {
            ratio,
            change: ratio * 50.0,
            previous: 50.0,
            days: 28,
        };
        assert_eq!(change_label(growth(0.4)), "40%");
        assert_eq!(change_label(growth(2.77)), "3.8×");
        assert_eq!(change_label(growth(14.0)), "15×");
        app.cards[0].trend = Some(growth(8.5));
        app.cards[1].trend = Some(growth(2.77));
        let words: String = comparison_line(&app)
            .expect("comparison")
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(
            words,
            "Against the 28 days before: 3.8× the revenue, 9.5× the people."
        );
        let small_people = Trend {
            previous: 5.0,
            ..growth(8.5)
        };
        let people = trend_spans(&app.cards[0], Some(small_people), "EUR", 80).expect("trend");
        assert!(people[0].content.contains("9.5×"), "{people:?}");
    }

    #[test]
    fn numbers_in_prose_are_grouped() {
        assert_eq!(grouped(14722.4), "14,722");
        assert_eq!(grouped(999.0), "999");
        assert_eq!(grouped(1_000_000.0), "1,000,000");
        assert_eq!(grouped_money(2314.0, "EUR"), "€2,314");
        assert_eq!(format_metric(2314.5, "$", "EUR"), "€2,314.50");
        assert_eq!(format_metric(14.99, "$", "USD"), "$14.99");
        assert_eq!(format_metric(-3.2, "$", "USD"), "-$3.20");
        assert_eq!(format_metric(19.999, "$", "USD"), "$20");
        assert_eq!(format_metric(14722.0, "#", "EUR"), "14,722");
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
