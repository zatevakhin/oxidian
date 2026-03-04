import type { GraphPayload, GraphSettings, ConnectionStatus } from "../types";
import { Header } from "./Header";
import { SigmaCanvas } from "./SigmaCanvas";

interface GraphViewProps {
  status: ConnectionStatus;
  payload: GraphPayload | null;
  settings: GraphSettings;
  onSettingChange: <K extends keyof GraphSettings>(
    key: K,
    value: GraphSettings[K],
  ) => void;
  onSimilarityChange: () => void;
}

export function GraphView({
  status,
  payload,
  settings,
  onSettingChange,
  onSimilarityChange,
}: GraphViewProps) {
  const nodeCount = payload?.nodes.length ?? 0;
  const edgeCount = payload?.edges.length ?? 0;
  const similarityAvailable = payload?.similarity?.available ?? false;

  return (
    <div className="flex flex-col h-screen">
      <Header
        status={status}
        nodeCount={nodeCount}
        edgeCount={edgeCount}
        similarityAvailable={similarityAvailable}
        settings={settings}
        onSettingChange={onSettingChange}
        onSimilarityChange={onSimilarityChange}
      />
      <SigmaCanvas payload={payload} settings={settings} />
    </div>
  );
}
