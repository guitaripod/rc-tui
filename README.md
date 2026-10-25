# rc-tui

RevenueCat in a terminal, built around the people your apps reach and the money
that actually arrived: how many people used your apps in the last 28 days, what
they paid across every kind of purchase, how that compares with the 28 days
before, your enough line, and every app at a glance. A live purchase and review
feed, a project picker, 7/28/90-day detail and per-project health sit around it.

[![rc-tui demo](docs/rc-tui-demo.png)](docs/rc-tui-demo.png)

*`rc-tui --demo` — 17 mocked projects, no credentials. Click for full size.*

[Showcase](docs/showcase.png): home, one app, light theme, wall, metric detail, a smaller terminal and the full feed.

## Philosophy

- **People first.** The first number is the people who used your apps; revenue
  sits beside it, and the headline says both in one sentence.
- **Money that arrived.** Revenue over the last 28 days counts every purchase:
  one-time, lifetime and subscriptions. MRR is one small card, not the goal, so
  nothing nudges a product toward a subscription it does not need.
- **Four weeks, not one.** Every trend compares the last 28 complete days with
  the 28 before. Four whole weeks cancel out weekday swings, and the faint line
  in People and Revenue is the 28 days before.
- **Calm by default.** Growth is lit, a change within 5% is steady, and a dip is
  said plainly in grey, never red. A doubling or more reads as a multiple.
- **Enough.** Set `enough` to the 28-day revenue that covers your life; the line
  fills toward it, then shows what is beyond it.
- **Every app counts.** Your apps are listed by the people they reached, so the
  quiet ones stay in view.

## Install

```sh
cargo install --git https://github.com/guitaripod/rc-tui   # ~/.cargo/bin/rc-tui
rc-tui --demo                 # look around without credentials
```

## Setup

Run `rc-tui` once. It asks for a RevenueCat **v2 secret API key** and picks the
project for you.

1. Dashboard → your project → **API keys** → **+ New secret API key**.
2. Grant **Charts & Metrics: Read** (`charts_metrics:overview:read`,
   `charts_metrics:charts:read`). Adding **Project Configuration: Read**
   (`project_configuration:projects:read`) lets rc-tui list your projects.
3. Paste the key; answer `y` to add the next project.

Keys are project-scoped: one key covers every app in its project, so a dozen apps
may need only a few keys. For many projects at once, use `rc-tui --import` and
paste one key per line. Credentials are saved to
`~/.config/rc-tui/config.toml` (mode 600).

```sh
rc-tui --check    # validate every saved project: metrics and charts
```

## Keys

```text
home        q quit · r refresh · p projects · f activity · Enter detail · ? help
            ! health · w wall · T light/dark · x export CSV · a add · i import · s setup
            ←→↑↓/Tab focus · + - UI scale · [ ] page
picker      type to filter · ↑↓ · Enter open · Esc close
            Ctrl+F pin · Ctrl+E edit · Ctrl+D remove (twice)
detail      Esc back · ↑↓ metric · t 7/28/90 days · j/k project · Enter scope
activity    f/Esc/q close · j/k move · Tab filter · [ ] 24h/7d/28d/all · g/G ends
```

`+`/`-` cycles the UI scale: auto is the home layout, then 3, 2 or 1 cards per
page; `[`/`]` changes page. Scale, page, wall, pins and the last project persist in the config. `x`
writes a CSV to `~/.local/share/rc-tui/exports/`.

## Activity

`f` opens the activity feed; on wide terminals it is always visible in a rail
beside the cards. It reads `~/.local/state/storebot/feed.json`, written by
[storebot](https://github.com/guitaripod/storebot). Every row is a timed event:
live sales, renewals, App Store purchases, every written review App Store
Connect exposes, and stars-only ratings when storebot observes a new one. The
rail header keeps the all-time rating count as a single number; it is not a
static block in the stream. Each item is a card with a full title that wraps
instead of truncating; first-ever sales, big sales, five-star reviews and
reviews that need attention light the whole activity section. On first run it
seeds 14 days of paid App Store sales, all available written reviews and the
current two days of hook-log purchases; seed rows are marked as history and are
never counted as new. Apple does not expose individual stars-only ratings, so
only new totals after storebot starts are definite events, not invented ones.

rc-tui only reads the feed: it makes no extra RevenueCat, App Store or Telegram
requests. Without storebot, the rail shows `NOT CONNECTED` and the metric
dashboard is unaffected.

## Config

```toml
currency = "EUR"
chart_minutes = 15      # chart cache; the overview still refreshes every 5 min
enough = 3000           # optional: the 28-day revenue that is enough for you

[[projects]]
id = "proj1a2b3c4d"
api_key = "sk_..."
name = "MagicWeather"
```

Optional keys: `ui_scale = 0..3` (0 auto, 1 = 3/page, 2 = 2/page, 3 = 1/page),
`motion = "full" | "minimal" | "off"`, `theme = "auto" | "light" | "dark"` (auto
reads the terminal background and falls back to dark), `wall = true`,
`pins = ["proj..."]`,
`feed_path = "/path/to/feed.json"` (defaults to the storebot state path).
`feed_seen_at` is the read cursor and is maintained automatically.

## Data

- Cards: `GET /v2/projects/{id}/metrics/overview`; sparklines:
  `GET /v2/projects/{id}/charts/{name}` (57 days: the 28 shown, the 28 they are
  compared with, and today). Purchases are the revenue chart's daily
  transactions, so they cost no extra request; they include renewals and are
  not refunded away.
- "All projects" sums projects locally. A failed project keeps its last known
  values; totals are marked `*`, the header shows `n/17 live`, and `!` names the
  failure.
- Charts cache for 15 minutes; `r` forces a refresh. Charts & Metrics are
  production-only; v1 keys do not work with v2.
- The activity feed is produced by storebot from RevenueCat webhooks, Apple
  server notifications and App Store review polling; rc-tui only reads it.
  Recent transactions and the sandbox toggle still have no public RevenueCat
  API. New and Active Customers can lag by one to two hours.

## License

GPL-3.0
