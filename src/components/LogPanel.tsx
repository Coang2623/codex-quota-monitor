import { useEffect, useMemo, useRef } from "react";
import type { AppLogEntry } from "../types";

interface LogPanelProps {
  entries: AppLogEntry[];
  onClear: () => void;
}

function formatTime(timestampMs: number): string {
  return new Intl.DateTimeFormat("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(new Date(timestampMs));
}

function levelClasses(level: string): string {
  switch (level) {
    case "error":
      return "bg-red-500/15 text-red-200 border-red-500/30";
    case "warn":
      return "bg-amber-500/15 text-amber-200 border-amber-500/30";
    case "success":
      return "bg-emerald-500/15 text-emerald-200 border-emerald-500/30";
    default:
      return "bg-sky-500/15 text-sky-200 border-sky-500/30";
  }
}

export function LogPanel({ entries, onClear }: LogPanelProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    containerRef.current.scrollTop = containerRef.current.scrollHeight;
  }, [entries]);

  const groupedEntries = useMemo(() => entries.slice(-250), [entries]);

  return (
    <aside className="hidden xl:flex xl:flex-col xl:sticky xl:top-0 xl:h-screen border-l border-white/10 bg-[#0d1117] text-slate-100">
      <div className="flex items-center justify-between gap-3 px-4 py-4 border-b border-white/10">
        <div className="min-w-0">
          <h2 className="text-sm font-semibold tracking-wide text-white">Runtime Log</h2>
          <p className="text-[11px] text-slate-400 truncate">
            Live events from frontend and backend
          </p>
        </div>
        <button
          onClick={onClear}
          className="px-2.5 py-1.5 text-[11px] font-medium rounded-lg bg-white/5 hover:bg-white/10 text-slate-200 transition-colors"
        >
          Clear
        </button>
      </div>

      <div ref={containerRef} className="flex-1 overflow-y-auto px-3 py-3 space-y-2">
        {groupedEntries.length === 0 ? (
          <div className="rounded-xl border border-dashed border-white/10 px-4 py-6 text-center text-xs text-slate-500">
            No logs yet
          </div>
        ) : (
          groupedEntries.map((entry) => (
            <div
              key={`${entry.source ?? "unknown"}-${entry.id}`}
              className="rounded-xl border border-white/8 bg-white/4 px-3 py-2 shadow-[0_0_0_1px_rgba(255,255,255,0.02)]"
            >
              <div className="flex items-center gap-2 text-[10px] uppercase tracking-[0.14em]">
                <span
                  className={`inline-flex items-center rounded-full border px-2 py-0.5 font-semibold ${levelClasses(entry.level)}`}
                >
                  {entry.level}
                </span>
                <span className="text-slate-500">{formatTime(entry.timestamp_ms)}</span>
                <span className="ml-auto text-slate-500">{entry.scope}</span>
              </div>
              <p className="mt-2 text-xs leading-5 text-slate-100 break-words">
                {entry.message}
              </p>
            </div>
          ))
        )}
      </div>
    </aside>
  );
}
