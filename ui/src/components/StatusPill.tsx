import type { ConnectionStatus } from "../types";

const textColor: Record<ConnectionStatus, string> = {
  connecting: "text-status-warn",
  connected: "text-status-ok",
  disconnected: "text-status-warn",
};

const dots: Record<ConnectionStatus, string> = {
  connecting: "bg-status-warn animate-pulse",
  connected: "bg-status-ok",
  disconnected: "bg-status-warn",
};

export function StatusPill({ status }: { status: ConnectionStatus }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-surface-alt border border-border-subtle text-xs font-medium transition-colors ${textColor[status]}`}
    >
      <span className={`w-1.5 h-1.5 rounded-full ${dots[status]}`} />
      {status}
    </span>
  );
}
