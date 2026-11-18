export interface WasmLatencyPercentiles {
  p50: number;
  p95: number;
  p99: number;
  max: number;
  mean: number;
  jitter: number;
  sample_count: number;
  engine: "wasm" | "ts-bridge";
}

export interface HistogramBucket {
  label: string;
  count: number;
  percentage: number;
}

export interface WasmLatencyHistogram {
  buckets: HistogramBucket[];
  total_samples: number;
  engine: "wasm" | "ts-bridge";
}

interface CompiledWasmModule {
  default?: () => Promise<unknown>;
  compute_latency_percentiles: (samples: Float64Array) => {
    p50: number;
    p95: number;
    p99: number;
    max: number;
    mean: number;
    jitter: number;
    sample_count: number;
  };
  compute_latency_histogram?: (samples: Float64Array) => {
    buckets: HistogramBucket[];
    total_samples: number;
  };
  detect_zscore_anomalies: (series: Float64Array, threshold: number) => Uint32Array;
}

let wasmMod: CompiledWasmModule | null = null;
let wasmInitAttempted = false;

/**
 * Attempts to load a compiled wasm-pack bundle from `/wasm/vulcan_wasm_analytics.js`.
 * Falls back cleanly to an exact IEEE-754 mirror of `wasm-analytics/src/lib.rs`
 * when `wasm-pack` output is not bundled in `public/wasm/`.
 */
export async function initWasmAnalytics(): Promise<"wasm" | "ts-bridge"> {
  if (wasmMod) return "wasm";
  if (wasmInitAttempted) return "ts-bridge";
  wasmInitAttempted = true;

  try {
    const wasmJsUrl = "/wasm/vulcan_wasm_analytics.js";
    const probe = await fetch(wasmJsUrl, { method: "HEAD" });
    if (!probe.ok) {
      return "ts-bridge";
    }
    const mod = (await import(/* @vite-ignore */ wasmJsUrl)) as CompiledWasmModule;
    if (typeof mod.default === "function") {
      await mod.default();
    }
    if (
      typeof mod.compute_latency_percentiles === "function" &&
      typeof mod.detect_zscore_anomalies === "function"
    ) {
      wasmMod = mod;
      return "wasm";
    }
  } catch {
    // Fallback to exact TypeScript mirror of wasm-analytics/src/lib.rs
  }
  return "ts-bridge";
}

function percentileSorted(sorted: number[], q: number): number {
  if (sorted.length === 0) return 0;
  if (sorted.length === 1) return sorted[0];
  const pos = Math.max(0, Math.min(1, q)) * (sorted.length - 1);
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  if (lo === hi) return sorted[lo];
  const weight = pos - lo;
  return sorted[lo] * (1 - weight) + sorted[hi] * weight;
}

function round2(n: number): number {
  return Math.round(n * 100) / 100;
}

export function computeLatencyPercentiles(samples: number[]): WasmLatencyPercentiles {
  if (wasmMod && samples.length > 0) {
    try {
      const f64 = new Float64Array(samples);
      const out = wasmMod.compute_latency_percentiles(f64);
      if (out && typeof out.p50 === "number") {
        return {
          p50: round2(out.p50),
          p95: round2(out.p95),
          p99: round2(out.p99),
          max: round2(out.max),
          mean: round2(out.mean),
          jitter: round2(out.jitter),
          sample_count: out.sample_count,
          engine: "wasm",
        };
      }
    } catch {
      // Fallback on runtime invocation failure
    }
  }

  const valid = samples.filter((v) => Number.isFinite(v) && v >= 0);
  if (valid.length === 0) {
    return {
      p50: 0,
      p95: 0,
      p99: 0,
      max: 0,
      mean: 0,
      jitter: 0,
      sample_count: 0,
      engine: "ts-bridge",
    };
  }

  const n = valid.length;
  const mean = valid.reduce((acc, v) => acc + v, 0) / n;

  let diffSum = 0;
  for (let i = 1; i < n; i++) {
    diffSum += Math.abs(valid[i] - valid[i - 1]);
  }
  const jitter = n > 1 ? diffSum / (n - 1) : 0;

  const sorted = [...valid].sort((a, b) => a - b);

  return {
    p50: round2(percentileSorted(sorted, 0.5)),
    p95: round2(percentileSorted(sorted, 0.95)),
    p99: round2(percentileSorted(sorted, 0.99)),
    max: round2(sorted[sorted.length - 1]),
    mean: round2(mean),
    jitter: round2(jitter),
    sample_count: n,
    engine: "ts-bridge",
  };
}

export function computeLatencyHistogram(samples: number[]): WasmLatencyHistogram {
  if (wasmMod && typeof wasmMod.compute_latency_histogram === "function" && samples.length > 0) {
    try {
      const f64 = new Float64Array(samples);
      const out = wasmMod.compute_latency_histogram(f64);
      if (out && Array.isArray(out.buckets)) {
        return {
          buckets: out.buckets,
          total_samples: out.total_samples,
          engine: "wasm",
        };
      }
    } catch {
      // Fallback below
    }
  }

  const counts = [0, 0, 0, 0, 0];
  let total = 0;

  for (const val of samples) {
    if (!Number.isFinite(val) || val < 0) continue;
    total += 1;
    if (val < 25) {
      counts[0] += 1;
    } else if (val < 50) {
      counts[1] += 1;
    } else if (val < 100) {
      counts[2] += 1;
    } else if (val <= 250) {
      counts[3] += 1;
    } else {
      counts[4] += 1;
    }
  }

  const labels = ["<25µs", "25-50µs", "50-100µs", "100-250µs", ">250µs"];
  const buckets = labels.map((label, i) => ({
    label,
    count: counts[i],
    percentage: total > 0 ? round2((counts[i] / total) * 100) : 0,
  }));

  return {
    buckets,
    total_samples: total,
    engine: "ts-bridge",
  };
}

export function detectZScoreAnomalies(series: number[], threshold = 3.0): number[] {
  if (series.length < 3) return [];

  if (wasmMod) {
    try {
      const f64 = new Float64Array(series);
      const indices = wasmMod.detect_zscore_anomalies(f64, threshold);
      return Array.from(indices);
    } catch {
      // Fallback below
    }
  }

  const n = series.length;
  const mean = series.reduce((acc, v) => acc + v, 0) / n;
  const variance = series.reduce((acc, v) => acc + (v - mean) ** 2, 0) / n;
  const stdDev = Math.sqrt(variance);
  if (stdDev <= 1e-9) return [];

  const effThreshold = threshold > 0 ? threshold : 3.0;
  const anomalies: number[] = [];
  for (let i = 0; i < n; i++) {
    const val = series[i];
    if (!Number.isFinite(val)) continue;
    if (Math.abs(val - mean) / stdDev > effThreshold) {
      anomalies.push(i);
    }
  }
  return anomalies;
}
