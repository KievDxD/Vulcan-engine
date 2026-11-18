import React, { useMemo, useState } from "react";
import { AlertTriangle, Cpu } from "lucide-react";
import {
  computeLatencyHistogram,
  computeLatencyPercentiles,
  detectZScoreAnomalies
} from "../lib/wasmAnalytics";
import {
  AnomalyAlert,
  DecisionEvent,
  JanitorStats,
  LatencySummary,
  LimiterCounters,
  ThroughputPoint
} from "../types/telemetry";

interface ThroughputTelemetryPanelProps {
  series: ThroughputPoint[];
  recentEvents: DecisionEvent[];
  backendLatency: LatencySummary;
  anomalies: AnomalyAlert[];
  counters: LimiterCounters;
  janitor: JanitorStats;
}

export const ThroughputTelemetryPanel: React.FC<ThroughputTelemetryPanelProps> = ({
  series,
  recentEvents,
  backendLatency,
  anomalies,
  counters,
  janitor
}) => {
  const [hoverIdx, setHoverIdx] = useState<number | null>(null);

  const latencySamples = useMemo(
    () => recentEvents.map((e) => e.eval_time_us).filter((v) => v > 0),
    [recentEvents]
  );

  const wasmStats = useMemo(() => {
    if (latencySamples.length === 0) {
      return {
        p50: backendLatency.p50_us,
        p95: backendLatency.p95_us,
        p99: backendLatency.p99_us,
        max: backendLatency.max_us,
        mean: backendLatency.mean_us,
        jitter: backendLatency.jitter_us,
        sample_count: backendLatency.sample_count,
        engine: "ts-bridge" as const
      };
    }
    return computeLatencyPercentiles(latencySamples);
  }, [latencySamples, backendLatency]);

  const histogram = useMemo(() => computeLatencyHistogram(latencySamples), [latencySamples]);

  const seriesSpikeIndices = useMemo(() => {
    const totals = series.map((p) => p.total_rps);
    return new Set(detectZScoreAnomalies(totals, 3.0));
  }, [series]);

  const width = 780;
  const height = 220;
  const padLeft = 46;
  const padRight = 14;
  const padTop = 14;
  const padBottom = 24;
  const plotW = width - padLeft - padRight;
  const plotH = height - padTop - padBottom;

  const displaySeries = useMemo(() => {
    if (series.length >= 2) return series;
    const now = Date.now();
    const base = series.length === 1 ? series[0] : null;
    return [
      {
        timestamp_ms: now - 1000,
        allowed_rps: 0,
        throttled_rps: 0,
        shadow_rps: 0,
        shed_rps: 0,
        total_rps: 0,
        mean_eval_us: 0,
        anomaly_detected: false
      },
      base ?? {
        timestamp_ms: now,
        allowed_rps: 0,
        throttled_rps: 0,
        shadow_rps: 0,
        shed_rps: 0,
        total_rps: 0,
        mean_eval_us: 0,
        anomaly_detected: false
      }
    ];
  }, [series]);

  const maxRps = useMemo(() => {
    const peak = Math.max(
      10,
      ...displaySeries.map((p) =>
        Math.max(p.allowed_rps, p.throttled_rps, p.shadow_rps, p.shed_rps, p.total_rps)
      )
    );
    return Math.ceil(peak * 1.15);
  }, [displaySeries]);

  const xFor = (idx: number) => {
    if (displaySeries.length <= 1) return padLeft;
    return padLeft + (idx / (displaySeries.length - 1)) * plotW;
  };

  const yFor = (val: number) => {
    const ratio = Math.max(0, Math.min(1, val / maxRps));
    return padTop + plotH - ratio * plotH;
  };

  const buildPolyline = (accessor: (p: ThroughputPoint) => number) =>
    displaySeries.map((p, i) => `${xFor(i).toFixed(1)},${yFor(accessor(p)).toFixed(1)}`).join(" ");

  const buildArea = (accessor: (p: ThroughputPoint) => number) => {
    const pts = displaySeries.map(
      (p, i) => `${xFor(i).toFixed(1)},${yFor(accessor(p)).toFixed(1)}`
    );
    const lastX = xFor(displaySeries.length - 1).toFixed(1);
    const firstX = xFor(0).toFixed(1);
    const baseY = (padTop + plotH).toFixed(1);
    return `M ${firstX},${baseY} L ${pts.join(" L ")} L ${lastX},${baseY} Z`;
  };

  const yTicks = [0, 0.25, 0.5, 0.75, 1].map((r) => ({
    value: Math.round(maxRps * r),
    y: padTop + plotH - r * plotH
  }));

  const hoveredPoint =
    hoverIdx !== null && hoverIdx >= 0 && hoverIdx < displaySeries.length
      ? displaySeries[hoverIdx]
      : null;

  const formatTime = (ms: number) => {
    const d = new Date(ms);
    return d.toLocaleTimeString("en-GB", { hour12: false });
  };

  return (
    <section className="flex flex-col rounded-md border border-white/[0.07] bg-[#0C0C0E]">
      {/* Panel Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-4 py-3">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-[#FAFAFA]">
              INGRESS THROUGHPUT &amp; SUB-MILLISECOND HISTOGRAM
            </h2>
            <span className="rounded border border-white/[0.07] bg-[#09090B] px-1.5 py-0.5 font-mono text-[10px] text-[#A1A1AA]">
              60-TICK WINDOW
            </span>
          </div>
          <p className="mt-0.5 text-xs text-[#52525B]">
            Real-time decision velocity, Wasm 5-bucket µs histogram, and adaptive load-shedding stream
          </p>
        </div>

        {/* Legend & Totals */}
        <div className="flex flex-wrap items-center gap-3.5 font-mono text-[11px] tabular-nums">
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-sm bg-[#10B981]" />
            <span className="text-[#A1A1AA]">200 OK</span>
            <span className="font-semibold text-[#FAFAFA]">{counters.allowed.toLocaleString()}</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-sm bg-[#EF4444]" />
            <span className="text-[#A1A1AA]">429</span>
            <span className="font-semibold text-[#EF4444]">{counters.throttled.toLocaleString()}</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-sm bg-[#F59E0B]" />
            <span className="text-[#A1A1AA]">503 SHED</span>
            <span className="font-semibold text-[#F59E0B]">
              {counters.shed_overload.toLocaleString()}
            </span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-sm bg-[#A1A1AA]" />
            <span className="text-[#A1A1AA]">SHADOW</span>
            <span className="font-semibold text-[#A1A1AA]">
              {counters.shadow_flagged.toLocaleString()}
            </span>
          </div>
        </div>
      </div>

      {/* High-Density Native SVG Throughput Chart */}
      <div className="relative px-3 pt-3 pb-2">
        <svg
          viewBox={`0 0 ${width} ${height}`}
          className="h-[218px] w-full select-none overflow-visible"
          onMouseLeave={() => setHoverIdx(null)}
          onMouseMove={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const relX = ((e.clientX - rect.left) / rect.width) * width;
            const norm = Math.max(0, Math.min(1, (relX - padLeft) / plotW));
            const idx = Math.round(norm * (displaySeries.length - 1));
            setHoverIdx(idx);
          }}
        >
          <defs>
            <linearGradient id="allowedAreaGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#10B981" stopOpacity="0.16" />
              <stop offset="100%" stopColor="#10B981" stopOpacity="0.0" />
            </linearGradient>
            <linearGradient id="throttledAreaGrad" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor="#EF4444" stopOpacity="0.2" />
              <stop offset="100%" stopColor="#EF4444" stopOpacity="0.0" />
            </linearGradient>
          </defs>

          {yTicks.map((t, i) => (
            <g key={i}>
              <line
                x1={padLeft}
                y1={t.y}
                x2={width - padRight}
                y2={t.y}
                stroke="rgba(255,255,255,0.06)"
                strokeWidth="1"
              />
              <text
                x={padLeft - 8}
                y={t.y + 3}
                textAnchor="end"
                className="fill-[#52525B] font-mono text-[10px]"
              >
                {t.value}
              </text>
            </g>
          ))}

          {displaySeries.map((pt, i) =>
            pt.anomaly_detected || seriesSpikeIndices.has(i) ? (
              <line
                key={`anom-${i}`}
                x1={xFor(i)}
                y1={padTop}
                x2={xFor(i)}
                y2={padTop + plotH}
                stroke="#EF4444"
                strokeOpacity="0.45"
                strokeWidth="1"
                strokeDasharray="3 3"
              />
            ) : null
          )}

          <path d={buildArea((p) => p.allowed_rps)} fill="url(#allowedAreaGrad)" />
          <path d={buildArea((p) => p.throttled_rps)} fill="url(#throttledAreaGrad)" />

          {/* Shadow line */}
          <polyline
            fill="none"
            stroke="#A1A1AA"
            strokeWidth="1.2"
            strokeDasharray="4 3"
            points={buildPolyline((p) => p.shadow_rps)}
          />

          {/* 503 Overload Shedding line */}
          <polyline
            fill="none"
            stroke="#F59E0B"
            strokeWidth="1.75"
            points={buildPolyline((p) => p.shed_rps)}
          />

          {/* Allowed line */}
          <polyline
            fill="none"
            stroke="#10B981"
            strokeWidth="1.75"
            points={buildPolyline((p) => p.allowed_rps)}
          />

          {/* Throttled 429 line */}
          <polyline
            fill="none"
            stroke="#EF4444"
            strokeWidth="1.75"
            points={buildPolyline((p) => p.throttled_rps)}
          />

          {[0, Math.floor((displaySeries.length - 1) / 2), displaySeries.length - 1].map((idx) => (
            <text
              key={`xt-${idx}`}
              x={xFor(idx)}
              y={height - 6}
              textAnchor={idx === 0 ? "start" : idx === displaySeries.length - 1 ? "end" : "middle"}
              className="fill-[#52525B] font-mono text-[10px]"
            >
              {formatTime(displaySeries[idx].timestamp_ms)}
            </text>
          ))}

          {hoverIdx !== null && hoveredPoint && (
            <g>
              <line
                x1={xFor(hoverIdx)}
                y1={padTop}
                x2={xFor(hoverIdx)}
                y2={padTop + plotH}
                stroke="rgba(255,255,255,0.25)"
                strokeWidth="1"
              />
              <circle cx={xFor(hoverIdx)} cy={yFor(hoveredPoint.allowed_rps)} r="3" fill="#10B981" />
              <circle cx={xFor(hoverIdx)} cy={yFor(hoveredPoint.throttled_rps)} r="3" fill="#EF4444" />
              <circle cx={xFor(hoverIdx)} cy={yFor(hoveredPoint.shed_rps)} r="3" fill="#F59E0B" />
            </g>
          )}
        </svg>

        {hoveredPoint && (
          <div className="pointer-events-none absolute right-5 top-4 rounded border border-white/[0.12] bg-[#09090B]/95 px-2.5 py-1.5 font-mono text-[11px] tabular-nums">
            <div className="text-[#A1A1AA]">{formatTime(hoveredPoint.timestamp_ms)}</div>
            <div className="mt-0.5 flex items-center gap-3">
              <span className="text-[#10B981]">OK {hoveredPoint.allowed_rps.toFixed(1)}/s</span>
              <span className="text-[#EF4444]">429 {hoveredPoint.throttled_rps.toFixed(1)}/s</span>
              <span className="text-[#F59E0B]">503 {hoveredPoint.shed_rps.toFixed(1)}/s</span>
              <span className="text-[#FAFAFA]">{hoveredPoint.mean_eval_us.toFixed(2)} µs</span>
            </div>
          </div>
        )}
      </div>

      {/* Wasm Percentiles + 5-Bucket Microsecond Latency Histogram */}
      <div className="grid grid-cols-1 divide-y divide-white/[0.07] border-t border-white/[0.07] xl:grid-cols-12 xl:divide-x xl:divide-y-0">
        {/* Percentiles Strip */}
        <div className="grid grid-cols-3 divide-x divide-y divide-white/[0.07] bg-[#09090B]/50 sm:grid-cols-6 sm:divide-y-0 xl:col-span-6 xl:grid-cols-3 xl:divide-y">
          {[
            { label: "WASM p50", value: `${wasmStats.p50.toFixed(2)} µs`, accent: "text-[#FAFAFA]" },
            { label: "WASM p95", value: `${wasmStats.p95.toFixed(2)} µs`, accent: "text-[#FAFAFA]" },
            { label: "WASM p99", value: `${wasmStats.p99.toFixed(2)} µs`, accent: "text-[#F59E0B]" },
            { label: "MAX EVAL", value: `${wasmStats.max.toFixed(2)} µs`, accent: "text-[#FAFAFA]" },
            { label: "MEAN EVAL", value: `${wasmStats.mean.toFixed(2)} µs`, accent: "text-[#10B981]" },
            { label: "JITTER", value: `${wasmStats.jitter.toFixed(2)} µs`, accent: "text-[#A1A1AA]" }
          ].map((metric) => (
            <div key={metric.label} className="px-3.5 py-2.5">
              <div className="text-[9px] font-medium uppercase tracking-wider text-[#52525B]">
                {metric.label}
              </div>
              <div className={`mt-0.5 font-mono text-sm font-semibold tabular-nums ${metric.accent}`}>
                {metric.value}
              </div>
            </div>
          ))}
        </div>

        {/* 5-Bucket Microsecond Histogram */}
        <div className="bg-[#09090B]/30 px-4 py-2.5 xl:col-span-6">
          <div className="mb-2 flex items-center justify-between font-mono text-[10px]">
            <span className="font-semibold uppercase tracking-wider text-[#A1A1AA]">
              WASM µs LATENCY HISTOGRAM
            </span>
            <span className="tabular-nums text-[#52525B]">
              {histogram.total_samples} SAMPLES
            </span>
          </div>
          <div className="space-y-1.5">
            {histogram.buckets.map((bucket) => {
              const barColor =
                bucket.label === "<25µs"
                  ? "bg-[#10B981]"
                  : bucket.label === "25-50µs"
                    ? "bg-[#E4E4E7]"
                    : bucket.label === "50-100µs"
                      ? "bg-[#A1A1AA]"
                      : bucket.label === "100-250µs"
                        ? "bg-[#F59E0B]"
                        : "bg-[#EF4444]";
              return (
                <div
                  key={bucket.label}
                  className="grid grid-cols-[64px_1fr_72px] items-center gap-2 font-mono text-[10px] tabular-nums"
                >
                  <span className="text-[#A1A1AA]">{bucket.label}</span>
                  <div className="h-2 overflow-hidden rounded-[2px] bg-white/[0.06]">
                    <div
                      className={`h-full transition-all duration-300 ${barColor}`}
                      style={{ width: `${Math.min(100, bucket.percentage)}%` }}
                    />
                  </div>
                  <span className="text-right text-[#FAFAFA]">
                    {bucket.count} ({bucket.percentage.toFixed(0)}%)
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* Tokio Background Janitor & Memory Telemetry Strip */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-t border-white/[0.07] bg-[#09090B]/60 px-4 py-2 font-mono text-[11px] tabular-nums">
        <div className="flex items-center gap-2 text-[#A1A1AA]">
          <Cpu className="h-3.5 w-3.5 text-[#10B981]" />
          <span className="text-[10px] font-semibold uppercase tracking-wider">
            TOKIO JANITOR (5s GC)
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-4 text-[11px]">
          <span className="text-[#52525B]">
            BUCKETS: <strong className="text-[#FAFAFA]">{janitor.active_buckets}</strong>
          </span>
          <span className="text-[#52525B]">
            EVICTED: <strong className="text-[#FAFAFA]">{janitor.keys_evicted_total}</strong>
          </span>
          <span className="text-[#52525B]">
            SWEEPS: <strong className="text-[#A1A1AA]">{janitor.sweeps_completed}</strong>
          </span>
          <span className="text-[#52525B]">
            EST MEM: <strong className="text-[#10B981]">{janitor.estimated_memory_kb} KB</strong>
          </span>
          <span className="text-[#52525B]">
            UPTIME: <strong className="text-[#A1A1AA]">{janitor.uptime_secs}s</strong>
          </span>
        </div>
      </div>

      {/* Active Z-Score (>3.0σ) Anomaly Detection Feed */}
      <div className="border-t border-white/[0.07] px-4 py-2.5">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2">
            <AlertTriangle
              className={`h-3.5 w-3.5 ${
                anomalies.length > 0 ? "text-[#EF4444]" : "text-[#52525B]"
              }`}
            />
            <span className="font-mono text-[11px] font-medium uppercase tracking-wider text-[#A1A1AA]">
              Z-SCORE ANOMALY DETECTOR (&gt; 3.0σ)
            </span>
          </div>
          <span className="font-mono text-[11px] tabular-nums text-[#52525B]">
            {anomalies.length} ACTIVE ALERTS • {wasmStats.sample_count} SAMPLES
          </span>
        </div>

        {anomalies.length > 0 ? (
          <div className="mt-2 flex flex-col gap-1.5">
            {anomalies.slice(0, 3).map((alert) => (
              <div
                key={alert.id}
                className="flex flex-wrap items-center justify-between gap-2 rounded border border-[#EF4444]/30 bg-[#EF4444]/[0.06] px-2.5 py-1.5 font-mono text-[11px] tabular-nums"
              >
                <div className="flex items-center gap-2">
                  <span className="rounded bg-[#EF4444]/20 px-1.5 py-0.5 text-[10px] font-semibold text-[#EF4444]">
                    Z = {alert.z_score.toFixed(2)}σ
                  </span>
                  <span className="font-semibold text-[#FAFAFA]">{alert.client_key}</span>
                  <span className="text-[#52525B]">•</span>
                  <span className="text-[#A1A1AA]">{alert.namespace}</span>
                  <span className="text-[#52525B]">→</span>
                  <span className="text-[#A1A1AA]">{alert.node_id}</span>
                </div>
                <div className="text-[#A1A1AA]">
                  OBS <span className="text-[#EF4444]">{alert.observed_hits}</span> vs μ{" "}
                  <span>{alert.baseline_mean.toFixed(1)}</span>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="mt-1.5 font-mono text-[11px] text-[#52525B]">
            Baseline traffic within normal Gaussian distribution (&le; 3.0σ). Trigger{" "}
            <span className="text-[#A1A1AA]">DDoS Key Spike</span> to observe real-time Z-Score isolation.
          </div>
        )}
      </div>
    </section>
  );
};
