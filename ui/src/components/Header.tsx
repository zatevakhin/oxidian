import * as Popover from "@radix-ui/react-popover";
import { Settings } from "lucide-react";
import type { ConnectionStatus, GraphSettings } from "../types";
import { StatusPill } from "./StatusPill";
import { SettingsPanel } from "./SettingsPanel";

interface HeaderProps {
  status: ConnectionStatus;
  nodeCount: number;
  edgeCount: number;
  similarityAvailable: boolean;
  settings: GraphSettings;
  onSettingChange: <K extends keyof GraphSettings>(
    key: K,
    value: GraphSettings[K],
  ) => void;
  onSimilarityChange: () => void;
}

export function Header({
  status,
  nodeCount,
  edgeCount,
  similarityAvailable,
  settings,
  onSettingChange,
  onSimilarityChange,
}: HeaderProps) {
  return (
    <header className="relative flex items-center gap-2 px-3 py-2 bg-surface-alt/80 backdrop-blur-sm border-b border-border">
      <Popover.Root>
        <Popover.Trigger asChild>
          <button
            className="p-1.5 rounded-lg border bg-surface-alt border-border-subtle text-text-muted hover:text-text-secondary hover:bg-surface-hover data-[state=open]:bg-accent/15 data-[state=open]:border-accent/40 data-[state=open]:text-accent transition-colors"
            title="Settings"
          >
            <Settings size={16} />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content
            align="start"
            sideOffset={8}
            className="z-50 w-56 rounded-xl border border-border bg-surface-alt shadow-lg shadow-black/40 p-3 flex flex-col gap-2"
            onOpenAutoFocus={(e) => e.preventDefault()}
          >
            <SettingsPanel
              similarityAvailable={similarityAvailable}
              settings={settings}
              onSettingChange={onSettingChange}
              onSimilarityChange={onSimilarityChange}
            />
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>

      <StatusPill status={status} />

      <span
        title={`${nodeCount} nodes \u00b7 ${edgeCount} edges`}
        className="px-3 py-1.5 rounded-lg text-xs text-text-muted bg-surface-alt border border-border-subtle font-mono cursor-default"
      >
        {nodeCount}n / {edgeCount}e
      </span>
    </header>
  );
}
