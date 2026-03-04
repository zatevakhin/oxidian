import { useCallback, useState } from "react";
import type { GraphSettings, LayoutMode } from "../types";

const PREFIX = "oxidian.";

function load<T>(key: string, fallback: T, parse: (v: string) => T): T {
  const stored = localStorage.getItem(PREFIX + key);
  if (stored === null) return fallback;
  try {
    return parse(stored);
  } catch {
    return fallback;
  }
}

function save(key: string, value: string) {
  localStorage.setItem(PREFIX + key, value);
}

function loadDefaults(): GraphSettings {
  return {
    layout: load<LayoutMode>("layout", "static", (v) => v as LayoutMode),
    hideLabels: load("hideLabels", false, (v) => v === "true"),
    showTags: load("showTags", true, (v) => v === "true"),
    forceAuto: load("forceAuto", false, (v) => v === "true"),
    clusterEnabled: load("clusterEnabled", false, (v) => v === "true"),
    minScore: load("minScore", 0.6, parseFloat),
    topK: load("topK", 8, (v) => parseInt(v, 10)),
  };
}

export function useGraphSettings() {
  const [settings, setSettings] = useState<GraphSettings>(loadDefaults);

  const update = useCallback(
    <K extends keyof GraphSettings>(key: K, value: GraphSettings[K]) => {
      setSettings((prev) => {
        const next = { ...prev, [key]: value };
        save(key, String(value));
        return next;
      });
    },
    [],
  );

  return { settings, update } as const;
}
