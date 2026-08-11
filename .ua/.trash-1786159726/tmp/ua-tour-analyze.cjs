#!/usr/bin/env node
/**
 * Graph topology analyzer for tour design.
 * Usage: node ua-tour-analyze.js <input.json> <output.json>
 */
const fs = require("fs");

function fail(msg) {
  console.error(msg);
  process.exit(1);
}

function main() {
  const [inputPath, outputPath] = process.argv.slice(2);
  if (!inputPath || !outputPath) fail("Usage: node ua-tour-analyze.js <input> <output>");

  let data;
  try {
    data = JSON.parse(fs.readFileSync(inputPath, "utf8"));
  } catch (e) {
    fail(`Failed to read input: ${e.message}`);
  }

  const nodes = data.nodes || [];
  const edges = data.edges || [];
  const layers = data.layers || [];
  const nodeById = new Map(nodes.map((n) => [n.id, n]));

  // Build adjacency
  const fanIn = new Map();
  const fanOut = new Map();
  const outAdj = new Map(); // for BFS: imports/calls
  const undirected = new Map(); // for clusters

  for (const n of nodes) {
    fanIn.set(n.id, 0);
    fanOut.set(n.id, 0);
    outAdj.set(n.id, new Set());
    undirected.set(n.id, new Set());
  }

  for (const e of edges) {
    const s = e.source;
    const t = e.target;
    if (!nodeById.has(s) || !nodeById.has(t)) continue;
    fanOut.set(s, (fanOut.get(s) || 0) + 1);
    fanIn.set(t, (fanIn.get(t) || 0) + 1);
    undirected.get(s).add(t);
    undirected.get(t).add(s);
    if (e.type === "imports" || e.type === "calls") {
      outAdj.get(s).add(t);
    }
  }

  // A. Fan-in ranking
  const fanInRanking = [...fanIn.entries()]
    .map(([id, count]) => ({
      id,
      fanIn: count,
      name: nodeById.get(id)?.name || id,
    }))
    .sort((a, b) => b.fanIn - a.fanIn)
    .slice(0, 20);

  // B. Fan-out ranking
  const fanOutRanking = [...fanOut.entries()]
    .map(([id, count]) => ({
      id,
      fanOut: count,
      name: nodeById.get(id)?.name || id,
    }))
    .sort((a, b) => b.fanOut - a.fanOut)
    .slice(0, 20);

  // C. Entry point candidates
  const ENTRY_NAMES = new Set([
    "index.ts", "index.js", "main.ts", "main.js", "app.ts", "app.js",
    "server.ts", "server.js", "mod.rs", "main.go", "main.py", "main.rs",
    "manage.py", "app.py", "wsgi.py", "asgi.py", "run.py", "__main__.py",
    "Application.java", "Main.java", "Program.cs", "config.ru", "index.php",
    "App.swift", "Application.kt", "main.cpp", "main.c", "lib.rs", "App.vue",
  ]);

  const fanOutValues = [...fanOut.values()].sort((a, b) => a - b);
  const fanInValues = [...fanIn.values()].sort((a, b) => a - b);
  const fanOutP90 = fanOutValues[Math.floor(fanOutValues.length * 0.9)] || 0;
  const fanInP25 = fanInValues[Math.floor(fanInValues.length * 0.25)] || 0;

  function depthOfPath(fp) {
    if (!fp) return 99;
    return fp.split("/").filter(Boolean).length;
  }

  const scored = [];
  for (const n of nodes) {
    let score = 0;
    const fp = n.filePath || "";
    const name = n.name || "";
    const type = n.type || "";

    if (type === "document") {
      if (name === "README.md" && depthOfPath(fp) === 1) score += 5;
      else if (name.endsWith(".md") && depthOfPath(fp) === 1) score += 2;
    } else if (type === "file") {
      if (ENTRY_NAMES.has(name)) score += 3;
      const d = depthOfPath(fp);
      if (d <= 2) score += 1;
      if ((fanOut.get(n.id) || 0) >= fanOutP90 && fanOutP90 > 0) score += 1;
      if ((fanIn.get(n.id) || 0) <= fanInP25) score += 1;
    }

    if (score > 0) {
      scored.push({
        id: n.id,
        score,
        name: n.name,
        summary: n.summary || "",
      });
    }
  }
  scored.sort((a, b) => b.score - a.score);
  const entryPointCandidates = scored.slice(0, 5);

  // D. BFS from top code entry point
  const codeEntry =
    scored.find((c) => {
      const n = nodeById.get(c.id);
      return n && n.type === "file";
    }) || null;

  // Prefer known project entries if present
  const preferredIds = ["file:src/main.ts", "file:src-tauri/src/lib.rs", "file:src-tauri/src/main.rs"];
  let startNode = codeEntry?.id || null;
  for (const pid of preferredIds) {
    if (nodeById.has(pid)) {
      startNode = pid;
      break;
    }
  }

  const bfsTraversal = {
    startNode: startNode,
    order: [],
    depthMap: {},
    byDepth: {},
  };

  if (startNode) {
    const visited = new Set();
    const queue = [[startNode, 0]];
    visited.add(startNode);
    while (queue.length) {
      const [cur, depth] = queue.shift();
      bfsTraversal.order.push(cur);
      bfsTraversal.depthMap[cur] = depth;
      const key = String(depth);
      if (!bfsTraversal.byDepth[key]) bfsTraversal.byDepth[key] = [];
      bfsTraversal.byDepth[key].push(cur);
      for (const next of outAdj.get(cur) || []) {
        if (!visited.has(next)) {
          visited.add(next);
          queue.push([next, depth + 1]);
        }
      }
    }
  }

  // E. Non-code inventory
  const nonCodeFiles = {
    documentation: [],
    infrastructure: [],
    data: [],
    config: [],
  };
  for (const n of nodes) {
    const item = {
      id: n.id,
      name: n.name,
      type: n.type,
      summary: n.summary || "",
    };
    if (n.type === "document") nonCodeFiles.documentation.push(item);
    else if (["service", "pipeline", "resource"].includes(n.type))
      nonCodeFiles.infrastructure.push(item);
    else if (["table", "schema", "endpoint"].includes(n.type))
      nonCodeFiles.data.push(item);
    else if (n.type === "config") nonCodeFiles.config.push(item);
  }

  // F. Clusters — bidirectional pairs, expand
  const bidir = new Set();
  const pairKey = (a, b) => (a < b ? `${a}|${b}` : `${b}|${a}`);
  const importsCalls = edges.filter(
    (e) =>
      (e.type === "imports" || e.type === "calls") &&
      nodeById.has(e.source) &&
      nodeById.has(e.target)
  );
  const directed = new Set(importsCalls.map((e) => `${e.source}->${e.target}`));
  for (const e of importsCalls) {
    if (directed.has(`${e.target}->${e.source}`)) {
      bidir.add(pairKey(e.source, e.target));
    }
  }

  const clusters = [];
  const used = new Set();
  for (const key of bidir) {
    const [a, b] = key.split("|");
    if (used.has(a) || used.has(b)) continue;
    const cluster = new Set([a, b]);
    let changed = true;
    while (changed && cluster.size < 5) {
      changed = false;
      for (const n of nodes) {
        if (cluster.has(n.id) || cluster.size >= 5) continue;
        let links = 0;
        for (const m of cluster) {
          if (undirected.get(n.id)?.has(m)) links++;
        }
        if (links >= 2) {
          cluster.add(n.id);
          changed = true;
        }
      }
    }
    const members = [...cluster];
    if (members.length >= 2 && members.length <= 5) {
      let edgeCount = 0;
      for (const e of edges) {
        if (cluster.has(e.source) && cluster.has(e.target)) edgeCount++;
      }
      members.forEach((id) => used.add(id));
      clusters.push({ nodes: members, edgeCount });
    }
  }
  clusters.sort((a, b) => b.edgeCount - a.edgeCount);
  const topClusters = clusters.slice(0, 10);

  // G. Layers
  const layersOut = {
    count: layers.length,
    list: layers.map((l) => ({
      id: l.id,
      name: l.name,
      description: l.description || "",
    })),
  };

  // H. Node summary index
  const nodeSummaryIndex = {};
  for (const n of nodes) {
    nodeSummaryIndex[n.id] = {
      name: n.name,
      type: n.type,
      summary: n.summary || "",
    };
  }

  const result = {
    scriptCompleted: true,
    entryPointCandidates,
    fanInRanking,
    fanOutRanking,
    bfsTraversal,
    nonCodeFiles,
    clusters: topClusters,
    layers: layersOut,
    nodeSummaryIndex,
    totalNodes: nodes.length,
    totalEdges: edges.length,
  };

  try {
    fs.writeFileSync(outputPath, JSON.stringify(result, null, 2));
  } catch (e) {
    fail(`Failed to write output: ${e.message}`);
  }
  console.log(
    `OK: ${nodes.length} nodes, ${edges.length} edges → ${outputPath}`
  );
}

main();
