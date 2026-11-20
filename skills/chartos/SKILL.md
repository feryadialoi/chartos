---
name: chartos
description: Draw Grafana dashboard panels as true color charts in the terminal with the chartos CLI. Use when the user wants to look at a Grafana panel, histogram, heatmap, stat or time series from the terminal, check latency percentiles (p50, p95, p99) of a Prometheus histogram, list the panels of a Grafana dashboard, or snapshot a panel into a terminal pane while debugging.
---

# chartos

`chartos` reads a dashboard from the Grafana HTTP API, runs each panel's queries through Grafana (`/api/ds/query`), and draws the result in the terminal with 24-bit color. Any datasource Grafana can query works.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/feryadialoi/chartos/main/install.sh | sh
```

Needs Rust (`cargo`). Check with `command -v chartos` before installing.

## Connect

| Option | Environment | Default |
|---|---|---|
| `--url` | `GRAFANA_URL` | `http://localhost:3000` |
| `--token` | `GRAFANA_TOKEN` | none |
| `--token-file PATH` | | none |

The token is a Grafana service account token; the Viewer role is enough. Prefer `--token-file` or `GRAFANA_TOKEN` over `--token` so the token stays out of shell history and process lists. Never print the token.

## Commands

```sh
chartos panels <dashboard-uid>                 # list panels: id, type, title
chartos panel <dashboard-uid> <panel-id>       # live view, refreshes every 10s
    --from now-30m   --to now                  # any Grafana time string
    --refresh 10s                              # ms, s, m or h
    --once                                     # draw one frame and exit
```

The dashboard uid is the part of the Grafana URL after `/d/`: `https://grafana.example.com/d/abc123/my-dashboard` has uid `abc123`.

## How to use it as an agent

1. Find the panel: `chartos panels <uid>`. Pick the id by title and type.
2. chartos needs a real terminal. Every draw, `--once` included, fails with `chartos draws to a terminal; stdout is not one` when stdout is a pipe or a non-interactive agent shell.
3. Run it in a separate terminal pane and read that pane's screen. In Rex, open a pane beside your own, then capture it:

```sh
rex split -s "$REX_SESSION" -b "$REX_BLOCK" --focus=false --keep-open --label chartos \
  -- chartos --token-file ~/.grafana-token panel abc123 5 --refresh 5s
rex capture -b <new block id>
```

The live view waits for `q`, so never run it in your own foreground shell. Use `--once` only in a pane the user is looking at, when one frame is enough.

Read the footer of histogram panels for the numbers: `n=17k ▲ p50 149ms ▲ p95 2.05s ▲ p99 4.27s`. Report those values exactly; do not estimate from bar heights.

## Panel types

| Grafana type | Drawn as |
|---|---|
| `bargauge`, `histogram` | bars from Prometheus `le` buckets with p50, p95, p99 markers |
| `heatmap` | time by bucket grid |
| `stat`, `gauge` | large digits with a sparkline |
| `timeseries`, `graph` | braille line chart with legend |

Anything else shows "not supported yet": tell the user, do not guess the data.

Histogram and heatmap panels need classic bucket queries, for example `sum by (le) (increase(x_bucket[5m]))`. Quantiles use the same interpolation as PromQL `histogram_quantile`.

## Errors

| Message | Meaning |
|---|---|
| `no token: set GRAFANA_TOKEN, --token or --token-file` | no credentials given |
| `dashboard X has no panel N` | wrong id; run `chartos panels X` |
| HTTP 401 / 403 | token invalid or lacks access to that dashboard |
| connection refused | wrong `--url`, or Grafana is down |

Colors are 24-bit when `COLORTERM` is `truecolor` or `24bit`, otherwise mapped to 256 colors.
