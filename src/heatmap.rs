//! Prometheus histogram over time: cumulative `le` series to a bucket x time count grid.

use crate::grafana::Series;
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Heatmap {
    /// Bucket upper bounds, ascending; `+Inf` last.
    pub bounds: Vec<f64>,
    /// Unix millis, ascending.
    pub times: Vec<i64>,
    /// `counts[t][b]`: requests in bucket `b` at time `times[t]`.
    pub counts: Vec<Vec<f64>>,
}

impl Heatmap {
    pub fn from_series(series: &[Series]) -> Option<Self> {
        let mut cols: Vec<(f64, &Series)> = series
            .iter()
            .filter_map(|s| Some((crate::histogram::parse_le(s.labels.get("le")?)?, s)))
            .collect();
        if cols.is_empty() {
            return None;
        }
        cols.sort_by(|a, b| a.0.total_cmp(&b.0));
        let times: Vec<i64> = cols.iter().flat_map(|(_, s)| s.points.iter().map(|p| p.0)).collect::<BTreeSet<_>>().into_iter().collect();
        let counts = times
            .iter()
            .map(|t| {
                let mut prev = 0.0_f64;
                cols.iter()
                    .map(|(_, s)| {
                        // A missing sample means no new requests reached this bucket.
                        let cum = s.points.binary_search_by_key(t, |p| p.0).map(|i| s.points[i].1).unwrap_or(prev);
                        let count = (cum - prev).max(0.0);
                        prev = prev.max(cum);
                        count
                    })
                    .collect()
            })
            .collect();
        Some(Self { bounds: cols.iter().map(|c| c.0).collect(), times, counts })
    }

    pub fn max(&self) -> f64 {
        self.counts.iter().flatten().copied().fold(0.0, f64::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn s(le: &str, pts: &[(i64, f64)]) -> Series {
        Series { name: le.into(), labels: BTreeMap::from([("le".into(), le.into())]), points: pts.to_vec() }
    }

    #[test]
    fn grid_per_time() {
        let hm = Heatmap::from_series(&[
            s("+Inf", &[(1, 10.0), (2, 20.0)]),
            s("0.1", &[(1, 4.0), (2, 5.0)]),
            s("1", &[(1, 9.0), (2, 15.0)]),
        ])
        .unwrap();
        assert_eq!(hm.bounds, [0.1, 1.0, f64::INFINITY]);
        assert_eq!(hm.times, [1, 2]);
        assert_eq!(hm.counts, [vec![4.0, 5.0, 1.0], vec![5.0, 10.0, 5.0]]);
        assert_eq!(hm.max(), 10.0);
    }

    #[test]
    fn missing_sample_counts_zero() {
        let hm = Heatmap::from_series(&[s("0.1", &[(1, 4.0)]), s("+Inf", &[(1, 6.0), (2, 3.0)])]).unwrap();
        assert_eq!(hm.counts[1], [0.0, 3.0]);
    }
}
