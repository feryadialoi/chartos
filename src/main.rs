mod color;
mod grafana;
mod heatmap;
mod histogram;
mod view;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use color::Rgb;
use grafana::{Client, Dashboard, Panel, Series};
use heatmap::Heatmap;
use histogram::Histogram;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, TerminalOptions, Viewport};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use view::{HeatmapView, HistogramView, Quantile, StatView};

#[derive(Parser)]
#[command(version, about = "Grafana dashboards as true color charts in the terminal")]
struct Cli {
    /// Grafana base URL.
    #[arg(long, env = "GRAFANA_URL", default_value = "http://localhost:3000")]
    url: String,
    /// Service account token.
    #[arg(long, env = "GRAFANA_TOKEN", hide_env_values = true)]
    token: Option<String>,
    /// File holding the service account token.
    #[arg(long, conflicts_with = "token")]
    token_file: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the panels of a dashboard.
    Panels { dashboard: String },
    /// Draw one panel, refreshing live.
    Panel {
        dashboard: String,
        panel_id: u32,
        #[arg(long, default_value = "now-30m")]
        from: String,
        #[arg(long, default_value = "now")]
        to: String,
        /// Refresh interval, e.g. 10s, 1m.
        #[arg(long, default_value = "10s", value_parser = parse_duration)]
        refresh: Duration,
        /// Draw once below the prompt and exit.
        #[arg(long)]
        once: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let token = match (&cli.token, &cli.token_file) {
        (Some(t), _) => t.clone(),
        (None, Some(path)) => std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))?
            .trim()
            .to_string(),
        (None, None) => bail!("no token: set GRAFANA_TOKEN, --token or --token-file"),
    };
    let client = Client::new(&cli.url, &token)?;

    match cli.cmd {
        Cmd::Panels { dashboard } => {
            let dash = client.dashboard(&dashboard)?;
            println!("{} ({})", dash.title, dash.uid);
            for p in &dash.panels {
                println!("{:>4}  {:<11} {}", p.id, p.kind, p.title);
            }
            Ok(())
        }
        Cmd::Panel { dashboard, panel_id, from, to, refresh, once } => {
            let dash = client.dashboard(&dashboard)?;
            let panel = dash
                .panels
                .iter()
                .find(|p| p.id == panel_id)
                .cloned()
                .with_context(|| format!("dashboard {dashboard} has no panel {panel_id}"))?;
            let mut app = App::new(dash, panel, refresh);
            if once {
                let series = client.query_panel(&app.panel, &from, &to, 200);
                app.update(series);
                return run_once(&app);
            }
            run_live(client, app, from, to)
        }
    }
}

struct App {
    dash: Dashboard,
    panel: Panel,
    unit: Option<String>,
    refresh: Duration,
    data: Option<Data>,
    error: Option<String>,
    updated: Option<Instant>,
}

impl App {
    fn new(dash: Dashboard, panel: Panel, refresh: Duration) -> Self {
        // Prometheus histograms name their unit; fall back to it when the panel sets none.
        let unit = panel.unit().map(str::to_string).or_else(|| {
            let seconds = panel.targets.iter().any(|t| t["expr"].as_str().unwrap_or("").contains("_seconds"));
            seconds.then(|| "s".to_string())
        });
        Self { dash, panel, unit, refresh, data: None, error: None, updated: None }
    }

    fn update(&mut self, result: Result<Vec<Series>>) {
        match result.and_then(|series| Data::build(&self.panel.kind, series)) {
            Ok(d) => {
                self.data = Some(d);
                self.error = None;
                self.updated = Some(Instant::now());
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn quantiles(&self) -> Vec<Quantile> {
        let Some(Data::Histogram(h)) = &self.data else { return vec![] };
        [("p50", Rgb(129, 212, 250)), ("p95", Rgb(206, 147, 216)), ("p99", Rgb(255, 255, 255))]
            .into_iter()
            .filter_map(|(label, color)| {
                let q: f64 = label[1..].parse::<f64>().ok()? / 100.0;
                Some(Quantile { label, value: h.quantile(q)?, color })
            })
            .collect()
    }

    fn draw(&self, frame: &mut Frame, status: bool) {
        let [main, footer] =
            Layout::vertical([Constraint::Min(5), Constraint::Length(u16::from(status))]).areas(frame.area());
        let quantiles = self.quantiles();
        let unit = self.unit.as_deref();

        let mut legend = vec![Span::raw(" ")];
        if let Some(Data::Histogram(h)) = &self.data {
            legend.push(Span::raw(format!("n={} ", view::compact(h.total()))));
        }
        for q in &quantiles {
            legend.push(Span::styled("▲ ", Style::default().fg(q.color.to_color())));
            legend.push(Span::raw(format!("{} {}  ", q.label, view::fmt_value(q.value, unit))));
        }
        let block = Block::bordered()
            .border_style(Style::default().fg(Rgb(70, 80, 78).to_color()))
            .title(Line::from(vec![
                Span::raw(" "),
                Span::styled(&self.dash.title, Style::default().fg(Rgb(120, 128, 126).to_color())),
                Span::raw(" / "),
                Span::styled(&self.panel.title, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" "),
            ]))
            .title_bottom(Line::from(legend));
        let inner = block.inner(main);
        frame.render_widget(block, main);

        match &self.data {
            Some(Data::Histogram(hist)) => frame.render_widget(HistogramView { hist, unit, quantiles: &quantiles }, inner),
            Some(Data::Heatmap(hm)) => frame.render_widget(HeatmapView { hm, unit }, inner),
            Some(Data::Stat(s)) => frame.render_widget(StatView { series: s, unit, color: color::SERIES[0] }, inner),
            Some(Data::Timeseries(series, points)) => frame.render_widget(view::timeseries(series, points, unit), inner),
            None if self.error.is_none() => frame.render_widget(Paragraph::new("loading…"), inner),
            None => {}
        }

        let mut line = vec![];
        if let Some(err) = &self.error {
            line.push(Span::styled(format!("{err}  "), Style::default().fg(Rgb(229, 57, 53).to_color())));
        }
        if status {
            let age = self.updated.map(|t| format!("updated {}s ago", t.elapsed().as_secs())).unwrap_or_default();
            line.push(Span::styled(
                format!("{age} · every {}s · r refresh · q quit", self.refresh.as_secs()),
                Style::default().fg(Rgb(120, 128, 126).to_color()),
            ));
            frame.render_widget(Paragraph::new(Line::from(line)), footer);
        } else if !line.is_empty() {
            frame.render_widget(Paragraph::new(Line::from(line)), inner);
        }
    }
}

/// What a panel draws, decided by its Grafana panel type.
enum Data {
    Histogram(Histogram),
    Heatmap(Heatmap),
    Stat(Series),
    /// Series plus their points as (millis, value) for the chart widget.
    Timeseries(Vec<Series>, Vec<Vec<(f64, f64)>>),
}

impl Data {
    fn build(kind: &str, series: Vec<Series>) -> Result<Self> {
        let none = || anyhow::anyhow!("panel type '{kind}': query returned no data");
        let no_le = || anyhow::anyhow!("panel type '{kind}' needs Prometheus histogram buckets (`le` label)");
        Ok(match kind {
            "bargauge" | "histogram" => Data::Histogram(Histogram::from_series(&series).ok_or_else(no_le)?),
            "heatmap" => Data::Heatmap(Heatmap::from_series(&series).ok_or_else(no_le)?),
            "stat" | "gauge" => Data::Stat(series.into_iter().next().ok_or_else(none)?),
            "timeseries" | "graph" => {
                if series.is_empty() {
                    return Err(none());
                }
                let points = series.iter().map(|s| s.points.iter().map(|p| (p.0 as f64, p.1)).collect()).collect();
                Data::Timeseries(series, points)
            }
            other => bail!("panel type '{other}' is not supported yet"),
        })
    }
}

fn run_once(app: &App) -> Result<()> {
    let mut terminal = ratatui::try_init_with_options(TerminalOptions { viewport: Viewport::Inline(20) })
        .context("chartos draws to a terminal; stdout is not one")?;
    let drawn = terminal.draw(|f| app.draw(f, false)).map(|_| ());
    ratatui::restore();
    println!();
    drawn?;
    match &app.error {
        Some(e) => bail!("{e}"),
        None => Ok(()),
    }
}

fn run_live(client: Client, mut app: App, from: String, to: String) -> Result<()> {
    let (data_tx, data_rx) = mpsc::channel();
    let (kick_tx, kick_rx) = mpsc::channel::<()>();
    let panel = app.panel.clone();
    let every = app.refresh;
    std::thread::spawn(move || loop {
        if data_tx.send(client.query_panel(&panel, &from, &to, 200)).is_err() {
            return;
        }
        match kick_rx.recv_timeout(every) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    });

    let mut terminal = ratatui::try_init().context("chartos draws to a terminal; stdout is not one")?;
    let result = (|| -> Result<()> {
        loop {
            while let Ok(r) = data_rx.try_recv() {
                app.update(r);
            }
            terminal.draw(|f| app.draw(f, true))?;
            if event::poll(Duration::from_millis(250))?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('r') => {
                        let _ = kick_tx.send(());
                    }
                    _ => {}
                }
            }
        }
    })();
    ratatui::restore();
    result
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: u64 = num.parse().map_err(|_| format!("bad duration '{s}'"))?;
    match unit {
        "ms" => Ok(Duration::from_millis(n)),
        "s" | "" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_secs(n * 60)),
        "h" => Ok(Duration::from_secs(n * 3600)),
        _ => Err(format!("bad duration unit in '{s}', use ms, s, m or h")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(panel_id: u32) -> App {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dashboard.json");
        let body: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let dash: Dashboard = serde_json::from_value(body["dashboard"].clone()).unwrap();
        let panel = dash.panels.iter().find(|p| p.id == panel_id).unwrap().clone();
        App::new(dash, panel, Duration::from_secs(10))
    }

    #[test]
    fn data_follows_panel_type() {
        let mut a = app(3);
        let series = Series {
            name: "0.5".into(),
            labels: [("route".to_string(), "/api/orders".to_string())].into(),
            points: vec![(0, 1.0)],
        };
        a.update(Ok(vec![series]));
        assert!(matches!(a.data, Some(Data::Timeseries(..))));
        let mut h = app(5);
        h.update(Ok(vec![]));
        assert!(h.data.is_none());
        assert!(h.error.unwrap().contains("`le` label"));
    }

    #[test]
    fn histogram_panel_infers_seconds() {
        let a = app(5);
        assert_eq!(a.unit.as_deref(), Some("s"));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_secs(120)));
        assert_eq!(parse_duration("500ms"), Ok(Duration::from_millis(500)));
        assert!(parse_duration("10x").is_err());
    }
}
