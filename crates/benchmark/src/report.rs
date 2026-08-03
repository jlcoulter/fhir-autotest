use chrono::{DateTime, Utc};
use hdrhistogram::Histogram;
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// A single benchmark sample: one request's timing and outcome.
#[derive(Debug, Clone, Serialize)]
pub struct BenchSample {
    pub test_group: String,
    pub test_name: String,
    pub request_method: String,
    pub request_url: String,
    pub status_code: u16,
    pub latency_us: u64,
    pub passed: bool,
    pub timestamp: DateTime<Utc>,
}

/// Per-group statistics.
#[derive(Debug, Clone, Serialize)]
pub struct GroupStats {
    pub group: String,
    pub total: u64,
    pub passed: u64,
    pub failed: u64,
    pub latency_min_us: u64,
    pub latency_max_us: u64,
    pub latency_mean_us: f64,
    pub latency_p50_us: u64,
    pub latency_p90_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,
    pub throughput_req_per_sec: f64,
}

/// Overall benchmark report.
#[derive(Debug, Clone, Serialize)]
pub struct BenchReport {
    pub title: String,
    pub timestamp: DateTime<Utc>,
    pub duration_secs: f64,
    pub concurrency: usize,
    pub total_requests: u64,
    pub passed: u64,
    pub failed: u64,
    pub latency_min_us: u64,
    pub latency_max_us: u64,
    pub latency_mean_us: f64,
    pub latency_p50_us: u64,
    pub latency_p90_us: u64,
    pub latency_p95_us: u64,
    pub latency_p99_us: u64,
    pub throughput_req_per_sec: f64,
    pub groups: Vec<GroupStats>,
    /// All raw samples (omitted from summary JSON, included in full results).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<BenchSample>,
}

impl BenchReport {
    /// Build a report from collected samples and benchmark parameters.
    pub fn from_samples(
        samples: Vec<BenchSample>,
        duration: Duration,
        concurrency: usize,
    ) -> Self {
        let total = samples.len() as u64;
        let passed = samples.iter().filter(|s| s.passed).count() as u64;
        let failed = total - passed;
        let duration_secs = duration.as_secs_f64();

        // Build overall histogram
        let mut hist = Histogram::<u64>::new_with_bounds(1, 10_000_000, 3).unwrap();
        for s in &samples {
            hist.record(s.latency_us).ok();
        }

        let throughput = if duration_secs > 0.0 {
            total as f64 / duration_secs
        } else {
            0.0
        };

        // Group samples by test_group
        let mut grouped: HashMap<String, Vec<&BenchSample>> = HashMap::new();
        for s in &samples {
            grouped.entry(s.test_group.clone()).or_default().push(s);
        }

        let mut groups: Vec<GroupStats> = grouped
            .into_iter()
            .map(|(group, group_samples)| {
                let g_total = group_samples.len() as u64;
                let g_passed = group_samples.iter().filter(|s| s.passed).count() as u64;
                let g_failed = g_total - g_passed;

                let mut g_hist =
                    Histogram::<u64>::new_with_bounds(1, 10_000_000, 3).unwrap();
                for s in &group_samples {
                    g_hist.record(s.latency_us).ok();
                }

                let g_throughput = if duration_secs > 0.0 {
                    g_total as f64 / duration_secs
                } else {
                    0.0
                };

                GroupStats {
                    group,
                    total: g_total,
                    passed: g_passed,
                    failed: g_failed,
                    latency_min_us: g_hist.min(),
                    latency_max_us: g_hist.max(),
                    latency_mean_us: g_hist.mean(),
                    latency_p50_us: g_hist.value_at_percentile(50.0),
                    latency_p90_us: g_hist.value_at_percentile(90.0),
                    latency_p95_us: g_hist.value_at_percentile(95.0),
                    latency_p99_us: g_hist.value_at_percentile(99.0),
                    throughput_req_per_sec: g_throughput,
                }
            })
            .collect();
        groups.sort_by(|a, b| a.group.cmp(&b.group));

        BenchReport {
            title: "FHIR Autotest Benchmark Report".to_string(),
            timestamp: Utc::now(),
            duration_secs,
            concurrency,
            total_requests: total,
            passed,
            failed,
            latency_min_us: hist.min(),
            latency_max_us: hist.max(),
            latency_mean_us: hist.mean(),
            latency_p50_us: hist.value_at_percentile(50.0),
            latency_p90_us: hist.value_at_percentile(90.0),
            latency_p95_us: hist.value_at_percentile(95.0),
            latency_p99_us: hist.value_at_percentile(99.0),
            throughput_req_per_sec: throughput,
            groups,
            samples,
        }
    }

    /// Write the report to the output directory.
    pub fn write(&self, output_dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(output_dir)?;

        // Summary JSON (no raw samples)
        let summary = BenchReport {
            samples: Vec::new(),
            ..self.clone()
        };
        let summary_json = serde_json::to_string_pretty(&summary)?;
        std::fs::write(output_dir.join("summary.json"), &summary_json)?;

        // Full results JSON (with all samples)
        let full_json = serde_json::to_string_pretty(self)?;
        std::fs::write(output_dir.join("full_results.json"), &full_json)?;

        // Human-readable text report
        let text = self.format_text();
        std::fs::write(output_dir.join("report.txt"), &text)?;

        // Simple HTML report
        let html = self.format_html();
        std::fs::write(output_dir.join("report.html"), &html)?;

        tracing::info!("Benchmark report written to {}", output_dir.display());
        Ok(())
    }

    fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\n", self.title));
        out.push_str(&format!("  Timestamp: {}\n", self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
        out.push_str(&format!("  Duration: {:.1}s\n", self.duration_secs));
        out.push_str(&format!("  Concurrency: {}\n", self.concurrency));
        out.push_str("\n");
        out.push_str("── Overall ──\n");
        out.push_str(&format!("  Total requests: {}\n", self.total_requests));
        out.push_str(&format!("  Passed:         {}\n", self.passed));
        out.push_str(&format!("  Failed:         {}\n", self.failed));
        out.push_str(&format!("  Throughput:     {:.1} req/s\n", self.throughput_req_per_sec));
        out.push_str("\n");
        out.push_str("── Latency (μs) ──\n");
        out.push_str(&format!("  Min:    {}\n", self.latency_min_us));
        out.push_str(&format!("  Mean:   {:.0}\n", self.latency_mean_us));
        out.push_str(&format!("  P50:    {}\n", self.latency_p50_us));
        out.push_str(&format!("  P90:    {}\n", self.latency_p90_us));
        out.push_str(&format!("  P95:    {}\n", self.latency_p95_us));
        out.push_str(&format!("  P99:    {}\n", self.latency_p99_us));
        out.push_str(&format!("  Max:    {}\n", self.latency_max_us));
        out.push_str("\n");
        out.push_str("── Per Group ──\n");
        for g in &self.groups {
            out.push_str(&format!(
                "  {:<20} total={:<6} passed={:<6} failed={:<4}  p50={:<8} p95={:<8} p99={:<8}  {:.1} req/s\n",
                g.group,
                g.total,
                g.passed,
                g.failed,
                format_duration_us(g.latency_p50_us),
                format_duration_us(g.latency_p95_us),
                format_duration_us(g.latency_p99_us),
                g.throughput_req_per_sec,
            ));
        }
        out
    }

    fn format_html(&self) -> String {
        let mut rows = String::new();
        for g in &self.groups {
            rows.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td>\
                 <td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{:.1}</td></tr>\n",
                html_escape(&g.group),
                g.total,
                g.passed,
                g.failed,
                format_duration_us(g.latency_p50_us),
                format_duration_us(g.latency_p90_us),
                format_duration_us(g.latency_p95_us),
                format_duration_us(g.latency_p99_us),
                g.throughput_req_per_sec,
            ));
        }

        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><title>Benchmark Report</title>
<style>
body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 2em; }}
h1 {{ color: #333; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ border: 1px solid #ddd; padding: 8px; text-align: right; }}
th {{ background-color: #f5f5f5; font-weight: 600; }}
td:first-child {{ text-align: left; }}
.pass {{ color: #2e7d32; }}
.fail {{ color: #c62828; }}
.summary {{ display: flex; gap: 2em; margin: 1em 0; }}
.stat {{ background: #f9f9f9; padding: 1em; border-radius: 8px; flex: 1; }}
.stat h3 {{ margin: 0 0 0.5em; color: #555; }}
.stat .value {{ font-size: 1.5em; font-weight: bold; }}
</style>
</head>
<body>
<h1>{}: FHIR Autotest Benchmark</h1>
<p>{} | Duration: {:.1}s | Concurrency: {}</p>
<div class="summary">
  <div class="stat"><h3>Total Requests</h3><div class="value">{}</div></div>
  <div class="stat"><h3>Passed</h3><div class="value pass">{}</div></div>
  <div class="stat"><h3>Failed</h3><div class="value fail">{}</div></div>
  <div class="stat"><h3>Throughput</h3><div class="value">{:.1} req/s</div></div>
</div>
<h2>Latency</h2>
<table>
<tr><th>Min</th><th>Mean</th><th>P50</th><th>P90</th><th>P95</th><th>P99</th><th>Max</th></tr>
<tr>
  <td>{}</td><td>{:.0}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td>
</tr>
</table>
<h2>Per Group</h2>
<table>
<tr><th>Group</th><th>Total</th><th>Passed</th><th>Failed</th><th>P50</th><th>P90</th><th>P95</th><th>P99</th><th>Throughput</th></tr>
{}
</table>
</body>
</html>"#,
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.title,
            self.duration_secs,
            self.concurrency,
            self.total_requests,
            self.passed,
            self.failed,
            self.throughput_req_per_sec,
            format_duration_us(self.latency_min_us),
            self.latency_mean_us,
            format_duration_us(self.latency_p50_us),
            format_duration_us(self.latency_p90_us),
            format_duration_us(self.latency_p95_us),
            format_duration_us(self.latency_p99_us),
            format_duration_us(self.latency_max_us),
            rows,
        )
    }
}

fn format_duration_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2}s", us as f64 / 1_000_000.0)
    } else if us >= 1_000 {
        format!("{:.1}ms", us as f64 / 1_000.0)
    } else {
        format!("{}μs", us)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(us: u64, passed: bool, group: &str) -> BenchSample {
        BenchSample {
            test_group: group.to_string(),
            test_name: "test-read".to_string(),
            request_method: "GET".to_string(),
            request_url: format!("http://fhir/Patient/{}", us),
            status_code: if passed { 200 } else { 500 },
            latency_us: us,
            passed,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn report_from_empty_samples() {
        let report = BenchReport::from_samples(vec![], Duration::from_secs(10), 5);
        assert_eq!(report.total_requests, 0);
        assert_eq!(report.passed, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.concurrency, 5);
        assert_eq!(report.duration_secs, 10.0);
        assert!(report.groups.is_empty());
    }

    #[test]
    fn report_all_passed() {
        let samples = vec![
            sample(100, true, "Patient"),
            sample(200, true, "Patient"),
            sample(150, true, "Observation"),
        ];
        let report = BenchReport::from_samples(samples, Duration::from_secs(10), 1);
        assert_eq!(report.total_requests, 3);
        assert_eq!(report.passed, 3);
        assert_eq!(report.failed, 0);
        assert_eq!(report.groups.len(), 2);
    }

    #[test]
    fn report_some_failed() {
        let samples = vec![
            sample(100, true, "Patient"),
            sample(200, false, "Patient"),
            sample(150, true, "Observation"),
        ];
        let report = BenchReport::from_samples(samples, Duration::from_secs(10), 1);
        assert_eq!(report.total_requests, 3);
        assert_eq!(report.passed, 2);
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn report_latency_percentiles() {
        let samples: Vec<_> = (1..=100)
            .map(|i| sample(i * 10, true, "Patient"))
            .collect();
        let report = BenchReport::from_samples(samples, Duration::from_secs(10), 1);
        // P50 should be around 500, P90 around 900, P99 around 990
        assert!(report.latency_p50_us >= 400 && report.latency_p50_us <= 600);
        assert!(report.latency_p90_us >= 800 && report.latency_p90_us <= 1000);
        assert!(report.latency_p99_us >= 900 && report.latency_p99_us <= 1000);
        assert_eq!(report.latency_min_us, 10);
        assert_eq!(report.latency_max_us, 1000);
    }

    #[test]
    fn report_per_group_stats() {
        let samples = vec![
            sample(100, true, "Patient"),
            sample(200, true, "Patient"),
            sample(300, false, "Observation"),
        ];
        let report = BenchReport::from_samples(samples, Duration::from_secs(10), 1);
        let patient = report.groups.iter().find(|g| g.group == "Patient").unwrap();
        assert_eq!(patient.total, 2);
        assert_eq!(patient.passed, 2);
        assert_eq!(patient.failed, 0);

        let obs = report.groups.iter().find(|g| g.group == "Observation").unwrap();
        assert_eq!(obs.total, 1);
        assert_eq!(obs.passed, 0);
        assert_eq!(obs.failed, 1);
    }

    #[test]
    fn report_throughput_calculation() {
        let samples = vec![sample(100, true, "Patient")];
        let report = BenchReport::from_samples(samples, Duration::from_secs(2), 1);
        // 1 request / 2 seconds = 0.5 req/s
        assert!((report.throughput_req_per_sec - 0.5).abs() < 0.01);
    }

    #[test]
    fn report_write_creates_files() {
        let dir = tempfile::tempdir().unwrap();
        let samples = vec![sample(100, true, "Patient")];
        let report = BenchReport::from_samples(samples, Duration::from_secs(1), 1);
        report.write(dir.path()).unwrap();

        assert!(dir.path().join("summary.json").exists());
        assert!(dir.path().join("full_results.json").exists());
        assert!(dir.path().join("report.txt").exists());
        assert!(dir.path().join("report.html").exists());
    }

    #[test]
    fn report_summary_omits_samples() {
        let dir = tempfile::tempdir().unwrap();
        let samples = vec![sample(100, true, "Patient")];
        let report = BenchReport::from_samples(samples, Duration::from_secs(1), 1);
        report.write(dir.path()).unwrap();

        let summary: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join("summary.json")).unwrap())
                .unwrap();
        // Summary should not contain raw samples
        assert!(summary.get("samples").map_or(true, |v| v.as_array().map_or(true, |a| a.is_empty())));
    }

    #[test]
    fn format_duration_us_various_units() {
        assert_eq!(format_duration_us(500), "500μs");
        assert_eq!(format_duration_us(1500), "1.5ms");
        assert_eq!(format_duration_us(1_000_000), "1.00s");
        assert_eq!(format_duration_us(2_500_000), "2.50s");
    }

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(html_escape("Patient & Observation"), "Patient &amp; Observation");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("say \"hi\""), "say &quot;hi&quot;");
    }
}
