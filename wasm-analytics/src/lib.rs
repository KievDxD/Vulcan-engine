use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyPercentiles {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
    pub jitter: f64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HistogramBucket {
    pub label: String,
    pub count: usize,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LatencyHistogram {
    pub buckets: Vec<HistogramBucket>,
    pub total_samples: usize,
}

fn percentile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lower = pos.floor() as usize;
    let upper = pos.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let weight = pos - lower as f64;
        sorted[lower] * (1.0 - weight) + sorted[upper] * weight
    }
}

/// Pure-Rust implementation of latency percentiles and RFC-3550-style inter-sample jitter.
pub fn compute_percentiles_native(samples: &[f64]) -> LatencyPercentiles {
    let valid: Vec<f64> = samples
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();

    if valid.is_empty() {
        return LatencyPercentiles {
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
            max: 0.0,
            mean: 0.0,
            jitter: 0.0,
            sample_count: 0,
        };
    }

    let n = valid.len();
    let sum: f64 = valid.iter().sum();
    let mean = sum / n as f64;

    let jitter = if n > 1 {
        let diff_sum: f64 = valid.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
        diff_sum / (n - 1) as f64
    } else {
        0.0
    };

    let mut sorted = valid;
    sorted.sort_by(|a, b| a.total_cmp(b));

    LatencyPercentiles {
        p50: percentile_sorted(&sorted, 0.50),
        p95: percentile_sorted(&sorted, 0.95),
        p99: percentile_sorted(&sorted, 0.99),
        max: *sorted.last().unwrap_or(&0.0),
        mean,
        jitter,
        sample_count: n,
    }
}

/// Pure-Rust Z-Score anomaly detector returning indices where |x_i - mean| / std > threshold.
pub fn detect_zscore_anomalies_native(series: &[f64], threshold: f64) -> Vec<usize> {
    if series.len() < 3 {
        return Vec::new();
    }

    let finite_values: Vec<f64> = series.iter().copied().filter(|v| v.is_finite()).collect();
    if finite_values.len() < 3 {
        return Vec::new();
    }

    let n = series.len() as f64;
    let mean = series.iter().sum::<f64>() / n;
    let variance = series.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();

    if std_dev <= f64::EPSILON {
        return Vec::new();
    }

    let effective_threshold = if threshold > 0.0 { threshold } else { 3.0 };

    series
        .iter()
        .enumerate()
        .filter_map(|(idx, val)| {
            if !val.is_finite() {
                return None;
            }
            let z = (*val - mean).abs() / std_dev;
            if z > effective_threshold {
                Some(idx)
            } else {
                None
            }
        })
        .collect()
}

/// Classifies finite latency samples in microseconds across 5 sub-millisecond buckets.
pub fn compute_histogram_native(samples: &[f64]) -> LatencyHistogram {
    let mut counts = [0usize; 5];
    let mut total = 0usize;

    for &val in samples {
        if !val.is_finite() || val < 0.0 {
            continue;
        }
        total += 1;
        if val < 25.0 {
            counts[0] += 1;
        } else if val < 50.0 {
            counts[1] += 1;
        } else if val < 100.0 {
            counts[2] += 1;
        } else if val <= 250.0 {
            counts[3] += 1;
        } else {
            counts[4] += 1;
        }
    }

    let labels = ["<25µs", "25-50µs", "50-100µs", "100-250µs", ">250µs"];
    let buckets = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let pct = if total > 0 {
                ((counts[i] as f64 / total as f64) * 10000.0).round() / 100.0
            } else {
                0.0
            };
            HistogramBucket {
                label: (*label).to_string(),
                count: counts[i],
                percentage: pct,
            }
        })
        .collect();

    LatencyHistogram {
        buckets,
        total_samples: total,
    }
}

#[wasm_bindgen]
pub fn compute_latency_percentiles(samples: &[f64]) -> JsValue {
    let stats = compute_percentiles_native(samples);
    serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn compute_latency_histogram(samples: &[f64]) -> JsValue {
    let hist = compute_histogram_native(samples);
    serde_wasm_bindgen::to_value(&hist).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn detect_zscore_anomalies(series: &[f64], threshold: f64) -> Vec<usize> {
    detect_zscore_anomalies_native(series, threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_percentiles_and_jitter_accurately() {
        let samples: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        let res = compute_percentiles_native(&samples);
        assert_eq!(res.sample_count, 100);
        assert!((res.p50 - 50.5).abs() < 0.05);
        assert!((res.p95 - 95.05).abs() < 0.05);
        assert!((res.p99 - 99.01).abs() < 0.05);
        assert!((res.max - 100.0).abs() < f64::EPSILON);
        assert!((res.mean - 50.5).abs() < 0.01);
        assert!((res.jitter - 1.0).abs() < 0.01);
    }

    #[test]
    fn handles_empty_and_single_samples() {
        let empty = compute_percentiles_native(&[]);
        assert_eq!(empty.sample_count, 0);
        assert_eq!(empty.p99, 0.0);

        let single = compute_percentiles_native(&[14.2]);
        assert_eq!(single.sample_count, 1);
        assert_eq!(single.p50, 14.2);
        assert_eq!(single.p99, 14.2);
        assert_eq!(single.jitter, 0.0);
    }

    #[test]
    fn detects_zscore_anomalies_above_three_sigma() {
        // 19 normal samples around ~20 and 1 extreme spike at index 15
        let mut series = vec![20.0, 21.0, 19.0, 20.5, 19.5, 20.0, 21.0, 19.0, 20.0, 20.2, 19.8, 20.1, 19.9, 20.0, 20.4, 210.0, 20.0, 19.7, 20.3, 20.0];
        let anomalies = detect_zscore_anomalies_native(&series, 3.0);
        assert_eq!(anomalies, vec![15]);

        series[3] = 240.0;
        let multi = detect_zscore_anomalies_native(&series, 2.5);
        assert!(multi.contains(&3) && multi.contains(&15));
    }

    #[test]
    fn ignores_flat_series_without_variance() {
        let flat = vec![42.0; 25];
        assert!(detect_zscore_anomalies_native(&flat, 3.0).is_empty());
    }

    #[test]
    fn computes_five_bucket_microsecond_histogram_accurately() {
        let samples = vec![4.2, 12.0, 24.9, 25.0, 49.5, 72.1, 120.0, 250.0, 310.5, 500.0];
        let hist = compute_histogram_native(&samples);
        assert_eq!(hist.total_samples, 10);
        assert_eq!(hist.buckets.len(), 5);
        assert_eq!(hist.buckets[0].label, "<25µs");
        assert_eq!(hist.buckets[0].count, 3);
        assert!((hist.buckets[0].percentage - 30.0).abs() < 0.01);
        assert_eq!(hist.buckets[1].count, 2);
        assert_eq!(hist.buckets[2].count, 1);
        assert_eq!(hist.buckets[3].count, 2);
        assert_eq!(hist.buckets[4].count, 2);
        assert_eq!(hist.buckets[4].label, ">250µs");
    }
}
