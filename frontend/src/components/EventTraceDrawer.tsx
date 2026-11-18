import React from "react";
import { X } from "lucide-react";
import { DecisionEvent } from "../types/telemetry";

interface EventTraceDrawerProps {
  event: DecisionEvent | null;
  onClose: () => void;
}

export const EventTraceDrawer: React.FC<EventTraceDrawerProps> = ({ event, onClose }) => {
  if (!event) return null;

  const httpStatus =
    event.decision === "SHED_OVERLOAD"
      ? 503
      : event.decision === "THROTTLED"
        ? 429
        : 200;

  const headers: Record<string, string> = {
    "X-RateLimit-Limit": String(event.limit),
    "X-RateLimit-Remaining": String(event.remaining),
    "X-RateLimit-Reset": event.decision === "SHED_OVERLOAD" ? "5" : "1",
    "X-Vulcan-Node": event.node_id,
    "X-Vulcan-Priority": event.priority,
    "X-Vulcan-Load-Shed":
      event.load_shed_active || event.decision === "SHED_OVERLOAD" ? "active" : "nominal"
  };

  if (event.decision !== "ALLOWED") {
    headers["Retry-After"] = event.decision === "SHED_OVERLOAD" ? "5" : "1";
  }

  return (
    <div
      className="fixed inset-0 z-50 flex justify-end bg-black/60 backdrop-blur-[1px]"
      onClick={onClose}
    >
      <aside
        className="flex h-full w-full max-w-[440px] flex-col border-l border-white/[0.1] bg-[#0C0C0E] text-[#FAFAFA]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-white/[0.07] px-4 py-3">
          <div>
            <div className="font-mono text-xs font-semibold tracking-wider text-[#FAFAFA]">
              EVENT TRACE INSPECTOR // #{event.id}
            </div>
            <div className="mt-0.5 font-mono text-[10px] text-[#52525B]">
              {new Date(event.timestamp_ms).toISOString()}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-white/[0.08] bg-[#09090B] p-1 text-[#A1A1AA] hover:text-[#FAFAFA]"
            aria-label="Close trace inspector"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 space-y-4 overflow-y-auto p-4 font-mono text-xs tabular-nums">
          {/* Status & Decision Banner */}
          <div className="flex items-center justify-between rounded border border-white/[0.07] bg-[#09090B] px-3 py-2.5">
            <div>
              <div className="text-[10px] uppercase text-[#52525B]">HTTP DISPOSITION</div>
              <div className="mt-0.5 text-sm font-semibold">
                HTTP {httpStatus} — {event.decision}
              </div>
            </div>
            <span
              className={`rounded border px-2 py-0.5 text-[10px] font-semibold ${
                event.decision === "ALLOWED"
                  ? "border-[#10B981]/40 bg-[#10B981]/10 text-[#10B981]"
                  : event.decision === "THROTTLED"
                    ? "border-[#EF4444]/40 bg-[#EF4444]/10 text-[#EF4444]"
                    : "border-[#F59E0B]/40 bg-[#F59E0B]/10 text-[#F59E0B]"
              }`}
            >
              {event.eval_time_us.toFixed(2)} µs
            </span>
          </div>

          {/* Core Routing & Quota Metadata */}
          <div className="rounded border border-white/[0.07] bg-[#09090B]">
            <div className="border-b border-white/[0.07] px-3 py-1.5 text-[10px] uppercase tracking-wider text-[#52525B]">
              ROUTING &amp; EVALUATION TRACE
            </div>
            <div className="divide-y divide-white/[0.05] text-[11px]">
              {[
                { label: "Client Key", value: event.client_key },
                { label: "Namespace", value: event.namespace },
                { label: "FNV-1a Hash Hex", value: `0x${event.node_hash}` },
                { label: "Virtual Ring Node", value: event.node_id },
                { label: "Priority Tier", value: event.priority.toUpperCase() },
                {
                  label: "Algorithm / Policy",
                  value: `${event.algorithm} (${event.mode})`
                },
                {
                  label: "Remaining Tokens",
                  value: `${event.remaining} / ${event.limit}`
                },
                { label: "Eval Time (µs)", value: `${event.eval_time_us.toFixed(2)} µs` }
              ].map((item) => (
                <div key={item.label} className="flex items-center justify-between gap-3 px-3 py-2">
                  <span className="text-[#A1A1AA]">{item.label}</span>
                  <span className="truncate font-medium text-[#FAFAFA]">{item.value}</span>
                </div>
              ))}
            </div>
          </div>

          {/* HTTP Response Headers */}
          <div className="rounded border border-white/[0.07] bg-[#09090B]">
            <div className="border-b border-white/[0.07] px-3 py-1.5 text-[10px] uppercase tracking-wider text-[#52525B]">
              FULL HTTP RESPONSE HEADERS
            </div>
            <div className="divide-y divide-white/[0.05] text-[11px]">
              {Object.entries(headers).map(([k, v]) => (
                <div key={k} className="flex items-center justify-between gap-2 px-3 py-1.5">
                  <span className="text-[#A1A1AA]">{k}:</span>
                  <span
                    className={`font-semibold ${
                      k === "X-Vulcan-Load-Shed" && v === "active"
                        ? "text-[#F59E0B]"
                        : "text-[#FAFAFA]"
                    }`}
                  >
                    {v}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Raw Event Payload */}
          <div className="rounded border border-white/[0.07] bg-[#09090B]">
            <div className="border-b border-white/[0.07] px-3 py-1.5 text-[10px] uppercase tracking-wider text-[#52525B]">
              RAW EVENT RECORD
            </div>
            <pre className="overflow-x-auto p-3 text-[10px] leading-relaxed text-[#A1A1AA]">
              {JSON.stringify(event, null, 2)}
            </pre>
          </div>
        </div>
      </aside>
    </div>
  );
};
