import * as Switch from "@radix-ui/react-switch";
import * as Slider from "@radix-ui/react-slider";
import type { GraphSettings, LayoutMode } from "../types";

interface SettingsPanelProps {
  similarityAvailable: boolean;
  settings: GraphSettings;
  onSettingChange: <K extends keyof GraphSettings>(
    key: K,
    value: GraphSettings[K],
  ) => void;
  onSimilarityChange: () => void;
}

const LAYOUTS: { value: LayoutMode; label: string }[] = [
  { value: "static", label: "Static" },
  { value: "circle", label: "Circle" },
  { value: "random", label: "Random" },
  { value: "forceatlas2", label: "ForceAtlas2" },
  { value: "forceatlas2-cluster", label: "FA2 Clustered" },
];

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      <span className="text-xs text-text-secondary">{label}</span>
      <Switch.Root
        checked={checked}
        onCheckedChange={onChange}
        className="w-8 h-[18px] rounded-full bg-surface-hover border border-border-subtle data-[state=checked]:bg-accent/30 data-[state=checked]:border-accent/50 transition-colors cursor-pointer"
      >
        <Switch.Thumb className="block w-3.5 h-3.5 rounded-full bg-text-muted data-[state=checked]:bg-accent translate-x-px data-[state=checked]:translate-x-[14px] transition-all" />
      </Switch.Root>
    </div>
  );
}

function SliderControl({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-xs text-text-secondary">{label}</span>
        <span className="text-xs font-mono text-text-primary bg-surface-hover px-1.5 py-0.5 rounded border border-border-subtle min-w-[2.5rem] text-right">
          {step < 1 ? value.toFixed(2) : value}
        </span>
      </div>
      <Slider.Root
        value={[value]}
        min={min}
        max={max}
        step={step}
        onValueChange={([v]) => onChange(v)}
        className="relative flex items-center w-full h-4 cursor-pointer"
      >
        <Slider.Track className="relative h-1 w-full rounded-full bg-surface-hover border border-border-subtle overflow-hidden">
          <Slider.Range className="absolute h-full rounded-full bg-accent/50" />
        </Slider.Track>
        <Slider.Thumb className="block w-3.5 h-3.5 rounded-full bg-accent border-2 border-surface-alt shadow-sm hover:bg-accent-hover focus:outline-none focus:ring-2 focus:ring-accent/30 transition-colors" />
      </Slider.Root>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-[10px] font-semibold uppercase tracking-wider text-text-muted pt-2 pb-1">
      {children}
    </div>
  );
}

export function SettingsPanel({
  similarityAvailable,
  settings,
  onSettingChange,
  onSimilarityChange,
}: SettingsPanelProps) {
  const isForceLayout =
    settings.layout === "forceatlas2" ||
    settings.layout === "forceatlas2-cluster";

  return (
    <>
      <SectionLabel>Layout</SectionLabel>

      <select
        value={settings.layout}
        onChange={(e) =>
          onSettingChange("layout", e.target.value as LayoutMode)
        }
        className="w-full bg-surface-hover text-text-primary text-xs px-2.5 py-1.5 rounded-lg border border-border-subtle outline-none focus:border-accent cursor-pointer transition-colors"
      >
        {LAYOUTS.map((l) => (
          <option key={l.value} value={l.value}>
            {l.label}
          </option>
        ))}
      </select>

      {isForceLayout && (
        <Toggle
          label="Auto-run"
          checked={settings.forceAuto}
          onChange={(v) => onSettingChange("forceAuto", v)}
        />
      )}

      <SectionLabel>Display</SectionLabel>

      <Toggle
        label="Hide labels"
        checked={settings.hideLabels}
        onChange={(v) => onSettingChange("hideLabels", v)}
      />

      <Toggle
        label="Show tags"
        checked={settings.showTags}
        onChange={(v) => onSettingChange("showTags", v)}
      />

      {similarityAvailable && (
        <>
          <SectionLabel>Similarity</SectionLabel>

          <Toggle
            label="Clusters"
            checked={settings.clusterEnabled}
            onChange={(v) => {
              onSettingChange("clusterEnabled", v);
              onSimilarityChange();
            }}
          />

          <SliderControl
            label="Min score"
            value={settings.minScore}
            min={0}
            max={1}
            step={0.05}
            onChange={(v) => {
              onSettingChange("minScore", v);
              onSimilarityChange();
            }}
          />

          <SliderControl
            label="Top K"
            value={settings.topK}
            min={1}
            max={20}
            step={1}
            onChange={(v) => {
              onSettingChange("topK", v);
              onSimilarityChange();
            }}
          />
        </>
      )}
    </>
  );
}
