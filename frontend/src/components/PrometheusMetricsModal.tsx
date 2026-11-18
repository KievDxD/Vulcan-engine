import React, { useEffect, useState } from "react";
import { Check, Copy, RefreshCw, X } from "lucide-react";
import { fetchPrometheusMetrics } from "../lib/api";

interface PrometheusMetricsModalProps {
  open: boolean;
  onClose: () => void;
}

export const PrometheusMetricsModal: React.FC<PrometheusMetricsModalProps> = ({
  open,
  onClose
}) => {
  const [rawText, setRawText] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);

  const loadMetrics = async () => {
    setLoading(true);
    try {
      const txt = await fetchPrometheusMetrics();
      setRawText(txt);
    } catch (err) {
      setRawText(
        `# Failed to fetch /metrics: ${err instanceof Error ? err.message : "unknown error"}`
      );
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (open) {
      void loadMetrics();
    }
  }, [open]);

  if (!open) return null;

  const handleCopy = () => {
    void navigator.clipboard.writeText(rawText).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-[1px]"
      onClick={onClose}
    >
      <div
        className="flex max-h-[85vh] w-full max-w-3xl flex-col rounded-md border border-white/[0.12] bg-[#0C0C0E] text-[#FAFAFA]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-white/[0.07] px-4 py-3">
          <div>
            <div className="font-mono text-xs font-semibold tracking-wider">
              OPENMETRICS // GET /metrics
            </div>
            <div className="font-mono text-[10px] text-[#52525B]">
              Content-Type: text/plain; version=0.0.4; charset=utf-8
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => void loadMetrics()}
              disabled={loading}
              className="inline-flex items-center gap-1 rounded border border-white/[0.1] bg-[#09090B] px-2.5 py-1 font-mono text-[10px] text-[#A1A1AA] hover:text-[#FAFAFA]"
            >
              <RefreshCw className={`h-3 w-3 ${loading ? "animate-spin" : ""}`} />
              Refresh
            </button>
            <button
              type="button"
              onClick={handleCopy}
              className="inline-flex items-center gap-1 rounded border border-white/[0.1] bg-[#09090B] px-2.5 py-1 font-mono text-[10px] text-[#A1A1AA] hover:text-[#FAFAFA]"
            >
              {copied ? <Check className="h-3 w-3 text-[#10B981]" /> : <Copy className="h-3 w-3" />}
              {copied ? "Copied" : "Copy"}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-white/[0.1] bg-[#09090B] p-1 text-[#A1A1AA] hover:text-[#FAFAFA]"
              aria-label="Close metrics modal"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        <pre className="flex-1 overflow-auto bg-[#09090B] p-4 font-mono text-[11px] leading-relaxed text-[#FAFAFA]">
          {rawText || "# Loading Prometheus exposition..."}
        </pre>
      </div>
    </div>
  );
};
