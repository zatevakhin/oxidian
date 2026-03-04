import { useEffect, useRef, useCallback } from "react";
import Graph from "graphology";
import Sigma from "sigma";
import forceAtlas2 from "graphology-layout-forceatlas2";
import type { GraphPayload, GraphNode, GraphSettings, LayoutMode } from "../types";

/* ── colour helpers ─────────────────────────────────────────────────── */

const NODE_COLORS: Record<string, string> = {
  markdown: "#6ea4f4",
  canvas: "#f4c06e",
  attachment: "#9ba3b5",
  other: "#717a90",
  tag: "#6ee7a0",
};

const HIGHLIGHT = {
  node: "#f4a56e",
  label: "#e8ecf4",
  labelBg: "#0b0e14",
  outline: "#1e2230",
  edge: "#6ea4f4",
  dim: "#262b38",
};

function hslToHex(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0, g = 0, b = 0;
  if (h < 60)      { r = c; g = x; }
  else if (h < 120) { r = x; g = c; }
  else if (h < 180) { g = c; b = x; }
  else if (h < 240) { g = x; b = c; }
  else if (h < 300) { r = x; b = c; }
  else              { r = c; b = x; }
  const hex = (v: number) =>
    Math.round((v + m) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${hex(r)}${hex(g)}${hex(b)}`;
}

function clusterColor(clusterId: number): string {
  return hslToHex((clusterId * 47) % 360, 0.62, 0.62);
}

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) {
    hash = (hash * 31 + value.charCodeAt(i)) | 0;
  }
  return hash;
}

function positionFor(id: string) {
  const hash = hashString(id);
  const angle = ((hash % 360) * Math.PI) / 180;
  const radius = 2 + (Math.abs(hash) % 100) / 50;
  return { x: Math.cos(angle) * radius, y: Math.sin(angle) * radius };
}

function drawRoundedRect(
  ctx: CanvasRenderingContext2D,
  x: number, y: number, w: number, h: number, r: number,
) {
  const cr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + cr, y);
  ctx.lineTo(x + w - cr, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + cr);
  ctx.lineTo(x + w, y + h - cr);
  ctx.quadraticCurveTo(x + w, y + h, x + w - cr, y + h);
  ctx.lineTo(x + cr, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - cr);
  ctx.lineTo(x, y + cr);
  ctx.quadraticCurveTo(x, y, x + cr, y);
  ctx.closePath();
}

/* ── filtering ──────────────────────────────────────────────────────── */

function filterPayload(
  payload: GraphPayload,
  showTags: boolean,
): GraphPayload {
  if (showTags) return payload;
  const nodes = payload.nodes.filter((n) => n.kind !== "tag");
  const ids = new Set(nodes.map((n) => n.id));
  const edges = payload.edges.filter(
    (e) => ids.has(e.source) && ids.has(e.target),
  );
  return { nodes, edges, similarity: payload.similarity };
}

/* ── component ──────────────────────────────────────────────────────── */

interface SigmaCanvasProps {
  payload: GraphPayload | null;
  settings: GraphSettings;
}

export function SigmaCanvas({ payload, settings }: SigmaCanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const rendererRef = useRef<Sigma | null>(null);
  const graphRef = useRef(new Graph());
  const positionsRef = useRef(new Map<string, { x: number; y: number }>());
  const hoveredRef = useRef<string | null>(null);
  const prevLayoutRef = useRef<LayoutMode>(settings.layout);

  /* ── node colour picker ─────────────────────────────────────────── */

  const effectiveNodeColor = useCallback(
    (node: GraphNode): string => {
      if (
        settings.clusterEnabled &&
        node.cluster_id != null &&
        node.kind !== "tag"
      ) {
        return clusterColor(node.cluster_id);
      }
      return NODE_COLORS[node.kind] ?? NODE_COLORS.other;
    },
    [settings.clusterEnabled],
  );

  /* ── apply layout ───────────────────────────────────────────────── */

  const applyLayout = useCallback(
    (layout: LayoutMode) => {
      const graph = graphRef.current;
      const renderer = rendererRef.current;
      const positions = positionsRef.current;
      if (!renderer) return;
      const ids = graph.nodes();
      if (!ids.length) return;

      if (layout === "random") {
        ids.forEach((id) => {
          const x = Math.random() * 4 - 2;
          const y = Math.random() * 4 - 2;
          graph.setNodeAttribute(id, "x", x);
          graph.setNodeAttribute(id, "y", y);
          positions.set(id, { x, y });
        });
        renderer.refresh();
        return;
      }

      if (layout === "circle") {
        const radius = Math.max(2.5, ids.length / 12);
        ids.forEach((id, index) => {
          const angle = (index / ids.length) * Math.PI * 2;
          const x = Math.cos(angle) * radius;
          const y = Math.sin(angle) * radius;
          graph.setNodeAttribute(id, "x", x);
          graph.setNodeAttribute(id, "y", y);
          positions.set(id, { x, y });
        });
        renderer.refresh();
        return;
      }

      if (layout === "forceatlas2" || layout === "forceatlas2-cluster") {
        // seed if all zeros
        let hasNonZero = false;
        for (const id of ids) {
          const x = graph.getNodeAttribute(id, "x") as number;
          const y = graph.getNodeAttribute(id, "y") as number;
          if (typeof x !== "number" || typeof y !== "number") break;
          if (x !== 0 || y !== 0) { hasNonZero = true; break; }
        }
        if (!hasNonZero) {
          ids.forEach((id) => {
            const x = Math.random() * 4 - 2;
            const y = Math.random() * 4 - 2;
            graph.setNodeAttribute(id, "x", x);
            graph.setNodeAttribute(id, "y", y);
            positions.set(id, { x, y });
          });
        }

        // cluster edges
        const tempEdges: string[] = [];
        if (layout === "forceatlas2-cluster") {
          const clusters = new Map<number, string[]>();
          graph.forEachNode((id, attrs) => {
            const cid = attrs.clusterId as number | undefined;
            const kind = attrs.nodeKind as string;
            if (cid == null || kind === "tag") return;
            if (!clusters.has(cid)) clusters.set(cid, []);
            clusters.get(cid)!.push(id);
          });
          clusters.forEach((nodeIds, cid) => {
            if (nodeIds.length < 2) return;
            nodeIds.sort();
            for (let i = 0; i < nodeIds.length - 1; i++) {
              const key = `cluster:${cid}:${i}`;
              if (graph.hasEdge(key) || graph.hasEdge(nodeIds[i], nodeIds[i + 1]))
                continue;
              graph.addEdgeWithKey(key, nodeIds[i], nodeIds[i + 1], {
                weight: 4,
              });
              tempEdges.push(key);
            }
          });
        }

        const fa2Settings = forceAtlas2.inferSettings(graph);
        if (layout === "forceatlas2-cluster") {
          fa2Settings.gravity = Math.max(0.1, (fa2Settings.gravity ?? 1) * 0.7);
          fa2Settings.scalingRatio = Math.max(
            2,
            (fa2Settings.scalingRatio ?? 2) * 1.1,
          );
        }

        forceAtlas2.assign(graph, {
          iterations: layout === "forceatlas2-cluster" ? 90 : 80,
          settings: fa2Settings,
          getEdgeWeight: (_edge, attr) =>
            (attr.weight as number | undefined) ?? 1,
        });

        // cleanup temp edges
        tempEdges.forEach((key) => {
          if (graph.hasEdge(key)) graph.dropEdge(key);
        });

        // sync positions
        graph.forEachNode((id) => {
          positions.set(id, {
            x: graph.getNodeAttribute(id, "x") as number,
            y: graph.getNodeAttribute(id, "y") as number,
          });
        });

        renderer.refresh();
      }
    },
    [],
  );

  /* ── apply graph data ───────────────────────────────────────────── */

  useEffect(() => {
    if (!payload || !containerRef.current) return;

    const filtered = filterPayload(payload, settings.showTags);
    const graph = graphRef.current;
    const positions = positionsRef.current;

    graph.clear();

    for (const node of filtered.nodes) {
      const pos = positions.get(node.id) ?? positionFor(node.id);
      positions.set(node.id, pos);
      graph.addNode(node.id, {
        label: node.label,
        fullLabel: node.label,
        size: Math.max(2, node.size || 1),
        color: effectiveNodeColor(node),
        nodeKind: node.kind,
        clusterId: node.cluster_id,
        x: pos.x,
        y: pos.y,
      });
    }

    for (const edge of filtered.edges) {
      if (!graph.hasNode(edge.source) || !graph.hasNode(edge.target)) continue;
      graph.addEdgeWithKey(edge.id, edge.source, edge.target, {
        size: 0.5,
        color: "#2d3548",
      });
    }

    if (!rendererRef.current && containerRef.current) {
      const renderer = new Sigma(graph, containerRef.current, {
        renderEdgeLabels: false,
        labelSize: 12,
        labelColor: { color: "#e8ecf4" },
        labelFont: "Inter, system-ui, sans-serif",
        zIndex: true,
        nodeReducer: (node, data) => {
          const out = { ...data };
          if (settings.hideLabels) out.label = "";
          const hovered = hoveredRef.current;
          if (hovered) {
            if (node === hovered) {
              out.color = HIGHLIGHT.node;
              out.zIndex = 1;
            } else {
              out.color = HIGHLIGHT.dim;
              out.zIndex = 0;
            }
          }
          return out;
        },
        edgeReducer: (edge, data) => {
          const out = { ...data };
          const hovered = hoveredRef.current;
          if (hovered) {
            const src = graph.source(edge);
            const tgt = graph.target(edge);
            if (src === hovered || tgt === hovered) {
              out.color = HIGHLIGHT.edge;
              out.size = 1.2;
            } else {
              out.color = HIGHLIGHT.dim;
              out.size = 0.3;
            }
          }
          return out;
        },
        hoverRenderer: (ctx, data, sigmaSettings) => {
          const label =
            (data.fullLabel as string | undefined) ?? data.label ?? "";
          const size = data.size || 1;

          ctx.beginPath();
          ctx.fillStyle = HIGHLIGHT.node;
          ctx.strokeStyle = HIGHLIGHT.outline;
          ctx.lineWidth = 2;
          ctx.arc(data.x, data.y, size + 2, 0, Math.PI * 2);
          ctx.fill();
          ctx.stroke();

          if (!label) return;

          const fontSize = sigmaSettings.labelSize || 12;
          const font = `${fontSize}px ${sigmaSettings.labelFont || "Inter"}`;
          ctx.font = font;
          const px = 8, py = 6;
          const tw = ctx.measureText(label).width;
          const bw = tw + px * 2;
          const bh = fontSize + py;
          const bx = data.x + size + 4;
          const by = data.y - bh / 2;

          ctx.fillStyle = HIGHLIGHT.labelBg;
          drawRoundedRect(ctx, bx, by, bw, bh, 6);
          ctx.fill();

          ctx.fillStyle = HIGHLIGHT.label;
          ctx.textBaseline = "middle";
          ctx.fillText(label, bx + px, data.y);
        },
      });

      renderer.on("enterNode", ({ node }) => {
        hoveredRef.current = node;
        renderer.refresh();
      });
      renderer.on("leaveNode", () => {
        hoveredRef.current = null;
        renderer.refresh();
      });

      rendererRef.current = renderer;
    } else {
      rendererRef.current?.refresh();
    }

    // layout logic
    const isForce =
      settings.layout === "forceatlas2" ||
      settings.layout === "forceatlas2-cluster";
    const layoutChanged = prevLayoutRef.current !== settings.layout;
    prevLayoutRef.current = settings.layout;

    if (isForce && settings.forceAuto) {
      applyLayout(settings.layout);
    } else if (layoutChanged) {
      applyLayout(settings.layout);
    }
  }, [payload, settings.showTags, settings.hideLabels, settings.clusterEnabled, settings.layout, settings.forceAuto, effectiveNodeColor, applyLayout]);

  /* ── cleanup ────────────────────────────────────────────────────── */

  useEffect(() => {
    return () => {
      rendererRef.current?.kill();
      rendererRef.current = null;
    };
  }, []);

  return (
    <div
      ref={containerRef}
      className="flex-1 bg-surface-graph overflow-hidden"
    />
  );
}
