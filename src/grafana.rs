//! Grafana HTTP API client: dashboards and the unified `/api/ds/query` endpoint.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::Duration;

pub struct Client {
    base: String,
    token: String,
    http: reqwest::blocking::Client,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dashboard {
    pub uid: String,
    pub title: String,
    #[serde(default)]
    pub panels: Vec<Panel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Panel {
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// Kept as raw JSON: each target is sent back to `/api/ds/query` unchanged.
    #[serde(default)]
    pub targets: Vec<Value>,
    #[serde(default, rename = "fieldConfig")]
    pub field_config: Value,
}

impl Panel {
    pub fn unit(&self) -> Option<&str> {
        self.field_config.pointer("/defaults/unit")?.as_str()
    }
}

/// One numeric series from a data frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    /// (unix millis, value); null values are dropped.
    pub points: Vec<(i64, f64)>,
}

impl Series {
    pub fn last(&self) -> Option<f64> {
        self.points.last().map(|p| p.1)
    }
}

impl Client {
    pub fn new(base: &str, token: &str) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            http,
        })
    }

    pub fn dashboard(&self, uid: &str) -> Result<Dashboard> {
        let url = format!("{}/api/dashboards/uid/{uid}", self.base);
        let resp = self.http.get(&url).bearer_auth(&self.token).send()?;
        let body: Value = check(resp).with_context(|| format!("GET {url}"))?;
        serde_json::from_value(body["dashboard"].clone()).context("parse dashboard")
    }

    /// Runs every target of the panel over `from`..`to` (Grafana time strings such as `now-30m`).
    pub fn query_panel(&self, panel: &Panel, from: &str, to: &str, width: u32) -> Result<Vec<Series>> {
        let queries: Vec<Value> = panel
            .targets
            .iter()
            .filter(|t| !t["hide"].as_bool().unwrap_or(false))
            .map(|t| {
                let mut q = t.clone();
                q["intervalMs"] = json!(15_000);
                q["maxDataPoints"] = json!(width.max(50));
                q
            })
            .collect();
        if queries.is_empty() {
            bail!("panel {} has no queries", panel.id);
        }
        let url = format!("{}/api/ds/query", self.base);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.token)
            .json(&json!({ "from": from, "to": to, "queries": queries }))
            .send()?;
        let body = check(resp).with_context(|| format!("POST {url}"))?;
        parse_frames(&body)
    }
}

fn check(resp: reqwest::blocking::Response) -> Result<Value> {
    let status = resp.status();
    let body: Value = resp.json().unwrap_or(Value::Null);
    if !status.is_success() {
        let msg = body["message"].as_str().unwrap_or("no message");
        bail!("HTTP {status}: {msg}");
    }
    Ok(body)
}

/// Flattens a `/api/ds/query` response into one series per numeric field.
pub fn parse_frames(body: &Value) -> Result<Vec<Series>> {
    let results = body["results"].as_object().context("response has no results")?;
    let mut out = Vec::new();
    for (ref_id, result) in results {
        if let Some(err) = result["error"].as_str() {
            bail!("query {ref_id}: {err}");
        }
        for frame in result["frames"].as_array().into_iter().flatten() {
            let fields = frame.pointer("/schema/fields").and_then(Value::as_array);
            let values = frame.pointer("/data/values").and_then(Value::as_array);
            let (Some(fields), Some(values)) = (fields, values) else { continue };
            let Some(t_idx) = fields.iter().position(|f| f["type"] == "time") else { continue };
            let times: Vec<Option<i64>> = values[t_idx]
                .as_array()
                .map(|a| a.iter().map(Value::as_i64).collect())
                .unwrap_or_default();
            for (i, field) in fields.iter().enumerate() {
                if field["type"] != "number" {
                    continue;
                }
                let labels: BTreeMap<String, String> = field["labels"]
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                    .unwrap_or_default();
                let name = field
                    .pointer("/config/displayNameFromDS")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| field["name"].as_str().unwrap_or("Value").to_string());
                let points = values[i]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .zip(&times)
                    .filter_map(|(v, t)| Some(((*t)?, v.as_f64()?)))
                    .collect();
                out.push(Series { name, labels, points });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_dashboard_panels() {
        let body = fixture("dashboard.json");
        let dash: Dashboard = serde_json::from_value(body["dashboard"].clone()).unwrap();
        assert_eq!(dash.uid, "chartos-dev");
        let kinds: Vec<&str> = dash.panels.iter().map(|p| p.kind.as_str()).collect();
        assert_eq!(kinds, ["stat", "stat", "timeseries", "timeseries", "bargauge", "heatmap"]);
        assert_eq!(dash.panels[0].unit(), Some("s"));
    }

    #[test]
    fn parses_histogram_frames() {
        let series = parse_frames(&fixture("histogram_query.json")).unwrap();
        assert_eq!(series.len(), 12);
        assert!(series.iter().all(|s| s.labels.contains_key("le") && s.points.len() == 1));
        assert!(series.iter().any(|s| s.labels["le"] == "+Inf"));
    }

    #[test]
    fn query_error_is_reported() {
        let body = json!({"results": {"A": {"error": "bad expr", "frames": []}}});
        assert!(parse_frames(&body).unwrap_err().to_string().contains("bad expr"));
    }
}
