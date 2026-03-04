import { useCallback, useEffect, useRef } from "react";
import { useGraphWs } from "./hooks/useGraphWs";
import { useGraphSettings } from "./hooks/useGraphSettings";
import { GraphView } from "./components/GraphView";

export function App() {
  const { status, payload, sendSimilaritySettings } = useGraphWs();
  const { settings, update } = useGraphSettings();
  const initialSent = useRef(false);

  // send similarity settings on connect
  useEffect(() => {
    if (status === "connected" && !initialSent.current) {
      sendSimilaritySettings(
        settings.clusterEnabled,
        settings.minScore,
        settings.topK,
      );
      initialSent.current = true;
    }
    if (status === "disconnected") {
      initialSent.current = false;
    }
  }, [status, settings.clusterEnabled, settings.minScore, settings.topK, sendSimilaritySettings]);

  const handleSimilarityChange = useCallback(() => {
    // use a microtask so the settings state has flushed
    queueMicrotask(() => {
      const stored = {
        enabled: localStorage.getItem("oxidian.clusterEnabled") === "true",
        minScore: parseFloat(localStorage.getItem("oxidian.minScore") ?? "0.6"),
        topK: parseInt(localStorage.getItem("oxidian.topK") ?? "8", 10),
      };
      sendSimilaritySettings(stored.enabled, stored.minScore, stored.topK);
    });
  }, [sendSimilaritySettings]);

  return (
    <GraphView
      status={status}
      payload={payload}
      settings={settings}
      onSettingChange={update}
      onSimilarityChange={handleSimilarityChange}
    />
  );
}
