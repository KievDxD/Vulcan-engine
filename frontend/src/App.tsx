import { useCallback, useEffect, useState } from "react";
import { BottomWorkspace } from "./components/BottomWorkspace";
import { CommandHeader } from "./components/CommandHeader";
import { ConsistentHashRingPanel } from "./components/ConsistentHashRingPanel";
import { EventTraceDrawer } from "./components/EventTraceDrawer";
import { PrometheusMetricsModal } from "./components/PrometheusMetricsModal";
import { ThroughputTelemetryPanel } from "./components/ThroughputTelemetryPanel";
import {
  configureOverloadGuard,
  createEmptySnapshot,
  deleteRule,
  fetchInitialTelemetry,
  fetchRules,
  probeEvaluate,
  toggleClusterNode,
  triggerChaosBurst,
  triggerChaosOverload,
  upsertRule
} from "./lib/api";
import { initWasmAnalytics } from "./lib/wasmAnalytics";
import {
  Algorithm,
  DecisionEvent,
  PriorityTier,
  RateLimitRule,
  TelemetrySnapshot
} from "./types/telemetry";

export function App() {
  const [snapshot, setSnapshot] = useState<TelemetrySnapshot>(createEmptySnapshot);
  const [rules, setRules] = useState<RateLimitRule[]>([]);
  const [sseConnected, setSseConnected] = useState(false);
  const [wasmMode, setWasmMode] = useState<"wasm" | "ts-bridge">("ts-bridge");
  const [burstPending, setBurstPending] = useState<"normal" | "ddos" | "overload" | null>(null);
  const [guardToggling, setGuardToggling] = useState(false);
  const [togglingNode, setTogglingNode] = useState<string | null>(null);
  const [toggleError, setToggleError] = useState<string | null>(null);
  const [selectedEvent, setSelectedEvent] = useState<DecisionEvent | null>(null);
  const [metricsModalOpen, setMetricsModalOpen] = useState(false);

  useEffect(() => {
    void initWasmAnalytics().then(setWasmMode);

    void fetchInitialTelemetry()
      .then((data) => setSnapshot(data))
      .catch(() => {});

    void fetchRules()
      .then(setRules)
      .catch(() => {});

    const es = new EventSource("/api/stream");
    es.onopen = () => setSseConnected(true);
    es.addEventListener("telemetry", (event: MessageEvent) => {
      try {
        const parsed = JSON.parse(event.data) as TelemetrySnapshot;
        setSnapshot(parsed);
        setSseConnected(true);
      } catch {
        // Ignore malformed SSE frame
      }
    });
    es.onerror = () => {
      setSseConnected(false);
    };

    return () => {
      es.close();
    };
  }, []);

  const handleSimulateNormal = useCallback(async () => {
    setBurstPending("normal");
    try {
      await triggerChaosBurst("normal", 110);
      const fresh = await fetchInitialTelemetry();
      setSnapshot(fresh);
    } finally {
      setBurstPending(null);
    }
  }, []);

  const handleSimulateDdos = useCallback(async () => {
    setBurstPending("ddos");
    try {
      await triggerChaosBurst("ddos", 160);
      const fresh = await fetchInitialTelemetry();
      setSnapshot(fresh);
    } finally {
      setBurstPending(null);
    }
  }, []);

  const handleSimulateOverload = useCallback(async () => {
    setBurstPending("overload");
    try {
      await triggerChaosOverload(150);
      const fresh = await fetchInitialTelemetry();
      setSnapshot(fresh);
    } finally {
      setBurstPending(null);
    }
  }, []);

  const handleToggleGuard = useCallback(async () => {
    setGuardToggling(true);
    try {
      const currentlyActive =
        snapshot.overload_guard.enabled && snapshot.overload_guard.shedding_active;
      const updated = await configureOverloadGuard({
        enabled: !currentlyActive,
        max_rps_threshold: snapshot.overload_guard.max_rps_threshold,
        shed_best_effort: true,
        shedding_active: !currentlyActive
      });
      setSnapshot((prev) => ({
        ...prev,
        overload_guard: updated
      }));
    } finally {
      setGuardToggling(false);
    }
  }, [snapshot.overload_guard]);

  const handleToggleNode = useCallback(async (nodeId: string) => {
    setTogglingNode(nodeId);
    setToggleError(null);
    try {
      const res = await toggleClusterNode(nodeId);
      setSnapshot((prev) => ({
        ...prev,
        topology: res.topology
      }));
    } catch (err) {
      setToggleError(err instanceof Error ? err.message : "Failed to toggle node");
    } finally {
      setTogglingNode(null);
    }
  }, []);

  const handleSaveRule = useCallback(async (rule: RateLimitRule) => {
    await upsertRule(rule);
    const updated = await fetchRules();
    setRules(updated);
  }, []);

  const handleDeleteRule = useCallback(async (namespace: string) => {
    await deleteRule(namespace);
    const updated = await fetchRules();
    setRules(updated);
  }, []);

  const handleRunProbe = useCallback(
    async (params: {
      namespace: string;
      key: string;
      tokens: number;
      algorithm?: Algorithm;
      priority?: PriorityTier;
    }) => {
      const result = await probeEvaluate(params);
      const fresh = await fetchInitialTelemetry().catch(() => null);
      if (fresh) setSnapshot(fresh);
      return result;
    },
    []
  );

  return (
    <div className="min-h-screen bg-[#09090B] text-[#FAFAFA]">
      <CommandHeader
        sseConnected={sseConnected}
        meanLatencyUs={snapshot.latency.mean_us}
        currentRps={snapshot.current_rps}
        rejectionRatePct={snapshot.rejection_rate_pct}
        totalEvaluations={snapshot.counters.total_evaluations}
        wasmMode={wasmMode}
        overloadGuard={snapshot.overload_guard}
        guardToggling={guardToggling}
        burstPending={burstPending}
        onToggleGuard={handleToggleGuard}
        onOpenMetrics={() => setMetricsModalOpen(true)}
        onSimulateNormal={handleSimulateNormal}
        onSimulateDdos={handleSimulateDdos}
        onSimulateOverload={handleSimulateOverload}
      />

      <main className="mx-auto flex max-w-[1720px] flex-col gap-4 p-4 sm:p-5">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[65fr_35fr]">
          <ThroughputTelemetryPanel
            series={snapshot.series}
            recentEvents={snapshot.recent_events}
            backendLatency={snapshot.latency}
            anomalies={snapshot.anomalies}
            counters={snapshot.counters}
            janitor={snapshot.janitor}
          />

          <ConsistentHashRingPanel
            topology={snapshot.topology}
            togglingNode={togglingNode}
            toggleError={toggleError}
            onToggleNode={handleToggleNode}
          />
        </div>

        <BottomWorkspace
          events={snapshot.recent_events}
          rules={rules}
          selectedEventId={selectedEvent?.id ?? null}
          onSelectEvent={setSelectedEvent}
          onSaveRule={handleSaveRule}
          onDeleteRule={handleDeleteRule}
          onRunProbe={handleRunProbe}
        />
      </main>

      <EventTraceDrawer event={selectedEvent} onClose={() => setSelectedEvent(null)} />
      <PrometheusMetricsModal
        open={metricsModalOpen}
        onClose={() => setMetricsModalOpen(false)}
      />
    </div>
  );
}

export default App;
