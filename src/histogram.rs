//! Prometheus classic histograms: cumulative `le` series to per-bucket counts and quantiles.

use crate::grafana::Series;

#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    /// Upper bound; `f64::INFINITY` for `+Inf`.
    pub upper: f64,
    pub count: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub buckets: Vec<Bucket>,
}

impl Histogram {
    /// Builds from series labelled `le`, using each series' latest value.
    /// Returns `None` when no series carries an `le` label.
    pub fn from_series(series: &[Series]) -> Option<Self> {
        let mut cumulative: Vec<(f64, f64)> = series
            .iter()
            .filter_map(|s| Some((parse_le(s.labels.get("le")?)?, s.last().unwrap_or(0.0))))
            .collect();
        if cumulative.is_empty() {
            return None;
        }
        cumulative.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut prev = 0.0;
        let buckets = cumulative
            .into_iter()
            .map(|(upper, cum)| {
                // Counter resets and rounding in increase() can make a bucket dip below its neighbour.
                let count = (cum - prev).max(0.0);
                prev = prev.max(cum);
                Bucket { upper, count }
            })
            .collect();
        Some(Self { buckets })
    }

    pub fn total(&self) -> f64 {
        self.buckets.iter().map(|b| b.count).sum()
    }

    /// Same interpolation as PromQL `histogram_quantile`: linear inside the bucket that
    /// holds the rank; a rank in the `+Inf` bucket returns the highest finite bound.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        let total = self.total();
        if total <= 0.0 {
            return None;
        }
        let rank = q * total;
        let mut cum = 0.0;
        let mut lower = 0.0;
        for b in &self.buckets {
            if cum + b.count >= rank && b.count > 0.0 {
                if b.upper.is_infinite() {
                    return Some(lower);
                }
                return Some(lower + (b.upper - lower) * (rank - cum) / b.count);
            }
            cum += b.count;
            if b.upper.is_finite() {
                lower = b.upper;
            }
        }
        Some(lower)
    }

    /// Where `value` falls on the bucket axis: integer part is the bucket index,
    /// fraction is the position inside it. Used to place quantile markers.
    pub fn position(&self, value: f64) -> f64 {
        let mut lower = 0.0;
        for (i, b) in self.buckets.iter().enumerate() {
            if value <= b.upper {
                if b.upper.is_infinite() {
                    return i as f64;
                }
                return i as f64 + (value - lower) / (b.upper - lower);
            }
            lower = b.upper;
        }
        self.buckets.len() as f64
    }
}

pub fn parse_le(s: &str) -> Option<f64> {
    match s {
        "+Inf" | "Inf" | "inf" => Some(f64::INFINITY),
        _ => s.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn s(le: &str, v: f64) -> Series {
        Series {
            name: le.into(),
            labels: BTreeMap::from([("le".into(), le.into())]),
            points: vec![(0, v)],
        }
    }

    fn hist() -> Histogram {
        // Deliberately unsorted input.
        Histogram::from_series(&[s("+Inf", 100.0), s("0.1", 20.0), s("1", 90.0), s("0.5", 60.0)]).unwrap()
    }

    #[test]
    fn decumulates_sorted() {
        let counts: Vec<f64> = hist().buckets.iter().map(|b| b.count).collect();
        assert_eq!(counts, [20.0, 40.0, 30.0, 10.0]);
        assert_eq!(hist().total(), 100.0);
    }

    #[test]
    fn quantiles_match_promql() {
        let h = hist();
        // rank 50 sits in (0.1, 0.5], 30 of 40 in: 0.1 + 0.4 * 0.75 = 0.4
        assert!((h.quantile(0.5).unwrap() - 0.4).abs() < 1e-9);
        // rank 99 sits in +Inf, so the highest finite bound
        assert_eq!(h.quantile(0.99), Some(1.0));
    }

    #[test]
    fn negative_dips_clamp_to_zero() {
        let h = Histogram::from_series(&[s("0.1", 10.0), s("0.5", 9.0), s("+Inf", 12.0)]).unwrap();
        let counts: Vec<f64> = h.buckets.iter().map(|b| b.count).collect();
        assert_eq!(counts, [10.0, 0.0, 2.0]);
    }

    #[test]
    fn empty_histogram_has_no_quantile() {
        let h = Histogram::from_series(&[s("0.1", 0.0), s("+Inf", 0.0)]).unwrap();
        assert_eq!(h.quantile(0.5), None);
    }

    #[test]
    fn position_inside_bucket() {
        let h = hist();
        assert!((h.position(0.3) - 1.5).abs() < 1e-9);
        assert_eq!(h.position(5.0), 3.0);
    }
}
