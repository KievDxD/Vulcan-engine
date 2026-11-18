export type Algorithm = "token_bucket" | "sliding_window";
export type PolicyMode = "enforce" | "shadow";
export type PriorityTier = "critical" | "standard" | "best_effort";
export type Decision = "ALLOWED" | "THROTTLED" | "SHADOW_FLAGGED" | "SHED_OVERLOAD";

export interface RateLimitRule {
  namespace: string;
  algorithm: Algorithm;
  capacity: number;
  refill_per_sec: number;
  mode: PolicyMode;
  description: string;
}

export interface OverloadGuardConfig {
  enabled: boolean;
  max_rps_threshold: number;
  shed_best_effort: boolean;
  shedding_active: boolean;
}

export interface JanitorStats {
  keys_evicted_total: number;
  last_sweep_evicted: number;
  sweeps_completed: number;
  active_buckets: number;
  uptime_secs: number;
  last_sweep_timestamp_ms: number;
  estimated_memory_kb: number;
}

export interface DecisionEvent {
  id: number;
  timestamp_ms: number;
  namespace: string;
  client_key: string;
  node_id: string;
  node_hash: string;
  algorithm: Algorithm;
  mode: PolicyMode;
  priority: PriorityTier;
  load_shed_active: boolean;
  decision: Decision;
  remaining: number;
  limit: number;
  eval_time_us: number;
}

export interface LatencySummary {
  p50_us: number;
  p95_us: number;
  p99_us: number;
  max_us: number;
  mean_us: number;
  jitter_us: number;
  sample_count: number;
}

export interface ThroughputPoint {
  timestamp_ms: number;
  allowed_rps: number;
  throttled_rps: number;
  shadow_rps: number;
  shed_rps: number;
  total_rps: number;
  mean_eval_us: number;
  anomaly_detected: boolean;
}

export interface AnomalyAlert {
  id: number;
  timestamp_ms: number;
  client_key: string;
  namespace: string;
  node_id: string;
  observed_hits: number;
  baseline_mean: number;
  z_score: number;
  severity: string;
}

export interface ClusterNodeStatus {
  node_id: string;
  region: string;
  online: boolean;
  vnodes: number;
  key_share_pct: number;
  routed_requests: number;
  primary_angle_deg: number;
}

export interface RingSegment {
  node_id: string;
  start_deg: number;
  end_deg: number;
  hash_hex: string;
}

export interface ClusterTopologySnapshot {
  hash_algorithm: string;
  virtual_replicas_per_node: number;
  total_vnodes: number;
  online_nodes: number;
  total_nodes: number;
  last_rebalance_pct: number;
  last_toggled_node: string | null;
  nodes: ClusterNodeStatus[];
  segments: RingSegment[];
}

export interface LimiterCounters {
  total_evaluations: number;
  allowed: number;
  throttled: number;
  shadow_flagged: number;
  shed_overload: number;
  keys_evicted: number;
  active_buckets: number;
}

export interface TelemetrySnapshot {
  timestamp_ms: number;
  uptime_secs: number;
  counters: LimiterCounters;
  current_rps: number;
  rejection_rate_pct: number;
  latency: LatencySummary;
  series: ThroughputPoint[];
  recent_events: DecisionEvent[];
  anomalies: AnomalyAlert[];
  topology: ClusterTopologySnapshot;
  overload_guard: OverloadGuardConfig;
  janitor: JanitorStats;
}

export interface TopologyToggleResult {
  node_id: string;
  online: boolean;
  reassigned_keys: number;
  sample_size: number;
  reassigned_pct: number;
  topology: ClusterTopologySnapshot;
}

export interface ChaosBurstResponse {
  mode: string;
  dispatched: number;
  allowed: number;
  throttled: number;
  shadow_flagged: number;
  shed_overload: number;
  mean_eval_us: number;
  new_anomalies: number;
}

export interface ChaosOverloadResponse {
  state: string;
  dispatched: number;
  allowed_critical: number;
  allowed_standard: number;
  throttled_standard: number;
  shed_best_effort: number;
  mean_eval_us: number;
}

export interface ProbeResult {
  http_status: number;
  decision: Decision;
  namespace: string;
  key: string;
  node_id: string;
  node_hash: string;
  algorithm: Algorithm;
  mode: PolicyMode;
  priority: PriorityTier;
  load_shed_active: boolean;
  limit: number;
  remaining: number;
  reset_secs: number;
  retry_after_secs: number;
  eval_time_us: number;
  headers: Record<string, string>;
}
