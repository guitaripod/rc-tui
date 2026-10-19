# rc-tui

RevenueCat's Overview in a terminal: six animated metric cards for one project or
all of them, with sparklines, a project picker, 7/28/90-day metric detail, and
per-project health.

[![rc-tui demo](docs/rc-tui-demo.png)](docs/rc-tui-demo.png)

*`rc-tui --demo` — 17 mocked projects, no credentials. Click for full size.*

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
dashboard   q quit · r refresh · p projects · Enter detail · ! health · ? help
            w wall · x export CSV · a add · i import · s setup · ←→↑↓/Tab focus
picker      type to filter · ↑↓ · Enter open · Esc close
            Ctrl+F pin · Ctrl+E edit · Ctrl+D remove (twice)
detail      Esc back · ↑↓ metric · t 7/28/90 days · j/k project · Enter scope
```

`w` and pins persist in the config. `x` writes a CSV to
`~/.local/share/rc-tui/exports/`.

## Config

```toml
currency = "EUR"
chart_minutes = 15      # chart cache; the overview still refreshes every 5 min

[[projects]]
id = "proj1a2b3c4d"
api_key = "sk_..."
name = "MagicWeather"
```

Optional keys: `motion = "full" | "minimal" | "off"`, `wall = true`,
`pins = ["proj..."]`.

## Data

- Cards: `GET /v2/projects/{id}/metrics/overview`; sparklines:
  `GET /v2/projects/{id}/charts/{name}` (28 days).
- "All projects" sums projects locally. A failed project keeps its last known
  values; totals are marked `*`, the header shows `n/17 live`, and `!` names the
  failure.
- Charts cache for 15 minutes; `r` forces a refresh. Charts & Metrics are
  production-only; v1 keys do not work with v2.
- The Recent transactions feed and the sandbox toggle have no public API. New
  and Active Customers can lag by one to two hours.

## License

GPL-3.0
