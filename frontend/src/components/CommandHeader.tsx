import React from "react";
import { Activity, FileCode2, Flame, ShieldAlert, ShieldCheck, Zap } from "lucide-react";
import { OverloadGuardConfig } from "../types/telemetry";

interface CommandHeaderProps {
  sseConnected: boolean;
  meanLatencyUs: number;
  currentRps: number;
  rejectionRatePct: number;
  totalEvaluations: number;
  wasmMode: "wasm" | "ts-bridge";
  overloadGuard: OverloadGuardConfig;
  guardToggling: boolean;
  burstPending: "normal" | "ddos" | "overload" | null;
  onToggleGuard: () => void;
  onOpenMetrics: () => void;
  onSimulateNormal: () => void;
  onSimulateDdos: () => void;
  onSimulateOverload: () => void;
}

export const CommandHeader: React.FC<CommandHeaderProps> = ({
  sseConnected,
  meanLatencyUs,
  currentRps,
  rejectionRatePct,
  totalEvaluations,
  wasmMode,
  overloadGuard,
  guardToggling,
  burstPending,
  onToggleGuard,
  onOpenMetrics,
  onSimulateNormal,
  onSimulateDdos,
  onSimulateOverload
}) => {
  const sheddingActive = overloadGuard.enabled && overloadGuard.shedding_active;

  return (
    <header className="border-b border-white/[0.07] bg-[#0C0C0E] px-4 py-2.5 sm:px-6">
      <div className="mx-auto flex max-w-[1720px] flex-wrap items-center justify-between gap-3">
        {/* Identity, Runtime & Guard Status */}
        <div className="flex flex-wrap items-center gap-2.5">
          <div className="flex items-center gap-2">
            <span className="inline-flex h-2.5 w-2.5 rounded-sm bg-white" />
            <h1 className="font-mono text-sm font-semibold tracking-wider text-[#FAFAFA]">
              VULCAN // ENGINE
            </h1>
            <span className="rounded border border-white/[0.12] bg-[#09090B] px-1.5 py-0.5 font-mono text-[10px] font-medium text-[#A1A1AA]">
              V2.0 ENTERPRISE
            </span>
          </div>

          <div className="hidden h-4 w-px bg-white/[0.07] md:block" />

          <span className="rounded border border-white/[0.07] bg-[#09090B] px-2 py-0.5 font-mono text-[10px] tracking-tight text-[#A1A1AA]">
            RUST / TOKIO • {wasmMode === "wasm" ? "WASM32" : "WASM BRIDGE"}
          </span>

          <span
            className={`inline-flex items-center gap-1.5 rounded border px-2 py-0.5 font-mono text-[10px] ${
              sseConnected
                ? "border-[#10B981]/30 bg-[#10B981]/10 text-[#10B981]"
                : "border-[#F59E0B]/30 bg-[#F59E0B]/10 text-[#F59E0B]"
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                sseConnected ? "bg-[#10B981]" : "bg-[#F59E0B]"
              }`}
            />
            {sseConnected ? "SSE LIVE" : "RECONNECTING"}
          </span>

          {/* Load Shedding Guard Status & Toggle */}
          <button
            type="button"
            disabled={guardToggling}
            onClick={onToggleGuard}
            title="Click to toggle Adaptive Load-Shedding Guard (503 best_effort shedding)"
            className={`inline-flex items-center gap-1.5 rounded border px-2 py-0.5 font-mono text-[10px] font-semibold transition-colors disabled:opacity-60 ${
              sheddingActive
                ? "border-[#F59E0B]/50 bg-[#F59E0B]/15 text-[#F59E0B] hover:bg-[#F59E0B]/25"
                : "border-[#10B981]/30 bg-[#10B981]/10 text-[#10B981] hover:bg-[#10B981]/20"
            }`}
          >
            {sheddingActive ? (
              <ShieldAlert className="h-3 w-3" />
            ) : (
              <ShieldCheck className="h-3 w-3" />
            )}
            GUARD: {sheddingActive ? "SHEDDING ACTIVE" : "NOMINAL"}
          </button>

          <button
            type="button"
            onClick={onOpenMetrics}
            className="inline-flex items-center gap-1 rounded border border-white/[0.08] bg-[#09090B] px-2 py-0.5 font-mono text-[10px] text-[#A1A1AA] transition-colors hover:border-white/25 hover:text-[#FAFAFA]"
          >
            <FileCode2 className="h-3 w-3" />
            /metrics
          </button>
        </div>

        {/* Live KPI Strip & 3 Stress Simulators */}
        <div className="flex flex-wrap items-center gap-3 sm:gap-4">
          <div className="flex items-center gap-4 border-r border-white/[0.07] pr-4">
            <div>
              <div className="text-[9px] font-medium uppercase tracking-wider text-[#52525B]">
                MEAN EVAL
              </div>
              <div className="font-mono text-xs font-semibold tabular-nums text-[#FAFAFA]">
                {meanLatencyUs.toFixed(2)} <span className="text-[10px] text-[#A1A1AA]">µs</span>
              </div>
            </div>
            <div>
              <div className="text-[9px] font-medium uppercase tracking-wider text-[#52525B]">
                THROUGHPUT
              </div>
              <div className="font-mono text-xs font-semibold tabular-nums text-[#10B981]">
                {currentRps.toFixed(1)} <span className="text-[10px] text-[#A1A1AA]">RPS</span>
              </div>
            </div>
            <div>
              <div className="text-[9px] font-medium uppercase tracking-wider text-[#52525B]">
                DROP RATE
              </div>
              <div
                className={`font-mono text-xs font-semibold tabular-nums ${
                  rejectionRatePct > 15 ? "text-[#EF4444]" : "text-[#FAFAFA]"
                }`}
              >
                {rejectionRatePct.toFixed(1)}%
              </div>
            </div>
            <div className="hidden xl:block">
              <div className="text-[9px] font-medium uppercase tracking-wider text-[#52525B]">
                TOTAL EVALS
              </div>
              <div className="font-mono text-xs font-semibold tabular-nums text-[#A1A1AA]">
                {totalEvaluations.toLocaleString()}
              </div>
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <button
              type="button"
              disabled={burstPending !== null}
              onClick={onSimulateNormal}
              className="inline-flex items-center gap-1.5 rounded border border-white/20 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-[#09090B] transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {burstPending === "normal" ? (
                <Activity className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Zap className="h-3.5 w-3.5" />
              )}
              Normal Traffic
            </button>

            <button
              type="button"
              disabled={burstPending !== null}
              onClick={onSimulateDdos}
              className="inline-flex items-center gap-1.5 rounded border border-[#EF4444]/40 bg-[#EF4444]/10 px-2.5 py-1.5 text-[11px] font-semibold text-[#EF4444] transition-colors hover:bg-[#EF4444]/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <ShieldAlert className="h-3.5 w-3.5" />
              DDoS Key Spike
            </button>

            <button
              type="button"
              disabled={burstPending !== null}
              onClick={onSimulateOverload}
              className="inline-flex items-center gap-1.5 rounded border border-[#F59E0B]/40 bg-[#F59E0B]/10 px-2.5 py-1.5 text-[11px] font-semibold text-[#F59E0B] transition-colors hover:bg-[#F59E0B]/20 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Flame className="h-3.5 w-3.5" />
              Cascade Overload (503 Shed)
            </button>
          </div>
        </div>
      </div>
    </header>
  );
};
