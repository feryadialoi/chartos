//! Terminal widgets. Bars use eighth blocks for sub-cell height and a per-row
//! true color gradient.

use crate::color::{self, Rgb};
use crate::grafana::Series;
use crate::heatmap::Heatmap;
use crate::histogram::Histogram;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::Span;
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, LegendPosition, Widget};

const EIGHTHS: [&str; 8] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇"];

pub struct Quantile {
    pub label: &'static str,
    pub value: f64,
    pub color: Rgb,
}

pub struct HistogramView<'a> {
    pub hist: &'a Histogram,
    pub unit: Option<&'a str>,
    pub quantiles: &'a [Quantile],
}

impl Widget for HistogramView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let n = self.hist.buckets.len() as u16;
        // Rows: count labels, bars, quantile markers, bucket labels.
        if n == 0 || area.height < 5 || area.width < n {
            buf.set_string(area.x, area.y, "too small to draw", Style::default());
            return;
        }
        let gap = if area.width >= n * 3 { 1 } else { 0 };
        let bar_w = (area.width + gap) / n - gap;
        let span = bar_w + gap;
        let left = area.x + (area.width - (n * span - gap)) / 2;
        let bars_top = area.y + 1;
        let bars_rows = area.height - 3;
        let marker_y = bars_top + bars_rows;
        let label_y = marker_y + 1;

        let max = self.hist.buckets.iter().map(|b| b.count).fold(0.0, f64::max);
        let dim = Rgb(120, 128, 126).to_color();

        for (i, b) in self.hist.buckets.iter().enumerate() {
            let x = left + i as u16 * span;
            let base = color::latency(if n > 1 { i as f64 / (n - 1) as f64 } else { 0.0 });
            let eighths = if max > 0.0 { (b.count / max * f64::from(bars_rows) * 8.0).round() as u16 } else { 0 };
            let (full, part) = (eighths / 8, eighths % 8);

            for row in 0..bars_rows {
                let y = marker_y - 1 - row;
                let symbol = if row < full {
                    "█"
                } else if row == full && part > 0 {
                    EIGHTHS[part as usize]
                } else {
                    continue;
                };
                // Darker at the base, brighter toward the top of the plot.
                let shade = 0.55 + 0.45 * f64::from(row + 1) / f64::from(bars_rows);
                let fg = base.scale(shade).to_color();
                for dx in 0..bar_w {
                    buf[(x + dx, y)].set_symbol(symbol).set_style(Style::default().fg(fg));
                }
            }

            let top_row = full + u16::from(part > 0);
            let count_y = (marker_y - 1).saturating_sub(top_row).max(area.y);
            put_centered(buf, x, span, count_y, &compact(b.count), Style::default().fg(base.to_color()));
            put_centered(buf, x, span, label_y, &fmt_bound(b.upper, self.unit), Style::default().fg(dim));
        }

        for q in self.quantiles {
            let pos = self.hist.position(q.value);
            let i = pos.floor();
            let x = left + (i as u16).min(n - 1) * span + ((pos - i) * f64::from(bar_w)).round() as u16;
            let x = x.min(left + n * span - gap - 1);
            buf[(x, marker_y)].set_symbol("▲").set_style(Style::default().fg(q.color.to_color()));
        }
    }
}

/// Buckets top to bottom (slowest at top), time left to right. Each cell is two
/// vertical pixels drawn as `▀` with separate fg and bg colors.
pub struct HeatmapView<'a> {
    pub hm: &'a Heatmap,
    pub unit: Option<&'a str>,
}

impl Widget for HeatmapView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let hm = self.hm;
        let nb = hm.bounds.len();
        let nt = hm.times.len();
        let labels: Vec<String> = hm.bounds.iter().map(|b| fmt_bound(*b, self.unit)).collect();
        let axis_w = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16 + 1;
        if nb == 0 || nt == 0 || area.height < 3 || area.width <= axis_w + 4 {
            buf.set_string(area.x, area.y, "too small to draw", Style::default());
            return;
        }
        let plot = Rect::new(area.x + axis_w, area.y, area.width - axis_w, area.height - 1);
        let px_rows = usize::from(plot.height) * 2;
        let max = hm.max();
        let dim = Style::default().fg(color::DIM.to_color());

        let pixel = |t: usize, p: usize| -> Rgb {
            let b = nb - 1 - (p * nb / px_rows).min(nb - 1);
            let v = hm.counts[t][b];
            // sqrt keeps the sparse tail visible next to the busy middle buckets.
            if v <= 0.0 || max <= 0.0 { color::EMPTY } else { color::heat(0.15 + 0.85 * (v / max).sqrt()) }
        };
        for x in 0..plot.width {
            let t = (usize::from(x) * nt / usize::from(plot.width)).min(nt - 1);
            for row in 0..plot.height {
                let p = usize::from(row) * 2;
                buf[(plot.x + x, plot.y + row)]
                    .set_symbol("▀")
                    .set_style(Style::default().fg(pixel(t, p).to_color()).bg(pixel(t, p + 1).to_color()));
            }
        }

        // Y labels at each bucket's centre row, skipping ones that would collide.
        let mut last_row = None;
        for b in (0..nb).rev() {
            let centre_px = (nb - 1 - b) * px_rows / nb + px_rows / nb / 2;
            let row = (centre_px / 2) as u16;
            if last_row.is_some_and(|r| row <= r) {
                continue;
            }
            let l = &labels[b];
            buf.set_string(area.x + axis_w - 1 - l.chars().count() as u16, plot.y + row, l, dim);
            last_row = Some(row);
        }
        time_axis(buf, plot.x, plot.width, area.y + area.height - 1, &hm.times, dim);
    }
}

/// Big digits for the number, the unit in normal text, a sparkline below.
pub struct StatView<'a> {
    pub series: &'a Series,
    pub unit: Option<&'a str>,
    pub color: Rgb,
}

impl Widget for StatView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let Some(v) = self.series.last() else {
            buf.set_string(area.x, area.y, "no data", Style::default());
            return;
        };
        let text = fmt_value(v, self.unit);
        let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(text.len());
        let (num, suffix) = text.split_at(split);
        let big_w: u16 = num.chars().map(|c| if c == '.' { 2 } else { 4 }).sum();
        let total_w = big_w + suffix.chars().count() as u16;

        let spark_h = area.height.saturating_sub(6).min(area.height / 2);
        if area.height >= 5 && area.width >= total_w {
            let x0 = area.x + (area.width - total_w) / 2;
            let y0 = area.y + (area.height - spark_h).saturating_sub(5) / 2;
            let mut x = x0;
            for c in num.chars() {
                let glyph = big_glyph(c);
                for (dy, line) in glyph.iter().enumerate() {
                    // Top of the digits brightest, fading toward the base.
                    let shade = self.color.scale(1.0 - 0.1 * dy as f64).to_color();
                    buf.set_string(x, y0 + dy as u16, line, Style::default().fg(shade));
                }
                x += glyph[0].chars().count() as u16 + 1;
            }
            buf.set_string(x, y0 + 4, suffix, Style::default().fg(self.color.to_color()).add_modifier(Modifier::BOLD));
        } else {
            let style = Style::default().fg(self.color.to_color()).add_modifier(Modifier::BOLD);
            put_centered(buf, area.x, area.width, area.y + area.height / 2, &text, style);
        }

        if spark_h >= 2 {
            let spark = Rect::new(area.x, area.y + area.height - spark_h, area.width, spark_h);
            let values: Vec<f64> = self.series.points.iter().map(|p| p.1).collect();
            sparkline(buf, spark, &values, self.color);
        }
    }
}

/// Filled area sparkline: values resampled to the width, eighth-block tops, faded fill.
fn sparkline(buf: &mut Buffer, area: Rect, values: &[f64], base: Rgb) {
    if values.is_empty() {
        return;
    }
    let (lo, hi) = values.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let range = (hi - lo).max(f64::EPSILON);
    for x in 0..area.width {
        let v = values[(usize::from(x) * values.len() / usize::from(area.width)).min(values.len() - 1)];
        // Keep a sliver at the minimum so the line never disappears.
        let eighths = ((0.1 + 0.9 * (v - lo) / range) * f64::from(area.height) * 8.0).round() as u16;
        let (full, part) = (eighths / 8, eighths % 8);
        for row in 0..area.height {
            let symbol = if row < full {
                "█"
            } else if row == full && part > 0 {
                EIGHTHS[part as usize]
            } else {
                break;
            };
            let shade = 0.25 + 0.5 * f64::from(row + 1) / f64::from(area.height);
            buf[(area.x + x, area.y + area.height - 1 - row)]
                .set_symbol(symbol)
                .set_style(Style::default().fg(base.scale(shade).to_color()));
        }
    }
}

/// Braille line chart, one dataset per series.
pub fn timeseries<'a>(series: &'a [Series], data: &'a [Vec<(f64, f64)>], unit: Option<&str>) -> Chart<'a> {
    let (t0, t1) = data.iter().flatten().fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let y_max = data.iter().flatten().map(|p| p.1).fold(0.0, f64::max) * 1.1;
    let y_max = if y_max > 0.0 { y_max } else { 1.0 };
    let dim = Style::default().fg(color::DIM.to_color());
    let datasets = series
        .iter()
        .zip(data)
        .enumerate()
        .map(|(i, (s, d))| {
            Dataset::default()
                .name(s.name.clone())
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(color::SERIES[i % color::SERIES.len()].to_color()))
                .data(d)
        })
        .collect();
    let now = t1;
    let x_labels: Vec<Span> = [t0, (t0 + t1) / 2.0, t1].iter().map(|t| Span::styled(fmt_age((now - t) as i64), dim)).collect();
    let y_labels: Vec<Span> =
        [0.0, y_max / 2.0, y_max].iter().map(|v| Span::styled(fmt_value(*v, unit), dim)).collect();
    Chart::new(datasets)
        .x_axis(Axis::default().bounds([t0, t1]).labels(x_labels).style(dim))
        .y_axis(Axis::default().bounds([0.0, y_max]).labels(y_labels).style(dim))
        .legend_position(Some(LegendPosition::TopLeft))
        .hidden_legend_constraints((Constraint::Ratio(1, 1), Constraint::Ratio(1, 1)))
}

fn time_axis(buf: &mut Buffer, x: u16, width: u16, y: u16, times: &[i64], style: Style) {
    let (Some(first), Some(last)) = (times.first(), times.last()) else { return };
    let left = fmt_age(last - first);
    let right = "now";
    buf.set_string(x, y, &left, style);
    if width as usize > left.len() + right.len() + 1 {
        buf.set_string(x + width - right.len() as u16, y, right, style);
    }
    let mid = fmt_age((last - first) / 2);
    if width as usize > left.len() + mid.len() + right.len() + 4 {
        buf.set_string(x + (width - mid.len() as u16) / 2, y, &mid, style);
    }
}

/// "-30m", "-90s", "now" for an age in millis.
pub fn fmt_age(ms: i64) -> String {
    let s = ms / 1000;
    match s {
        0 => "now".into(),
        s if s % 3600 == 0 || s >= 2 * 3600 => format!("-{}h", (s as f64 / 3600.0).round()),
        s if s >= 120 => format!("-{}m", (s as f64 / 60.0).round()),
        s => format!("-{s}s"),
    }
}

fn big_glyph(c: char) -> [&'static str; 5] {
    match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => ["██ ", " █ ", " █ ", " █ ", "███"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", "███", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        '-' => ["   ", "   ", "███", "   ", "   "],
        _ => [" ", " ", " ", " ", "█"], // '.'
    }
}

/// Writes `text` centred in `width` cells starting at `x`, or nothing if it does not fit.
fn put_centered(buf: &mut Buffer, x: u16, width: u16, y: u16, text: &str, style: Style) {
    let len = text.chars().count() as u16;
    if len <= width {
        buf.set_string(x + (width - len) / 2, y, text, style);
    }
}

pub fn compact(v: f64) -> String {
    match v.abs() {
        a if a >= 1e6 => format!("{:.1}M", v / 1e6),
        a if a >= 1e4 => format!("{:.0}k", v / 1e3),
        a if a >= 1e3 => format!("{:.1}k", v / 1e3),
        _ => format!("{v:.0}"),
    }
}

/// Formats a value in the panel unit. Only seconds get special handling for now.
pub fn fmt_value(v: f64, unit: Option<&str>) -> String {
    match unit {
        Some("s") => {
            if v >= 1.0 {
                format!("{}s", trim(v, 2))
            } else if v >= 1e-3 {
                let ms = v * 1e3;
                format!("{}ms", trim(ms, if ms >= 10.0 { 0 } else { 1 }))
            } else {
                format!("{}µs", trim(v * 1e6, 0))
            }
        }
        Some("reqps") => format!("{} req/s", trim(v, if v >= 10.0 { 0 } else { 1 })),
        _ => trim(v, 3),
    }
}

fn fmt_bound(upper: f64, unit: Option<&str>) -> String {
    if upper.is_infinite() { "+Inf".into() } else { fmt_value(upper, unit) }
}

fn trim(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::histogram::Bucket;

    #[test]
    fn formats_seconds() {
        assert_eq!(fmt_value(0.005, Some("s")), "5ms");
        assert_eq!(fmt_value(0.025, Some("s")), "25ms");
        assert_eq!(fmt_value(2.5, Some("s")), "2.5s");
        assert_eq!(fmt_value(1.0, Some("s")), "1s");
        assert_eq!(fmt_value(0.0004, Some("s")), "400µs");
        assert_eq!(fmt_value(0.2062, Some("s")), "206ms");
        assert_eq!(fmt_value(0.0025, Some("s")), "2.5ms");
        assert_eq!(fmt_bound(f64::INFINITY, Some("s")), "+Inf");
        assert_eq!(compact(12_345.0), "12k");
        assert_eq!(compact(1_250.0), "1.2k");
    }

    #[test]
    fn formats_ages_and_rates() {
        assert_eq!(fmt_age(0), "now");
        assert_eq!(fmt_age(30 * 60_000), "-30m");
        assert_eq!(fmt_age(90_000), "-90s");
        assert_eq!(fmt_age(3 * 3_600_000), "-3h");
        assert_eq!(fmt_value(61.4, Some("reqps")), "61 req/s");
    }

    #[test]
    fn heatmap_puts_slow_buckets_on_top() {
        let hm = Heatmap { bounds: vec![0.1, 1.0], times: vec![0, 60_000], counts: vec![vec![0.0, 4.0], vec![0.0, 4.0]] };
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        HeatmapView { hm: &hm, unit: Some("s") }.render(area, &mut buf);
        let cell = &buf[(10, 0)];
        assert_eq!(cell.symbol(), "▀");
        // Top half is the 1s bucket (busy), bottom row's lower half is the empty 100ms bucket.
        assert_ne!(cell.fg, color::EMPTY.to_color());
        assert_eq!(buf[(10, 1)].bg, color::EMPTY.to_color());
    }

    #[test]
    fn stat_shows_big_number() {
        let s = Series { name: "x".into(), labels: Default::default(), points: vec![(0, 1.0), (1, 2.0)] };
        let area = Rect::new(0, 0, 30, 12);
        let mut buf = Buffer::empty(area);
        StatView { series: &s, unit: Some("s"), color: color::SERIES[0] }.render(area, &mut buf);
        let text: String = (0..area.height)
            .map(|y| (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>() + "\n")
            .collect();
        assert!(text.contains("███"), "{text}");
        assert!(text.contains('s'));
    }

    #[test]
    fn tallest_bar_fills_plot() {
        let hist = Histogram {
            buckets: vec![Bucket { upper: 0.1, count: 10.0 }, Bucket { upper: 1.0, count: 5.0 }],
        };
        let area = Rect::new(0, 0, 20, 7); // 4 bar rows
        let mut buf = Buffer::empty(area);
        HistogramView { hist: &hist, unit: Some("s"), quantiles: &[] }.render(area, &mut buf);
        let col = |x: u16| (1..5).map(|y| buf[(x, y)].symbol().to_string()).collect::<Vec<_>>().join("");
        // n=2, gap=1: bars are 9 wide at x=0..9 and x=10..19.
        assert_eq!(col(0), "████");
        assert_eq!(col(10), "  ██");
        // "100ms" centred under the first bar on the label row (y=6).
        let label: String = (2..7).map(|x| buf[(x, 6)].symbol().to_string()).collect();
        assert_eq!(label, "100ms");
    }
}
