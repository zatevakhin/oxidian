export interface GraphNode {
  id: string;
  label: string;
  kind: "markdown" | "canvas" | "attachment" | "other" | "tag";
  size: number;
  cluster_id?: number;
}

export interface GraphEdge {
  id: string;
  source: string;
  target: string;
}

export interface SimilarityMeta {
  available: boolean;
  enabled: boolean;
  min_score: number;
  top_k: number;
}

export interface GraphPayload {
  nodes: GraphNode[];
  edges: GraphEdge[];
  similarity: SimilarityMeta;
}

export interface SimilaritySettingsMessage {
  type: "similarity_settings";
  enabled: boolean;
  min_score: number;
  top_k: number;
}

export type ConnectionStatus = "connecting" | "connected" | "disconnected";

export type LayoutMode =
  | "static"
  | "circle"
  | "random"
  | "forceatlas2"
  | "forceatlas2-cluster";

export interface GraphSettings {
  layout: LayoutMode;
  hideLabels: boolean;
  showTags: boolean;
  forceAuto: boolean;
  clusterEnabled: boolean;
  minScore: number;
  topK: number;
}
