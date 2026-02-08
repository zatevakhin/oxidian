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
  const layoutSelect = document.getElementById("layout-select");
  const forceToggle = document.getElementById("force-toggle");
  const toggleForceAuto = document.getElementById("toggle-force-auto");
  let currentLayout = "static";
  let layoutDirty = false;

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

  function applyGraph(payload) {
    graph.clear();

    for (const node of payload.nodes) {
      const pos = positions.get(node.id) || positionFor(node.id);
      positions.set(node.id, pos);

      graph.addNode(node.id, {
        label: node.label,
        size: Math.max(2, node.size || 1),
        color: nodeColor(node.kind),
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
          const size = data.size || 1;
          ctx.beginPath();
          ctx.fillStyle = highlightColors.node;
          ctx.strokeStyle = highlightColors.outline;
          ctx.lineWidth = 2;
          ctx.arc(data.x, data.y, size + 2, 0, Math.PI * 2);
          ctx.fill();
          ctx.stroke();

          if (!data.label) {
            return;
          }

          const fontSize = settings.labelSize || 12;
          const font = `${fontSize}px ${settings.labelFont || "Cantarell"}`;
          ctx.font = font;
          const paddingX = 8;
          const paddingY = 6;
          const textWidth = ctx.measureText(data.label).width;
          const boxWidth = textWidth + paddingX * 2;
          const boxHeight = fontSize + paddingY;
          const boxX = data.x + size + 4;
          const boxY = data.y - boxHeight / 2;

          ctx.fillStyle = highlightColors.labelBg;
          drawRoundedRect(ctx, boxX, boxY, boxWidth, boxHeight, 6);
          ctx.fill();

          ctx.fillStyle = highlightColors.label;
          ctx.textBaseline = "middle";
          ctx.fillText(data.label, boxX + paddingX, data.y);
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

    if (currentLayout === "forceatlas2" && toggleForceAuto.checked) {
      applyLayout(currentLayout);
      return;
    }

    if (layoutDirty) {
      applyLayout(currentLayout);
      layoutDirty = false;
    }
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
      window.forceAtlas2.assign(graph, { iterations: 80, settings });
      syncPositionsFromGraph();
      renderer.refresh();
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
    const socket = new WebSocket(`${protocol}://${location.host}/ws`);

    setStatus("connecting", "connecting");

    socket.addEventListener("open", () => {
      setStatus("connected", "connected");
    });

    socket.addEventListener("close", () => {
      setStatus("disconnected", "disconnected");
      setTimeout(connect, 1500);
    });

    socket.addEventListener("message", (event) => {
      try {
        const payload = JSON.parse(event.data);
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
        currentLayout === "forceatlas2" ? "inline-flex" : "none";
    });

    const storedForceAuto = localStorage.getItem("oxidian.forceAuto");
    toggleForceAuto.checked = storedForceAuto === "true";
    toggleForceAuto.addEventListener("change", () => {
      localStorage.setItem("oxidian.forceAuto", String(toggleForceAuto.checked));
    });
    forceToggle.style.display =
      currentLayout === "forceatlas2" ? "inline-flex" : "none";

    connect();
  });
})();
