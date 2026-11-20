# ChartOS

Grafana dashboards drawn in the terminal, in 24-bit color.

ChartOS reads a dashboard from the Grafana HTTP API, runs each panel's queries through Grafana (`/api/ds/query`), and draws the result with [ratatui](https://ratatui.rs). Because queries go through Grafana, the dashboard's datasources work without ChartOS knowing each one.

```
┌ ChartOS dev / Latency histogram (last 5m) ─────────────────────────────┐
│                              4.4k                                      │
│                           █████████                                    │
│                    3.6k   █████████                                    │
│                 ▁▁▁▁▁▁▁▁▁ █████████    2.2k                            │
│                 █████████ █████████ ▂▂▂▂▂▂▂▂▂    1.9k                  │
│        2.2k     █████████ █████████ █████████ ▂▂▂▂▂▂▂▂▂                │
│     ▆▆▆▆▆▆▆▆▆   █████████ █████████ █████████ █████████                │
│                                  ▲                   ▲      ▲          │
│      50ms         100ms     250ms     500ms       1s      2.5s     5s  │
└ n=17k ▲ p50 149ms  ▲ p95 2.05s  ▲ p99 4.27s  ──────────────────────────┘
updated 2s ago · every 5s · r refresh · q quit
```

## Install

Requires Rust (`cargo`).

```sh
curl -fsSL https://raw.githubusercontent.com/feryadialoi/chartos/main/install.sh | sh
```

| Flag | Effect |
|---|---|
| `--skill` | Also install the agent skill to `~/.claude/skills/chartos` (override with `SKILL_DIR`) |
| `--skill-only` | Install only the agent skill |
| `--ref REF` | Install from a branch or tag instead of `main` |

Pass flags with `sh -s --`, for example `... | sh -s -- --skill`. From a clone, `./install.sh` builds the local checkout.

## Agent skill

`skills/chartos/SKILL.md` teaches a coding agent (Claude Code and others that read `SKILL.md`) how to find panels, take `--once` snapshots instead of blocking on the live view, and report quantiles from the footer.

## Quick start

Requires Rust (edition 2024) and Docker for the local stack.

```sh
# 1. Local Grafana + Prometheus + synthetic latency data
cd dev && docker compose up -d && cd ..

# 2. Read-only service account token, saved to dev/.grafana-token (gitignored)
dev/token.sh > dev/.grafana-token

# 3. Build and run
cargo build --release
./target/release/chartos --token-file dev/.grafana-token panels chartos-dev
./target/release/chartos --token-file dev/.grafana-token panel chartos-dev 5
```

## Usage

```
chartos [--url URL] [--token TOKEN | --token-file PATH] <command>

chartos panels <dashboard-uid>                 List panels: id, type, title
chartos panel <dashboard-uid> <panel-id>       Draw one panel, refreshing live
    --from now-30m      Start of the time range (any Grafana time string)
    --to now            End of the time range
    --refresh 10s       Refresh interval: ms, s, m or h
    --once              Draw one frame below the prompt and exit
```

| Option | Environment | Default |
|---|---|---|
| `--url` | `GRAFANA_URL` | `http://localhost:3000` |
| `--token` | `GRAFANA_TOKEN` | none |

Keys in the live view: `r` refreshes now, `q` or `Esc` quits.

To view a whole dashboard, run one `chartos panel` per terminal pane.

## Panel support

| Grafana panel type | Drawn as |
|---|---|
| `bargauge`, `histogram` | Bars from Prometheus `le` buckets, with p50, p95 and p99 markers |
| `heatmap` | Time by bucket grid, two pixels per cell (`▀` with separate fg and bg colors) |
| `stat`, `gauge` | Large digits with a filled sparkline |
| `timeseries`, `graph` | Braille line chart with a legend |

Other panel types show "not supported yet".

Histogram and heatmap panels need queries that return classic Prometheus buckets, for example `sum by (le) (increase(x_bucket[5m]))`. ChartOS turns the cumulative counts into per-bucket counts. Quantiles use the same interpolation as PromQL `histogram_quantile`.

Units: `s` and `reqps` are formatted. If a panel sets no unit and its query mentions `_seconds`, seconds are assumed.

## Color

All colors are 24-bit when `COLORTERM` is `truecolor` or `24bit`. Otherwise each color is mapped to the nearest entry in the 256-color palette.

- Histogram bars: teal to red by latency, brighter toward the top.
- Heatmap: plasma gradient, square-root scaled so sparse slow buckets stay visible.
- Series: Grafana's classic palette order.

## Local dev stack

`dev/` holds a Docker Compose project, `chartos-dev`:

| Service | Address | Notes |
|---|---|---|
| Grafana 11.4.0 | http://localhost:3000 | admin / admin; Prometheus datasource and the `chartos-dev` dashboard are provisioned |
| Prometheus v3.1.0 | http://localhost:9090 | scrapes itself, Grafana and the demo exporter every 5s |
| demo | internal :8000 | `dev/demo/exporter.py`, standard library only |

The demo exporter simulates about 60 requests per second over three routes with log-normal latencies. The median drifts on a 10-minute cycle and a slow tail spikes for 30 seconds every 4 minutes, so histograms and heatmaps have shape and movement.

Stop the stack with `docker compose down` from `dev/`.

## Code layout

| File | Purpose |
|---|---|
| `src/main.rs` | CLI, panel type dispatch, live refresh loop |
| `src/grafana.rs` | Grafana client and data frame parsing |
| `src/histogram.rs` | Bucket counts and quantiles at one point in time |
| `src/heatmap.rs` | Bucket counts over time |
| `src/view.rs` | Widgets: histogram, heatmap, stat, timeseries |
| `src/color.rs` | Gradients, palette, 256-color fallback |
| `tests/fixtures/` | Real Grafana responses used by unit tests |

## Development

```sh
cargo test
cargo clippy --all-targets
```

## Not done yet

- One command that lays out a whole dashboard using Grafana's grid positions
- Dashboard template variables
- Loki and log panels, tables
- `--once` is untested in a real terminal
