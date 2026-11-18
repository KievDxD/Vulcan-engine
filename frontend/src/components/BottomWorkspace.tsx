import React, { useMemo, useState } from "react";
import { Play, Plus, Search, Sliders, Terminal, Trash2 } from "lucide-react";
import {
  Algorithm,
  Decision,
  DecisionEvent,
  PolicyMode,
  PriorityTier,
  ProbeResult,
  RateLimitRule
} from "../types/telemetry";

interface BottomWorkspaceProps {
  events: DecisionEvent[];
  rules: RateLimitRule[];
  selectedEventId: number | null;
  onSelectEvent: (event: DecisionEvent) => void;
  onSaveRule: (rule: RateLimitRule) => Promise<void>;
  onDeleteRule: (namespace: string) => Promise<void>;
  onRunProbe: (params: {
    namespace: string;
    key: string;
    tokens: number;
    algorithm?: Algorithm;
    priority?: PriorityTier;
  }) => Promise<ProbeResult>;
}

type BottomTab = "stream" | "rules" | "probe";

export const BottomWorkspace: React.FC<BottomWorkspaceProps> = ({
  events,
  rules,
  selectedEventId,
  onSelectEvent,
  onSaveRule,
  onDeleteRule,
  onRunProbe
}) => {
  const [activeTab, setActiveTab] = useState<BottomTab>("stream");
  const [searchTerm, setSearchTerm] = useState("");
  const [decisionFilter, setDecisionFilter] = useState<"ALL" | Decision>("ALL");

  const [nsInput, setNsInput] = useState("partner-webhooks");
  const [algoInput, setAlgoInput] = useState<Algorithm>("token_bucket");
  const [capacityInput, setCapacityInput] = useState(50);
  const [refillInput, setRefillInput] = useState(15);
  const [modeInput, setModeInput] = useState<PolicyMode>("enforce");
  const [descInput, setDescInput] = useState("Outbound webhook delivery SLA policy");
  const [savingRule, setSavingRule] = useState(false);

  const [probeNs, setProbeNs] = useState("ai-inference");
  const [probeKey, setProbeKey] = useState("client-edge-10.42.0.9");
  const [probeTokens, setProbeTokens] = useState(1);
  const [probeAlgo, setProbeAlgo] = useState<"" | Algorithm>("");
  const [probePriority, setProbePriority] = useState<PriorityTier>("standard");
  const [probeResult, setProbeResult] = useState<ProbeResult | null>(null);
  const [probeRunning, setProbeRunning] = useState(false);

  const filteredEvents = useMemo(() => {
    const q = searchTerm.trim().toLowerCase();
    return events.filter((ev) => {
      if (decisionFilter !== "ALL" && ev.decision !== decisionFilter) return false;
      if (!q) return true;
      return (
        ev.client_key.toLowerCase().includes(q) ||
        ev.namespace.toLowerCase().includes(q) ||
        ev.node_id.toLowerCase().includes(q) ||
        ev.priority.toLowerCase().includes(q)
      );
    });
  }, [events, searchTerm, decisionFilter]);

  const handleSaveRuleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!nsInput.trim()) return;
    setSavingRule(true);
    try {
      await onSaveRule({
        namespace: nsInput.trim(),
        algorithm: algoInput,
        capacity: Math.max(1, Number(capacityInput)),
        refill_per_sec: Math.max(0.1, Number(refillInput)),
        mode: modeInput,
        description: descInput.trim()
      });
    } finally {
      setSavingRule(false);
    }
  };

  const handleSingleProbe = async () => {
    setProbeRunning(true);
    try {
      const res = await onRunProbe({
        namespace: probeNs,
        key: probeKey,
        tokens: Math.max(1, Number(probeTokens)),
        algorithm: probeAlgo === "" ? undefined : probeAlgo,
        priority: probePriority
      });
      setProbeResult(res);
    } finally {
      setProbeRunning(false);
    }
  };

  const handleRapidProbe = async () => {
    setProbeRunning(true);
    try {
      let last: ProbeResult | null = null;
      for (let i = 0; i < 15; i++) {
        last = await onRunProbe({
          namespace: probeNs,
          key: probeKey,
          tokens: Math.max(1, Number(probeTokens)),
          algorithm: probeAlgo === "" ? undefined : probeAlgo,
          priority: probePriority
        });
      }
      if (last) setProbeResult(last);
    } finally {
      setProbeRunning(false);
    }
  };

  const formatTimestamp = (ms: number) => {
    const d = new Date(ms);
    const time = d.toLocaleTimeString("en-GB", { hour12: false });
    const millis = String(d.getMilliseconds()).padStart(3, "0");
    return `${time}.${millis}`;
  };

  return (
    <section className="rounded-md border border-white/[0.07] bg-[#0C0C0E]">
      {/* Tab Navigation Bar */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/[0.07] px-4 py-2.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <button
            type="button"
            onClick={() => setActiveTab("stream")}
            className={`inline-flex items-center gap-1.5 rounded px-3 py-1.5 font-mono text-[11px] font-medium transition-colors ${
              activeTab === "stream"
                ? "bg-white text-[#09090B]"
                : "text-[#A1A1AA] hover:bg-white/[0.05] hover:text-[#FAFAFA]"
            }`}
          >
            <Terminal className="h-3.5 w-3.5" />
            LIVE DECISION STREAM
            <span
              className={`rounded px-1.5 py-0.2 text-[10px] tabular-nums ${
                activeTab === "stream"
                  ? "bg-[#09090B]/15 text-[#09090B]"
                  : "bg-[#09090B] text-[#A1A1AA]"
              }`}
            >
              {events.length}
            </span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("rules")}
            className={`inline-flex items-center gap-1.5 rounded px-3 py-1.5 font-mono text-[11px] font-medium transition-colors ${
              activeTab === "rules"
                ? "bg-white text-[#09090B]"
                : "text-[#A1A1AA] hover:bg-white/[0.05] hover:text-[#FAFAFA]"
            }`}
          >
            <Sliders className="h-3.5 w-3.5" />
            RATE LIMIT RULES
            <span
              className={`rounded px-1.5 py-0.2 text-[10px] tabular-nums ${
                activeTab === "rules"
                  ? "bg-[#09090B]/15 text-[#09090B]"
                  : "bg-[#09090B] text-[#A1A1AA]"
              }`}
            >
              {rules.length}
            </span>
          </button>

          <button
            type="button"
            onClick={() => setActiveTab("probe")}
            className={`inline-flex items-center gap-1.5 rounded px-3 py-1.5 font-mono text-[11px] font-medium transition-colors ${
              activeTab === "probe"
                ? "bg-white text-[#09090B]"
                : "text-[#A1A1AA] hover:bg-white/[0.05] hover:text-[#FAFAFA]"
            }`}
          >
            <Play className="h-3.5 w-3.5" />
            INTERACTIVE PROBE // /v1/evaluate
          </button>
        </div>

        {activeTab === "stream" && (
          <div className="flex flex-wrap items-center gap-2">
            <span className="hidden font-mono text-[10px] text-[#52525B] xl:inline">
              CLICK ANY ROW TO OPEN EVENT TRACE DRAWER
            </span>
            <div className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[#52525B]" />
              <input
                type="text"
                value={searchTerm}
                onChange={(e) => setSearchTerm(e.target.value)}
                placeholder="Filter key, ns, node, tier..."
                className="w-52 rounded border border-white/[0.07] bg-[#09090B] py-1 pl-8 pr-2.5 font-mono text-[11px] text-[#FAFAFA] placeholder-[#52525B] focus:border-white/30 focus:outline-none"
              />
            </div>

            <div className="flex items-center rounded border border-white/[0.07] bg-[#09090B] p-0.5 font-mono text-[10px]">
              {(["ALL", "ALLOWED", "THROTTLED", "SHED_OVERLOAD", "SHADOW_FLAGGED"] as const).map(
                (status) => (
                  <button
                    key={status}
                    type="button"
                    onClick={() => setDecisionFilter(status)}
                    className={`rounded px-2 py-0.5 transition-colors ${
                      decisionFilter === status
                        ? "bg-white/[0.12] text-[#FAFAFA]"
                        : "text-[#52525B] hover:text-[#A1A1AA]"
                    }`}
                  >
                    {status === "THROTTLED"
                      ? "429"
                      : status === "SHED_OVERLOAD"
                        ? "503 SHED"
                        : status === "SHADOW_FLAGGED"
                          ? "SHADOW"
                          : status}
                  </button>
                )
              )}
            </div>
          </div>
        )}
      </div>

      {/* Tab 1: Live Decision Stream */}
      {activeTab === "stream" && (
        <div className="max-h-[340px] overflow-auto">
          <table className="w-full border-collapse text-left font-mono text-[11px] tabular-nums">
            <thead className="sticky top-0 z-10 border-b border-white/[0.07] bg-[#09090B] text-[10px] uppercase tracking-wider text-[#52525B]">
              <tr>
                <th className="px-4 py-2 font-medium">TIMESTAMP</th>
                <th className="px-3 py-2 font-medium">CLIENT KEY</th>
                <th className="px-3 py-2 font-medium">NAMESPACE</th>
                <th className="px-3 py-2 font-medium">RING NODE</th>
                <th className="px-3 py-2 font-medium">PRIORITY</th>
                <th className="px-3 py-2 font-medium">ALGORITHM</th>
                <th className="px-3 py-2 font-medium text-right">REMAINING</th>
                <th className="px-3 py-2 font-medium text-right">EVAL (µs)</th>
                <th className="px-4 py-2 font-medium text-right">DECISION</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/[0.04]">
              {filteredEvents.length === 0 ? (
                <tr>
                  <td colSpan={9} className="px-4 py-8 text-center text-[#52525B]">
                    No decision events recorded yet. Trigger{" "}
                    <span className="text-[#A1A1AA]">Normal Traffic</span>,{" "}
                    <span className="text-[#A1A1AA]">Cascade Overload</span>, or run an{" "}
                    <span className="text-[#A1A1AA]">Interactive Probe</span>.
                  </td>
                </tr>
              ) : (
                filteredEvents.slice(0, 80).map((ev) => {
                  const selected = selectedEventId === ev.id;
                  return (
                    <tr
                      key={ev.id}
                      onClick={() => onSelectEvent(ev)}
                      className={`cursor-pointer transition-colors ${
                        selected ? "bg-white/[0.07]" : "hover:bg-white/[0.03]"
                      }`}
                    >
                      <td className="whitespace-nowrap px-4 py-1.5 text-[#52525B]">
                        {formatTimestamp(ev.timestamp_ms)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 font-medium text-[#FAFAFA]">
                        {ev.client_key}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 text-[#A1A1AA]">
                        {ev.namespace}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 text-[#A1A1AA]">
                        {ev.node_id}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5">
                        <span
                          className={`rounded border px-1.5 py-0.5 text-[9px] uppercase ${
                            ev.priority === "critical"
                              ? "border-[#10B981]/30 text-[#10B981]"
                              : ev.priority === "best_effort"
                                ? "border-[#52525B]/50 text-[#52525B]"
                                : "border-white/[0.08] text-[#A1A1AA]"
                          }`}
                        >
                          {ev.priority === "best_effort"
                            ? "BEST"
                            : ev.priority === "critical"
                              ? "CRIT"
                              : "STD"}
                        </span>
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 text-[#52525B]">
                        {ev.algorithm === "token_bucket" ? "TOKEN_BUCKET" : "SLIDING_WIN"}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 text-right text-[#A1A1AA]">
                        {ev.remaining} / {ev.limit}
                      </td>
                      <td className="whitespace-nowrap px-3 py-1.5 text-right text-[#FAFAFA]">
                        {ev.eval_time_us.toFixed(2)}
                      </td>
                      <td className="whitespace-nowrap px-4 py-1.5 text-right">
                        <span
                          className={`inline-block rounded border px-1.5 py-0.5 text-[10px] font-semibold ${
                            ev.decision === "ALLOWED"
                              ? "border-[#10B981]/30 bg-[#10B981]/10 text-[#10B981]"
                              : ev.decision === "THROTTLED"
                                ? "border-[#EF4444]/30 bg-[#EF4444]/10 text-[#EF4444]"
                                : "border-[#F59E0B]/30 bg-[#F59E0B]/10 text-[#F59E0B]"
                          }`}
                        >
                          {ev.decision === "THROTTLED"
                            ? "429 THROTTLED"
                            : ev.decision === "SHED_OVERLOAD"
                              ? "503 SHED"
                              : ev.decision}
                        </span>
                      </td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
      )}

      {/* Tab 2: Rate Limit Rules Configuration */}
      {activeTab === "rules" && (
        <div className="grid grid-cols-1 divide-y divide-white/[0.07] lg:grid-cols-12 lg:divide-x lg:divide-y-0">
          <div className="overflow-auto lg:col-span-8">
            <table className="w-full border-collapse text-left font-mono text-[11px] tabular-nums">
              <thead className="border-b border-white/[0.07] bg-[#09090B] text-[10px] uppercase tracking-wider text-[#52525B]">
                <tr>
                  <th className="px-4 py-2 font-medium">NAMESPACE</th>
                  <th className="px-3 py-2 font-medium">ALGORITHM</th>
                  <th className="px-3 py-2 font-medium text-right">CAPACITY</th>
                  <th className="px-3 py-2 font-medium text-right">REFILL / SEC</th>
                  <th className="px-3 py-2 font-medium">POLICY MODE</th>
                  <th className="px-4 py-2 font-medium text-right">ACTIONS</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/[0.04]">
                {rules.map((rule) => (
                  <tr key={rule.namespace} className="hover:bg-white/[0.02]">
                    <td className="px-4 py-2.5">
                      <div className="font-semibold text-[#FAFAFA]">{rule.namespace}</div>
                      <div className="font-sans text-[11px] text-[#52525B]">
                        {rule.description || "Custom namespace policy"}
                      </div>
                    </td>
                    <td className="px-3 py-2.5">
                      <button
                        type="button"
                        onClick={() =>
                          onSaveRule({
                            ...rule,
                            algorithm:
                              rule.algorithm === "token_bucket" ? "sliding_window" : "token_bucket"
                          })
                        }
                        className="rounded border border-white/[0.12] bg-[#09090B] px-2 py-0.5 text-[10px] text-[#A1A1AA] hover:text-[#FAFAFA]"
                      >
                        {rule.algorithm === "token_bucket" ? "TOKEN BUCKET" : "SLIDING WINDOW"}
                      </button>
                    </td>
                    <td className="px-3 py-2.5 text-right text-[#FAFAFA]">{rule.capacity}</td>
                    <td className="px-3 py-2.5 text-right text-[#A1A1AA]">
                      {rule.refill_per_sec}/s
                    </td>
                    <td className="px-3 py-2.5">
                      <button
                        type="button"
                        onClick={() =>
                          onSaveRule({
                            ...rule,
                            mode: rule.mode === "enforce" ? "shadow" : "enforce"
                          })
                        }
                        className={`rounded border px-2 py-0.5 text-[10px] font-semibold ${
                          rule.mode === "enforce"
                            ? "border-[#10B981]/30 bg-[#10B981]/10 text-[#10B981]"
                            : "border-[#F59E0B]/30 bg-[#F59E0B]/10 text-[#F59E0B]"
                        }`}
                      >
                        {rule.mode.toUpperCase()}
                      </button>
                    </td>
                    <td className="px-4 py-2.5 text-right">
                      <button
                        type="button"
                        onClick={() => onDeleteRule(rule.namespace)}
                        className="inline-flex items-center rounded p-1 text-[#52525B] hover:bg-[#EF4444]/10 hover:text-[#EF4444]"
                        title="Delete rule"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <form onSubmit={handleSaveRuleSubmit} className="space-y-3 p-4 lg:col-span-4">
            <div className="font-mono text-[11px] font-semibold uppercase tracking-wider text-[#FAFAFA]">
              UPSERT NAMESPACE POLICY
            </div>
            <div className="grid grid-cols-2 gap-2.5">
              <label className="col-span-2 block">
                <span className="text-[11px] text-[#A1A1AA]">Namespace</span>
                <input
                  type="text"
                  value={nsInput}
                  onChange={(e) => setNsInput(e.target.value)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                />
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Algorithm</span>
                <select
                  value={algoInput}
                  onChange={(e) => setAlgoInput(e.target.value as Algorithm)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                >
                  <option value="token_bucket">Token Bucket</option>
                  <option value="sliding_window">Sliding Window</option>
                </select>
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Mode</span>
                <select
                  value={modeInput}
                  onChange={(e) => setModeInput(e.target.value as PolicyMode)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                >
                  <option value="enforce">ENFORCE (429)</option>
                  <option value="shadow">SHADOW (AUDIT)</option>
                </select>
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Capacity</span>
                <input
                  type="number"
                  min={1}
                  value={capacityInput}
                  onChange={(e) => setCapacityInput(Number(e.target.value))}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs tabular-nums text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                />
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Refill / sec</span>
                <input
                  type="number"
                  step="0.5"
                  min={0.5}
                  value={refillInput}
                  onChange={(e) => setRefillInput(Number(e.target.value))}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs tabular-nums text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                />
              </label>

              <label className="col-span-2 block">
                <span className="text-[11px] text-[#A1A1AA]">Description</span>
                <input
                  type="text"
                  value={descInput}
                  onChange={(e) => setDescInput(e.target.value)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                />
              </label>
            </div>

            <button
              type="submit"
              disabled={savingRule}
              className="inline-flex w-full items-center justify-center gap-1.5 rounded-md bg-white px-3 py-1.5 text-xs font-semibold text-[#09090B] hover:opacity-90 disabled:opacity-50"
            >
              <Plus className="h-3.5 w-3.5" />
              {savingRule ? "Persisting..." : "Save & Persist Policy"}
            </button>
          </form>
        </div>
      )}

      {/* Tab 3: Interactive Probe against POST /v1/evaluate */}
      {activeTab === "probe" && (
        <div className="grid grid-cols-1 divide-y divide-white/[0.07] lg:grid-cols-12 lg:divide-x lg:divide-y-0">
          <div className="space-y-3 p-4 lg:col-span-5">
            <div className="font-mono text-[11px] font-semibold uppercase tracking-wider text-[#FAFAFA]">
              POST /v1/evaluate PAYLOAD
            </div>

            <div className="grid grid-cols-2 gap-2.5">
              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Namespace</span>
                <select
                  value={probeNs}
                  onChange={(e) => setProbeNs(e.target.value)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                >
                  {rules.map((r) => (
                    <option key={r.namespace} value={r.namespace}>
                      {r.namespace}
                    </option>
                  ))}
                </select>
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Priority Tier</span>
                <select
                  value={probePriority}
                  onChange={(e) => setProbePriority(e.target.value as PriorityTier)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                >
                  <option value="critical">critical</option>
                  <option value="standard">standard</option>
                  <option value="best_effort">best_effort</option>
                </select>
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Client Key</span>
                <input
                  type="text"
                  value={probeKey}
                  onChange={(e) => setProbeKey(e.target.value)}
                  className="mt-1 w-full rounded border border-white/[0.07] bg-[#09090B] px-2.5 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                />
              </label>

              <label className="block">
                <span className="text-[11px] text-[#A1A1AA]">Algorithm / Tokens</span>
                <div className="mt-1 flex gap-1.5">
                  <select
                    value={probeAlgo}
                    onChange={(e) => setProbeAlgo(e.target.value as "" | Algorithm)}
                    className="w-full rounded border border-white/[0.07] bg-[#09090B] px-2 py-1.5 font-mono text-xs text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                  >
                    <option value="">Default</option>
                    <option value="token_bucket">token_bucket</option>
                    <option value="sliding_window">sliding_window</option>
                  </select>
                  <input
                    type="number"
                    min={1}
                    value={probeTokens}
                    onChange={(e) => setProbeTokens(Number(e.target.value))}
                    className="w-16 rounded border border-white/[0.07] bg-[#09090B] px-2 py-1.5 font-mono text-xs tabular-nums text-[#FAFAFA] focus:border-white/30 focus:outline-none"
                  />
                </div>
              </label>
            </div>

            <div className="flex flex-wrap gap-2 pt-1">
              <button
                type="button"
                disabled={probeRunning}
                onClick={handleSingleProbe}
                className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-md bg-white px-3 py-1.5 text-xs font-semibold text-[#09090B] hover:opacity-90 disabled:opacity-50"
              >
                <Play className="h-3.5 w-3.5" />
                Send Probe (1x)
              </button>
              <button
                type="button"
                disabled={probeRunning}
                onClick={handleRapidProbe}
                className="inline-flex items-center justify-center gap-1.5 rounded-md border border-white/[0.12] bg-[#09090B] px-3 py-1.5 font-mono text-xs text-[#FAFAFA] hover:border-white/30 disabled:opacity-50"
              >
                Fire 15x Burst
              </button>
            </div>
          </div>

          <div className="p-4 lg:col-span-7">
            <div className="flex items-center justify-between">
              <span className="font-mono text-[11px] font-semibold uppercase tracking-wider text-[#FAFAFA]">
                HTTP RESPONSE &amp; RATE-LIMIT HEADERS
              </span>
              {probeResult && (
                <span
                  className={`rounded border px-2 py-0.5 font-mono text-[11px] font-semibold tabular-nums ${
                    probeResult.http_status === 503
                      ? "border-[#F59E0B]/40 bg-[#F59E0B]/10 text-[#F59E0B]"
                      : probeResult.http_status === 429
                        ? "border-[#EF4444]/40 bg-[#EF4444]/10 text-[#EF4444]"
                        : "border-[#10B981]/40 bg-[#10B981]/10 text-[#10B981]"
                  }`}
                >
                  HTTP {probeResult.http_status}{" "}
                  {probeResult.http_status === 503
                    ? "SERVICE UNAVAILABLE"
                    : probeResult.http_status === 429
                      ? "TOO MANY REQUESTS"
                      : "OK"}{" "}
                  • {probeResult.eval_time_us.toFixed(2)} µs
                </span>
              )}
            </div>

            {probeResult ? (
              <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div className="rounded border border-white/[0.07] bg-[#09090B] p-3 font-mono text-[11px] tabular-nums">
                  <div className="mb-1.5 text-[10px] uppercase text-[#52525B]">
                    STANDARD RESPONSE HEADERS
                  </div>
                  {Object.entries(probeResult.headers).map(([k, v]) => (
                    <div key={k} className="flex justify-between py-0.5">
                      <span className="text-[#A1A1AA]">{k}:</span>
                      <span className="font-semibold text-[#FAFAFA]">{v}</span>
                    </div>
                  ))}
                  {Object.keys(probeResult.headers).length === 0 && (
                    <div className="text-[#52525B]">No rate-limit headers returned</div>
                  )}
                </div>

                <div className="rounded border border-white/[0.07] bg-[#09090B] p-3 font-mono text-[11px] tabular-nums">
                  <div className="mb-1.5 text-[10px] uppercase text-[#52525B]">
                    ENGINE EVALUATION METADATA
                  </div>
                  <div className="flex justify-between py-0.5">
                    <span className="text-[#A1A1AA]">decision:</span>
                    <span
                      className={
                        probeResult.decision === "ALLOWED"
                          ? "text-[#10B981]"
                          : probeResult.decision === "THROTTLED"
                            ? "text-[#EF4444]"
                            : "text-[#F59E0B]"
                      }
                    >
                      {probeResult.decision}
                    </span>
                  </div>
                  <div className="flex justify-between py-0.5">
                    <span className="text-[#A1A1AA]">priority:</span>
                    <span className="text-[#FAFAFA]">{probeResult.priority}</span>
                  </div>
                  <div className="flex justify-between py-0.5">
                    <span className="text-[#A1A1AA]">ring_node:</span>
                    <span className="text-[#FAFAFA]">{probeResult.node_id}</span>
                  </div>
                  <div className="flex justify-between py-0.5">
                    <span className="text-[#A1A1AA]">fnv1a_hash:</span>
                    <span className="text-[#A1A1AA]">{probeResult.node_hash}</span>
                  </div>
                  <div className="flex justify-between py-0.5">
                    <span className="text-[#A1A1AA]">remaining:</span>
                    <span className="text-[#FAFAFA]">
                      {probeResult.remaining} / {probeResult.limit}
                    </span>
                  </div>
                </div>
              </div>
            ) : (
              <div className="mt-3 rounded border border-dashed border-white/[0.07] bg-[#09090B] p-6 text-center font-mono text-xs text-[#52525B]">
                Execute a probe request to inspect live HTTP 200 / 429 / 503 headers, priority tier
                routing, Consistent Hash node assignment, and microsecond evaluation latency.
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
};
