//! 24-bit color with a 256-color fallback for terminals without true color.

use ratatui::style::Color;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Latency gradient, cool to hot: teal, green, amber, red.
const LATENCY: [Rgb; 4] = [Rgb(38, 166, 154), Rgb(124, 179, 66), Rgb(255, 179, 0), Rgb(229, 57, 53)];

pub fn truecolor() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("COLORTERM")
            .map(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"))
            .unwrap_or(false)
    })
}

impl Rgb {
    pub fn lerp(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb(mix(self.0, other.0), mix(self.1, other.1), mix(self.2, other.2))
    }

    pub fn scale(self, k: f64) -> Rgb {
        let f = |c: u8| (c as f64 * k).round().clamp(0.0, 255.0) as u8;
        Rgb(f(self.0), f(self.1), f(self.2))
    }

    pub fn to_color(self) -> Color {
        if truecolor() { Color::Rgb(self.0, self.1, self.2) } else { Color::Indexed(to_ansi256(self)) }
    }
}

/// Heatmap gradient (plasma), dark to bright.
const HEAT: [Rgb; 5] = [Rgb(13, 8, 135), Rgb(126, 3, 168), Rgb(204, 71, 120), Rgb(248, 149, 64), Rgb(240, 249, 33)];

/// Series colors, Grafana's classic palette order.
pub const SERIES: [Rgb; 6] =
    [Rgb(115, 191, 105), Rgb(250, 222, 42), Rgb(87, 148, 242), Rgb(255, 152, 48), Rgb(242, 73, 92), Rgb(184, 119, 217)];

pub const DIM: Rgb = Rgb(120, 128, 126);
pub const EMPTY: Rgb = Rgb(24, 28, 27);

/// Samples the latency gradient at `t` in 0..=1.
pub fn latency(t: f64) -> Rgb {
    sample(&LATENCY, t)
}

pub fn heat(t: f64) -> Rgb {
    sample(&HEAT, t)
}

fn sample(stops: &[Rgb], t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let i = (t.floor() as usize).min(stops.len() - 2);
    stops[i].lerp(stops[i + 1], t - i as f64)
}

/// Nearest entry in the xterm 6x6x6 cube or grayscale ramp.
pub fn to_ansi256(c: Rgb) -> u8 {
    let level = |v: u8| if v < 48 { 0 } else if v < 115 { 1 } else { (v as u16 - 35) as u8 / 40 };
    let steps = [0u8, 95, 135, 175, 215, 255];
    let (r, g, b) = (level(c.0), level(c.1), level(c.2));
    let cube = Rgb(steps[r as usize], steps[g as usize], steps[b as usize]);
    let avg = (c.0 as u16 + c.1 as u16 + c.2 as u16) / 3;
    let gray_idx = if avg > 238 { 23 } else { (avg.saturating_sub(3) / 10) as u8 };
    let gray_v = 8 + 10 * gray_idx;
    let dist = |a: Rgb| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, c.0) + d(a.1, c.1) + d(a.2, c.2)
    };
    if dist(Rgb(gray_v, gray_v, gray_v)) < dist(cube) { 232 + gray_idx } else { 16 + 36 * r + 6 * g + b }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_ends() {
        assert_eq!(latency(0.0), LATENCY[0]);
        assert_eq!(latency(1.0), LATENCY[3]);
        assert_eq!(latency(-1.0), LATENCY[0]);
    }

    #[test]
    fn ansi256_mapping() {
        assert_eq!(to_ansi256(Rgb(255, 0, 0)), 196);
        assert_eq!(to_ansi256(Rgb(0, 0, 0)), 16);
        assert_eq!(to_ansi256(Rgb(128, 128, 128)), 244);
    }
}
