(() => {
  const statusEl = document.getElementById("status");
  const countsEl = document.getElementById("counts");
  const container = document.getElementById("graph");

  const Graph = graphology.Graph;
  let graph = new Graph();
  let renderer = null;
  const positions = new Map();
  let lastPayload = null;
  const toggleTags = document.getElementById("toggle-tags");
  const toggleLabels = document.getElementById("toggle-labels");
  const layoutSelect = document.getElementById("layout-select");
  const forceToggle = document.getElementById("force-toggle");
  const toggleForceAuto = document.getElementById("toggle-force-auto");
  const clusterToggle = document.getElementById("cluster-toggle");
  const toggleCluster = document.getElementById("toggle-cluster");
  const minScoreWrap = document.getElementById("min-score-wrap");
  const minScore = document.getElementById("min-score");
  const minScoreValue = document.getElementById("min-score-value");
  const topKWrap = document.getElementById("top-k-wrap");
  const topK = document.getElementById("top-k");
  const topKValue = document.getElementById("top-k-value");
  let currentLayout = "static";
  let layoutDirty = false;
  let socket = null;
  let similarityAvailable = false;
  let clusterMap = new Map();
  let nodeKindMap = new Map();

  const nodeColors = {
    markdown: "#6dd4ff",
    canvas: "#f2c97d",
    attachment: "#a3b1c6",
    other: "#8b9bb0",
    tag: "#9ad68b",
  };

  const highlightColors = {
    node: "#ffb84d",
    label: "#f6f8fc",
    labelBg: "#0b1220",
    outline: "#1c2a3f",
    edge: "#6dd4ff",
    dim: "#2a3342",
  };

  function drawRoundedRect(ctx, x, y, width, height, radius) {
    const r = Math.min(radius, width / 2, height / 2);
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + width - r, y);
    ctx.quadraticCurveTo(x + width, y, x + width, y + r);
    ctx.lineTo(x + width, y + height - r);
    ctx.quadraticCurveTo(x + width, y + height, x + width - r, y + height);
    ctx.lineTo(x + r, y + height);
    ctx.quadraticCurveTo(x, y + height, x, y + height - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  }

  function hashString(value) {
    let hash = 0;
    for (let i = 0; i < value.length; i += 1) {
      hash = (hash * 31 + value.charCodeAt(i)) | 0;
    }
    return hash;
  }

  function positionFor(id) {
    const hash = hashString(id);
    const angle = ((hash % 360) * Math.PI) / 180;
    const radius = 2 + (Math.abs(hash) % 100) / 50;
    return {
      x: Math.cos(angle) * radius,
      y: Math.sin(angle) * radius,
    };
  }

  function nodeColor(kind) {
    return nodeColors[kind] || nodeColors.other;
  }

  function clusterColor(clusterId) {
    const hue = (clusterId * 47) % 360;
    return hslToHex(hue, 0.62, 0.62);
  }

  function hslToHex(h, s, l) {
    const c = (1 - Math.abs(2 * l - 1)) * s;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const m = l - c / 2;
    let r = 0;
    let g = 0;
    let b = 0;

    if (h < 60) {
      r = c;
      g = x;
    } else if (h < 120) {
      r = x;
      g = c;
    } else if (h < 180) {
      g = c;
      b = x;
    } else if (h < 240) {
      g = x;
      b = c;
    } else if (h < 300) {
      r = x;
      b = c;
    } else {
      r = c;
      b = x;
    }

    const toHex = (value) => {
      const v = Math.round((value + m) * 255);
      return v.toString(16).padStart(2, "0");
    };

    return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
  }

  function effectiveNodeColor(node) {
    if (toggleCluster.checked && node.cluster_id != null && node.kind !== "tag") {
      return clusterColor(node.cluster_id);
    }
    return nodeColor(node.kind);
  }

  function applyGraph(payload) {
    graph.clear();

    clusterMap = new Map();
    nodeKindMap = new Map();
    for (const node of payload.nodes) {
      nodeKindMap.set(node.id, node.kind);
      if (node.cluster_id != null) {
        clusterMap.set(node.id, node.cluster_id);
      }
    }

    for (const node of payload.nodes) {
      const pos = positions.get(node.id) || positionFor(node.id);
      positions.set(node.id, pos);

      graph.addNode(node.id, {
        label: node.label,
        fullLabel: node.label,
        size: Math.max(2, node.size || 1),
        color: effectiveNodeColor(node),
        x: pos.x,
        y: pos.y,
      });
    }

    for (const edge of payload.edges) {
      if (!graph.hasNode(edge.source) || !graph.hasNode(edge.target)) {
        continue;
      }

      graph.addEdgeWithKey(edge.id, edge.source, edge.target, {
        size: 0.5,
        color: "#45566c",
      });
    }

    if (!renderer) {
      let hoveredNode = null;

      renderer = new Sigma(graph, container, {
        renderEdgeLabels: false,
        labelSize: 12,
        labelColor: { color: "#e8eef6" },
        labelFont: "Cantarell",
        zIndex: true,
        nodeReducer: (node, data) => {
          const out = { ...data };
          if (toggleLabels.checked) {
            out.label = "";
          }
          if (hoveredNode) {
            if (node === hoveredNode) {
              out.color = highlightColors.node;
              out.labelColor = highlightColors.label;
              out.zIndex = 1;
            } else {
              out.color = highlightColors.dim;
              out.borderWidth = 0;
              out.labelColor = "#8896aa";
              out.zIndex = 0;
            }
          }
          return out;
        },
        edgeReducer: (edge, data) => {
          const out = { ...data };
          if (hoveredNode) {
            const source = graph.source(edge);
            const target = graph.target(edge);
            if (source === hoveredNode || target === hoveredNode) {
              out.color = highlightColors.edge;
              out.size = 1.2;
            } else {
              out.color = highlightColors.dim;
              out.size = 0.3;
            }
          }
          return out;
        },
        hoverRenderer: (ctx, data, settings) => {
          const label = data.fullLabel || data.label;
          const size = data.size || 1;
          ctx.beginPath();
          ctx.fillStyle = highlightColors.node;
          ctx.strokeStyle = highlightColors.outline;
          ctx.lineWidth = 2;
          ctx.arc(data.x, data.y, size + 2, 0, Math.PI * 2);
          ctx.fill();
          ctx.stroke();

          if (!label) {
            return;
          }

          const fontSize = settings.labelSize || 12;
          const font = `${fontSize}px ${settings.labelFont || "Cantarell"}`;
          ctx.font = font;
          const paddingX = 8;
          const paddingY = 6;
          const textWidth = ctx.measureText(label).width;
          const boxWidth = textWidth + paddingX * 2;
          const boxHeight = fontSize + paddingY;
          const boxX = data.x + size + 4;
          const boxY = data.y - boxHeight / 2;

          ctx.fillStyle = highlightColors.labelBg;
          drawRoundedRect(ctx, boxX, boxY, boxWidth, boxHeight, 6);
          ctx.fill();

          ctx.fillStyle = highlightColors.label;
          ctx.textBaseline = "middle";
          ctx.fillText(label, boxX + paddingX, data.y);
        },
      });

      renderer.on("enterNode", ({ node }) => {
        hoveredNode = node;
        renderer.refresh();
      });
      renderer.on("leaveNode", () => {
        hoveredNode = null;
        renderer.refresh();
      });
    } else {
      renderer.refresh();
    }

    countsEl.textContent = `nodes: ${payload.nodes.length} edges: ${payload.edges.length}`;

    if (
      (currentLayout === "forceatlas2" ||
        currentLayout === "forceatlas2-cluster") &&
      toggleForceAuto.checked
    ) {
      applyLayout(currentLayout);
      return;
    }

    if (layoutDirty) {
      applyLayout(currentLayout);
      layoutDirty = false;
    }
  }

  function updateSimilarityUi(payload) {
    similarityAvailable = Boolean(payload.similarity && payload.similarity.available);
    const enabled = similarityAvailable && payload.similarity.enabled;
    clusterToggle.style.display = similarityAvailable ? "inline-flex" : "none";
    minScoreWrap.style.display = similarityAvailable ? "inline-flex" : "none";
    topKWrap.style.display = similarityAvailable ? "inline-flex" : "none";
    toggleCluster.checked = enabled;
    minScore.value = payload.similarity.min_score;
    minScoreValue.value = payload.similarity.min_score;
    topK.value = payload.similarity.top_k;
    topKValue.value = payload.similarity.top_k;
  }

  function sendSimilaritySettings() {
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return;
    }
    socket.send(
      JSON.stringify({
        type: "similarity_settings",
        enabled: toggleCluster.checked,
        min_score: parseFloat(minScoreValue.value),
        top_k: parseInt(topKValue.value, 10),
      }),
    );
  }

  function syncPositionsFromGraph() {
    for (const id of graph.nodes()) {
      const x = graph.getNodeAttribute(id, "x");
      const y = graph.getNodeAttribute(id, "y");
      positions.set(id, { x, y });
    }
  }

  function applyLayout(layout) {
    if (!renderer) {
      return;
    }

    const ids = graph.nodes();
    if (!ids.length) {
      return;
    }

    if (layout === "random") {
      for (const id of ids) {
        const x = Math.random() * 4 - 2;
        const y = Math.random() * 4 - 2;
        graph.setNodeAttribute(id, "x", x);
        graph.setNodeAttribute(id, "y", y);
        positions.set(id, { x, y });
      }
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

    if (layout === "forceatlas2") {
      if (!window.forceAtlas2) {
        console.warn("forceatlas2 layout not available");
        return;
      }

      let hasNonZero = false;
      for (const id of ids) {
        const x = graph.getNodeAttribute(id, "x");
        const y = graph.getNodeAttribute(id, "y");
        if (typeof x !== "number" || typeof y !== "number") {
          hasNonZero = false;
          break;
        }
        if (x !== 0 || y !== 0) {
          hasNonZero = true;
        }
      }

      if (!hasNonZero) {
        for (const id of ids) {
          const x = Math.random() * 4 - 2;
          const y = Math.random() * 4 - 2;
          graph.setNodeAttribute(id, "x", x);
          graph.setNodeAttribute(id, "y", y);
          positions.set(id, { x, y });
        }
      }

      const settings = window.forceAtlas2.inferSettings(graph);
      window.forceAtlas2.assign(graph, {
        iterations: 80,
        settings,
        getEdgeWeight: (_, attr) => attr.weight || 1,
      });
      syncPositionsFromGraph();
      renderer.refresh();
      return;
    }

    if (layout === "forceatlas2-cluster") {
      if (!window.forceAtlas2) {
        console.warn("forceatlas2 layout not available");
        return;
      }

      if (!clusterMap.size) {
        applyLayout("forceatlas2");
        return;
      }

      let hasNonZero = false;
      for (const id of ids) {
        const x = graph.getNodeAttribute(id, "x");
        const y = graph.getNodeAttribute(id, "y");
        if (typeof x !== "number" || typeof y !== "number") {
          hasNonZero = false;
          break;
        }
        if (x !== 0 || y !== 0) {
          hasNonZero = true;
        }
      }

      if (!hasNonZero) {
        for (const id of ids) {
          const x = Math.random() * 4 - 2;
          const y = Math.random() * 4 - 2;
          graph.setNodeAttribute(id, "x", x);
          graph.setNodeAttribute(id, "y", y);
          positions.set(id, { x, y });
        }
      }

      const tempEdges = addClusterEdges();
      const settings = window.forceAtlas2.inferSettings(graph);
      settings.gravity = Math.max(0.1, settings.gravity * 0.7);
      settings.scalingRatio = Math.max(2, settings.scalingRatio * 1.1);
      window.forceAtlas2.assign(graph, {
        iterations: 90,
        settings,
        getEdgeWeight: (_, attr) => attr.weight || 1,
      });
      cleanupClusterEdges(tempEdges);
      syncPositionsFromGraph();
      renderer.refresh();
    }
  }

  function addClusterEdges() {
    const clusters = new Map();
    for (const [id, clusterId] of clusterMap.entries()) {
      const kind = nodeKindMap.get(id);
      if (!kind || kind === "tag") {
        continue;
      }
      if (!clusters.has(clusterId)) {
        clusters.set(clusterId, []);
      }
      clusters.get(clusterId).push(id);
    }

    const keys = [];
    for (const [clusterId, ids] of clusters.entries()) {
      if (ids.length < 2) {
        continue;
      }
      ids.sort();
      for (let i = 0; i < ids.length - 1; i += 1) {
        const key = `cluster:${clusterId}:${i}`;
        if (graph.hasEdge(key) || graph.hasEdge(ids[i], ids[i + 1])) {
          continue;
        }
        graph.addEdgeWithKey(key, ids[i], ids[i + 1], { weight: 4 });
        keys.push(key);
      }
    }
    return keys;
  }

  function cleanupClusterEdges(keys) {
    for (const key of keys) {
      if (graph.hasEdge(key)) {
        graph.dropEdge(key);
      }
    }
  }

  function filterPayload(payload, showTags) {
    if (showTags) {
      return payload;
    }

    const nodes = payload.nodes.filter((node) => node.kind !== "tag");
    const nodeIds = new Set(nodes.map((node) => node.id));
    const edges = payload.edges.filter(
      (edge) => nodeIds.has(edge.source) && nodeIds.has(edge.target),
    );

    return { nodes, edges };
  }

  function setStatus(state, text) {
    statusEl.dataset.state = state;
    statusEl.textContent = text;
  }

  function connect() {
    const protocol = location.protocol === "https:" ? "wss" : "ws";
    socket = new WebSocket(`${protocol}://${location.host}/ws`);

    setStatus("connecting", "connecting");

    socket.addEventListener("open", () => {
      setStatus("connected", "connected");
      sendSimilaritySettings();
    });

    socket.addEventListener("close", () => {
      setStatus("disconnected", "disconnected");
      setTimeout(connect, 1500);
    });

    socket.addEventListener("message", (event) => {
      try {
        const payload = JSON.parse(event.data);
        updateSimilarityUi(payload);
        lastPayload = payload;
        const showTags = toggleTags.checked;
        applyGraph(filterPayload(payload, showTags));
      } catch (err) {
        console.warn("failed to parse graph payload", err);
      }
    });
  }

  window.addEventListener("load", () => {
    if (typeof Sigma === "undefined" || typeof graphology === "undefined") {
      setStatus("disconnected", "cdn missing");
      return;
    }

    const stored = localStorage.getItem("oxidian.showTags");
    const showTags = stored ? stored === "true" : true;
    toggleTags.checked = showTags;
    toggleTags.addEventListener("change", () => {
      const enabled = toggleTags.checked;
      localStorage.setItem("oxidian.showTags", String(enabled));
      if (lastPayload) {
        applyGraph(filterPayload(lastPayload, enabled));
      }
    });

    const storedLabels = localStorage.getItem("oxidian.hideLabels");
    toggleLabels.checked = storedLabels === "true";
    toggleLabels.addEventListener("change", () => {
      localStorage.setItem("oxidian.hideLabels", String(toggleLabels.checked));
      if (lastPayload) {
        applyGraph(filterPayload(lastPayload, toggleTags.checked));
      }
    });

    const storedLayout = localStorage.getItem("oxidian.layout");
    currentLayout = storedLayout || "static";
    layoutSelect.value = currentLayout;
    layoutSelect.addEventListener("change", () => {
      currentLayout = layoutSelect.value;
      localStorage.setItem("oxidian.layout", currentLayout);
      layoutDirty = true;
      applyLayout(currentLayout);
      layoutDirty = false;
      forceToggle.style.display =
        currentLayout === "forceatlas2" || currentLayout === "forceatlas2-cluster"
          ? "inline-flex"
          : "none";
    });

    const storedForceAuto = localStorage.getItem("oxidian.forceAuto");
    toggleForceAuto.checked = storedForceAuto === "true";
    toggleForceAuto.addEventListener("change", () => {
      localStorage.setItem("oxidian.forceAuto", String(toggleForceAuto.checked));
    });
    forceToggle.style.display =
      currentLayout === "forceatlas2" || currentLayout === "forceatlas2-cluster"
        ? "inline-flex"
        : "none";

    const storedCluster = localStorage.getItem("oxidian.clusterEnabled");
    toggleCluster.checked = storedCluster === "true";
    toggleCluster.addEventListener("change", () => {
      localStorage.setItem("oxidian.clusterEnabled", String(toggleCluster.checked));
      if (lastPayload) {
        applyGraph(filterPayload(lastPayload, toggleTags.checked));
      }
      sendSimilaritySettings();
    });

    const storedMinScore = localStorage.getItem("oxidian.minScore");
    const storedTopK = localStorage.getItem("oxidian.topK");
    minScore.value = storedMinScore || "0.6";
    minScoreValue.value = minScore.value;
    topK.value = storedTopK || "8";
    topKValue.value = topK.value;

    minScore.addEventListener("input", () => {
      minScoreValue.value = minScore.value;
    });
    minScoreValue.addEventListener("change", () => {
      minScore.value = minScoreValue.value;
    });
    topK.addEventListener("input", () => {
      topKValue.value = topK.value;
    });
    topKValue.addEventListener("change", () => {
      topK.value = topKValue.value;
    });

    const handleSimilarityChange = () => {
      localStorage.setItem("oxidian.minScore", String(minScore.value));
      localStorage.setItem("oxidian.topK", String(topK.value));
      sendSimilaritySettings();
    };
    minScore.addEventListener("change", handleSimilarityChange);
    minScoreValue.addEventListener("change", handleSimilarityChange);
    topK.addEventListener("change", handleSimilarityChange);
    topKValue.addEventListener("change", handleSimilarityChange);

    connect();
  });
})();
