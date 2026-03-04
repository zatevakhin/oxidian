import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ConnectionStatus,
  GraphPayload,
  SimilaritySettingsMessage,
} from "../types";

const RECONNECT_DELAY = 1500;

export interface GraphWs {
  status: ConnectionStatus;
  payload: GraphPayload | null;
  sendSimilaritySettings: (
    enabled: boolean,
    minScore: number,
    topK: number,
  ) => void;
}

export function useGraphWs(): GraphWs {
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  const [payload, setPayload] = useState<GraphPayload | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const connect = useCallback(() => {
    const protocol = location.protocol === "https:" ? "wss" : "ws";
    const url = `${protocol}://${location.host}/ws`;
    const ws = new WebSocket(url);
    socketRef.current = ws;

    setStatus("connecting");

    ws.addEventListener("open", () => {
      setStatus("connected");
    });

    ws.addEventListener("close", () => {
      setStatus("disconnected");
      socketRef.current = null;
      reconnectTimer.current = setTimeout(connect, RECONNECT_DELAY);
    });

    ws.addEventListener("message", (event) => {
      try {
        const data = JSON.parse(event.data as string) as GraphPayload;
        setPayload(data);
      } catch {
        console.warn("failed to parse graph payload");
      }
    });
  }, []);

  useEffect(() => {
    connect();
    return () => {
      if (reconnectTimer.current) {
        clearTimeout(reconnectTimer.current);
      }
      if (socketRef.current) {
        socketRef.current.close();
      }
    };
  }, [connect]);

  const sendSimilaritySettings = useCallback(
    (enabled: boolean, minScore: number, topK: number) => {
      const ws = socketRef.current;
      if (!ws || ws.readyState !== WebSocket.OPEN) return;
      const msg: SimilaritySettingsMessage = {
        type: "similarity_settings",
        enabled,
        min_score: minScore,
        top_k: topK,
      };
      ws.send(JSON.stringify(msg));
    },
    [],
  );

  return { status, payload, sendSimilaritySettings };
}
