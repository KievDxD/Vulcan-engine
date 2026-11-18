import React from "react";
import { Power, Server } from "lucide-react";
import { ClusterTopologySnapshot } from "../types/telemetry";

interface ConsistentHashRingPanelProps {
  topology: ClusterTopologySnapshot;
  togglingNode: string | null;
  toggleError: string | null;
  onToggleNode: (nodeId: string) => void;
}

const NODE_STROKE_COLOR: Record<string, string> = {
  "edge-iad-01": "#10B981",
  "edge-gru-02": "#E4E4E7",
  "edge-bog-03": "#A1A1AA",
  "edge-fra-04": "#F59E0B"
};

function polarToCartesian(cx: number, cy: number, r: number, angleDeg: number) {
  const rad = ((angleDeg - 90) * Math.PI) / 180;
  return {
    x: cx + r * Math.cos(rad),
    y: cy + r * Math.sin(rad)
  };
}

export const ConsistentHashRingPanel: React.FC<ConsistentHashRingPanelProps> = ({
  topology,
  togglingNode,
  toggleError,
  onToggleNode
}) => {
  const cx = 160;
  const cy = 114;
  const ringR = 76;

  const totalRouted = Math.max(
    1,
    topology.nodes.reduce((sum, n) => sum + n.routed_requests, 0)
  );

  const sampledSegments =
    topology.segments.length > 0
      ? topology.segments.filter((_, idx) => idx % 2 === 0)
      : Array.from({ length: 64 }, (_, i) => ({
          node_id: topology.nodes[i % topology.nodes.length]?.node_id ?? "edge-iad-01",
          start_deg: (i / 64) * 360,
          end_deg: ((i + 1) / 64) * 360,
          hash_hex: ""
        }));

  return (
    <section className="flex flex-col justify-between rounded-md border border-white/[0.07] bg-[#0C0C0E]">
      <div className="flex items-center justify-between gap-2 border-b border-white/[0.07] px-4 py-3">
        <div>
          <div className="flex items-center gap-2">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-[#FAFAFA]">
              CONSISTENT HASHING RING
            </h2>
            <span className="rounded border border-white/[0.07] bg-[#09090B] px-1.5 py-0.5 font-mono text-[10px] text-[#A1A1AA]">
              {topology.hash_algorithm}
            </span>
          </div>
          <p className="mt-0.5 text-xs text-[#52525B]">
            {topology.online_nodes}/{topology.total_nodes} nodes online • {topology.total_vnodes} active vnodes
          </p>
        </div>

        {topology.last_toggled_node && (
          <div className="rounded border border-[#F59E0B]/30 bg-[#F59E0B]/10 px-2 py-1 text-right font-mono text-[10px] tabular-nums">
            <div className="text-[#F59E0B]">
              REASSIGNED: {topology.last_rebalance_pct.toFixed(2)}%
            </div>
            <div className="text-[#A1A1AA]">{topology.last_toggled_node}</div>
          </div>
        )}
      </div>

      <div className="flex flex-col items-center px-4 pt-4 pb-2">
        <svg viewBox="0 0 320 228" className="h-[224px] w-full max-w-[340px] select-none">
          <circle
            cx={cx}
            cy={cy}
            r={ringR}
            fill="none"
            stroke="rgba(255,255,255,0.06)"
            strokeWidth="10"
          />

          {sampledSegments.map((seg, i) => {
            const p1 = polarToCartesian(cx, cy, ringR - 5, seg.start_deg);
            const p2 = polarToCartesian(cx, cy, ringR + 5, seg.start_deg);
            const nodeObj = topology.nodes.find((n) => n.node_id === seg.node_id);
            const stroke =
              nodeObj && !nodeObj.online
                ? "#EF4444"
                : NODE_STROKE_COLOR[seg.node_id] ?? "#A1A1AA";
            return (
              <line
                key={i}
                x1={p1.x}
                y1={p1.y}
                x2={p2.x}
                y2={p2.y}
                stroke={stroke}
                strokeOpacity={0.75}
                strokeWidth="1.5"
              />
            );
          })}

          <circle
            cx={cx}
            cy={cy}
            r={ringR - 14}
            fill="none"
            stroke="rgba(255,255,255,0.06)"
            strokeDasharray="2 3"
            strokeWidth="1"
          />

          <text
            x={cx}
            y={cy - 10}
            textAnchor="middle"
            className="fill-[#52525B] font-mono text-[9px] uppercase tracking-wider"
          >
            RING QUORUM
          </text>
          <text
            x={cx}
            y={cy + 8}
            textAnchor="middle"
            className="fill-[#FAFAFA] font-mono text-base font-semibold tabular-nums"
          >
            {topology.online_nodes} / {topology.total_nodes} ONLINE
          </text>
          <text
            x={cx}
            y={cy + 23}
            textAnchor="middle"
            className="fill-[#A1A1AA] font-mono text-[10px] tabular-nums"
          >
            {topology.virtual_replicas_per_node} VNODES / NODE
          </text>

          {topology.nodes.map((node) => {
            const anchor = polarToCartesian(cx, cy, ringR + 18, node.primary_angle_deg);
            const isLeft = anchor.x < cx;
            const labelX = isLeft ? anchor.x - 6 : anchor.x + 6;
            const ringEdge = polarToCartesian(cx, cy, ringR + 5, node.primary_angle_deg);
            const statusColor = node.online ? "#10B981" : "#EF4444";

            return (
              <g
                key={node.node_id}
                className="cursor-pointer"
                onClick={() => onToggleNode(node.node_id)}
              >
                <line
                  x1={ringEdge.x}
                  y1={ringEdge.y}
                  x2={anchor.x}
                  y2={anchor.y}
                  stroke={statusColor}
                  strokeOpacity={0.55}
                  strokeWidth="1"
                />
                <circle
                  cx={anchor.x}
                  cy={anchor.y}
                  r="4.5"
                  fill="#09090B"
                  stroke={statusColor}
                  strokeWidth="2"
                />
                <text
                  x={labelX}
                  y={anchor.y - 2}
                  textAnchor={isLeft ? "end" : "start"}
                  className={`font-mono text-[10px] font-semibold ${
                    node.online ? "fill-[#FAFAFA]" : "fill-[#EF4444]"
                  }`}
                >
                  {node.node_id}
                </text>
                <text
                  x={labelX}
                  y={anchor.y + 9}
                  textAnchor={isLeft ? "end" : "start"}
                  className="fill-[#52525B] font-mono text-[9px] tabular-nums"
                >
                  {node.online ? `${node.key_share_pct.toFixed(1)}% arc` : "OFFLINE"}
                </text>
              </g>
            );
          })}
        </svg>

        {toggleError && (
          <div className="mb-2 w-full rounded border border-[#EF4444]/40 bg-[#EF4444]/10 px-2.5 py-1.5 font-mono text-[11px] text-[#EF4444]">
            {toggleError}
          </div>
        )}
      </div>

      <div className="divide-y divide-white/[0.07] border-t border-white/[0.07]">
        {topology.nodes.map((node) => {
          const shareOfTraffic = ((node.routed_requests / totalRouted) * 100).toFixed(1);
          const isToggling = togglingNode === node.node_id;

          return (
            <div
              key={node.node_id}
              className="flex items-center justify-between gap-2 px-4 py-2.5 text-xs"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <Server
                    className={`h-3.5 w-3.5 shrink-0 ${
                      node.online ? "text-[#10B981]" : "text-[#EF4444]"
                    }`}
                  />
                  <span className="font-mono font-medium text-[#FAFAFA]">{node.node_id}</span>
                  <span className="font-mono text-[10px] text-[#52525B]">{node.region}</span>
                </div>
                <div className="mt-1 flex items-center gap-2">
                  <div className="h-1 flex-1 overflow-hidden rounded-sm bg-white/[0.06]">
                    <div
                      className={`h-full transition-all ${
                        node.online ? "bg-[#10B981]" : "bg-[#EF4444]/40"
                      }`}
                      style={{ width: `${Math.min(100, node.key_share_pct)}%` }}
                    />
                  </div>
                  <span className="font-mono text-[10px] tabular-nums text-[#A1A1AA]">
                    {node.routed_requests.toLocaleString()} req ({shareOfTraffic}%)
                  </span>
                </div>
              </div>

              <button
                type="button"
                disabled={isToggling}
                onClick={() => onToggleNode(node.node_id)}
                className={`inline-flex shrink-0 items-center gap-1 rounded border px-2 py-1 font-mono text-[10px] font-medium transition-colors disabled:opacity-50 ${
                  node.online
                    ? "border-white/[0.12] bg-[#09090B] text-[#A1A1AA] hover:border-[#EF4444]/40 hover:text-[#EF4444]"
                    : "border-[#10B981]/40 bg-[#10B981]/10 text-[#10B981] hover:bg-[#10B981]/20"
                }`}
              >
                <Power className="h-3 w-3" />
                {node.online ? "DROP NODE" : "RECOVER"}
              </button>
            </div>
          );
        })}
      </div>
    </section>
  );
};
