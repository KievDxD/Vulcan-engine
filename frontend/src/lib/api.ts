import {
  Algorithm,
  ChaosBurstResponse,
  ChaosOverloadResponse,
  ClusterTopologySnapshot,
  OverloadGuardConfig,
  PriorityTier,
  ProbeResult,
  RateLimitRule,
  TelemetrySnapshot,
  TopologyToggleResult
} from "../types/telemetry";

export async function fetchInitialTelemetry(): Promise<TelemetrySnapshot> {
  const res = await fetch("/api/telemetry");
  if (!res.ok) {
    throw new Error(`Telemetry HTTP ${res.status}`);
  }
  return res.json();
}

export async function fetchRules(): Promise<RateLimitRule[]> {
  const res = await fetch("/api/rules");
  if (!res.ok) {
    throw new Error(`Rules HTTP ${res.status}`);
  }
  return res.json();
}

export async function upsertRule(rule: RateLimitRule): Promise<RateLimitRule> {
  const res = await fetch("/api/rules", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(rule)
  });
  if (!res.ok) {
    throw new Error(`Failed to save rule: HTTP ${res.status}`);
  }
  return res.json();
}

export async function deleteRule(namespace: string): Promise<void> {
  const res = await fetch(`/api/rules/${encodeURIComponent(namespace)}`, {
    method: "DELETE"
  });
  if (!res.ok) {
    throw new Error(`Failed to delete rule: HTTP ${res.status}`);
  }
}

export async function fetchClusterTopology(): Promise<ClusterTopologySnapshot> {
  const res = await fetch("/api/cluster/topology");
  if (!res.ok) {
    throw new Error(`Topology HTTP ${res.status}`);
  }
  return res.json();
}

export async function toggleClusterNode(nodeId: string): Promise<TopologyToggleResult> {
  const res = await fetch(`/api/cluster/nodes/${encodeURIComponent(nodeId)}/toggle`, {
    method: "POST"
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
    throw new Error(err.error || `Node toggle HTTP ${res.status}`);
  }
  return res.json();
}

export async function configureOverloadGuard(params: {
  enabled: boolean;
  max_rps_threshold?: number;
  shed_best_effort?: boolean;
  shedding_active?: boolean;
}): Promise<OverloadGuardConfig> {
  const res = await fetch("/api/system/overload-guard", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(params)
  });
  if (!res.ok) {
    throw new Error(`Overload guard HTTP ${res.status}`);
  }
  return res.json();
}

export async function triggerChaosBurst(
  mode: "normal" | "ddos",
  count?: number
): Promise<ChaosBurstResponse> {
  const res = await fetch("/api/chaos/burst", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ mode, count })
  });
  if (!res.ok) {
    throw new Error(`Chaos burst HTTP ${res.status}`);
  }
  return res.json();
}

export async function triggerChaosOverload(count = 150): Promise<ChaosOverloadResponse> {
  const res = await fetch("/api/chaos/overload", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ count })
  });
  if (!res.ok) {
    throw new Error(`Chaos overload HTTP ${res.status}`);
  }
  return res.json();
}

export async function fetchPrometheusMetrics(): Promise<string> {
  const res = await fetch("/metrics");
  if (!res.ok) {
    throw new Error(`Metrics HTTP ${res.status}`);
  }
  return res.text();
}

export async function probeEvaluate(params: {
  namespace: string;
  key: string;
  tokens: number;
  algorithm?: Algorithm;
  priority?: PriorityTier;
}): Promise<ProbeResult> {
  const res = await fetch("/v1/evaluate", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(params)
  });
  const headers: Record<string, string> = {};
  for (const name of [
    "X-RateLimit-Limit",
    "X-RateLimit-Remaining",
    "X-RateLimit-Reset",
    "X-Vulcan-Node",
    "X-Vulcan-Priority",
    "X-Vulcan-Load-Shed",
    "Retry-After"
  ]) {
    const val = res.headers.get(name);
    if (val !== null) {
      headers[name] = val;
    }
  }
  const body = await res.json();
  return {
    ...body,
    http_status: res.status,
    headers
  };
}

export function createEmptySnapshot(): TelemetrySnapshot {
  const now = Date.now();
  return {
    timestamp_ms: now,
    uptime_secs: 0,
    counters: {
      total_evaluations: 0,
      allowed: 0,
      throttled: 0,
      shadow_flagged: 0,
      shed_overload: 0,
      keys_evicted: 0,
      active_buckets: 0
    },
    current_rps: 0,
    rejection_rate_pct: 0,
    latency: {
      p50_us: 0,
      p95_us: 0,
      p99_us: 0,
      max_us: 0,
      mean_us: 0,
      jitter_us: 0,
      sample_count: 0
    },
    series: [],
    recent_events: [],
    anomalies: [],
    overload_guard: {
      enabled: false,
      max_rps_threshold: 450,
      shed_best_effort: true,
      shedding_active: false
    },
    janitor: {
      keys_evicted_total: 0,
      last_sweep_evicted: 0,
      sweeps_completed: 0,
      active_buckets: 0,
      uptime_secs: 0,
      last_sweep_timestamp_ms: now,
      estimated_memory_kb: 128
    },
    topology: {
      hash_algorithm: "FNV-1a 64-bit",
      virtual_replicas_per_node: 64,
      total_vnodes: 256,
      online_nodes: 4,
      total_nodes: 4,
      last_rebalance_pct: 0,
      last_toggled_node: null,
      nodes: [
        {
          node_id: "edge-iad-01",
          region: "us-east-1",
          online: true,
          vnodes: 64,
          key_share_pct: 25,
          routed_requests: 0,
          primary_angle_deg: 45
        },
        {
          node_id: "edge-gru-02",
          region: "sa-east-1",
          online: true,
          vnodes: 64,
          key_share_pct: 25,
          routed_requests: 0,
          primary_angle_deg: 135
        },
        {
          node_id: "edge-bog-03",
          region: "sa-north-1",
          online: true,
          vnodes: 64,
          key_share_pct: 25,
          routed_requests: 0,
          primary_angle_deg: 225
        },
        {
          node_id: "edge-fra-04",
          region: "eu-central-1",
          online: true,
          vnodes: 64,
          key_share_pct: 25,
          routed_requests: 0,
          primary_angle_deg: 315
        }
      ],
      segments: []
    }
  };
}
