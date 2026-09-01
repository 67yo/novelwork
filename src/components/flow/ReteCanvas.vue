<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { NodeEditor, type GetSchemes } from "rete";
import { AreaExtensions, AreaPlugin, Zoom, type Area2D } from "rete-area-plugin";
import { BidirectFlow, ConnectionPlugin, getSourceTarget } from "rete-connection-plugin";
import { Presets, VuePlugin, type VueArea2D } from "rete-vue-plugin";
import { Minus, Plus, Maximize2 } from "@lucide/vue";
import { useI18n } from "@/i18n";
import {
  StoryConnection,
  StoryReteNode,
  type FlowCanvasEdge,
  type FlowCanvasNode,
  type FlowConnect,
} from "@/lib/flowCanvas";
import { storyConnectionPath } from "@/lib/flowConnectionPath";
import StoryNode from "@/components/flow/StoryNode.vue";
import StorySocket from "@/components/flow/StorySocket.vue";
import StoryConnectionView from "@/components/flow/StoryConnection.vue";

type Schemes = GetSchemes<StoryReteNode, StoryConnection>;
type AreaExtra = Area2D<Schemes> | VueArea2D<Schemes>;

/** One zoom per frame — default Zoom does getBoundingClientRect + async pipe on every wheel tick. */
class RafZoom extends Zoom {
  private accPx = 0;
  private cx = 0;
  private cy = 0;
  private raf = 0;
  constructor() {
    super(0.1);
    this.wheel = (e: WheelEvent) => {
      e.preventDefault();
      const px = e.deltaMode === 1 ? e.deltaY * 8 : e.deltaMode === 2 ? e.deltaY * 24 : e.deltaY;
      this.accPx += px;
      this.cx = e.clientX;
      this.cy = e.clientY;
      if (this.raf) return;
      this.raf = requestAnimationFrame(() => {
        this.raf = 0;
        const pixels = this.accPx;
        this.accPx = 0;
        const intensity = 0.1;
        const raw = (-pixels * intensity) / 8;
        const delta = Math.sign(raw) * Math.min(intensity, Math.abs(raw));
        if (!delta || !this.element) return;
        const { left, top } = this.element.getBoundingClientRect();
        this.onzoom(delta, (left - this.cx) * delta, (top - this.cy) * delta, "wheel");
      });
    };
  }
  destroy() {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    super.destroy();
  }
}

const props = withDefaults(
  defineProps<{
    nodes: FlowCanvasNode[];
    edges: FlowCanvasEdge[];
    selectedId?: string | null;
  }>(),
  { selectedId: null },
);

const emit = defineEmits<{
  nodeClick: [{ node: { data: FlowCanvasNode["data"] } }];
  connect: [conn: FlowConnect];
  edgeClick: [{ edge: { id: string } }];
  edgeDoubleClick: [{ edge: { id: string } }];
  nodeDragStop: [{ node: { id: string; position: { x: number; y: number } } }];
}>();

const { t } = useI18n();
const host = ref<HTMLElement | null>(null);

let editor: NodeEditor<Schemes> | null = null;
let area: AreaPlugin<Schemes, AreaExtra> | null = null;
let syncing = false;
let destroyed = false;
let draggingNode = false;
let dragStart: { x: number; y: number } | null = null;

function applyPickClass(side: "input" | "output" | null) {
  const el = host.value;
  if (!el) return;
  el.classList.toggle("rete-pick-out", side === "output");
  el.classList.toggle("rete-pick-in", side === "input");
}

async function createEditor() {
  const container = host.value;
  if (!container) return;
  const ed = new NodeEditor<Schemes>();
  const ar = new AreaPlugin<Schemes, AreaExtra>(container);
  ar.area.setZoomHandler(new RafZoom());
  const connection = new ConnectionPlugin<Schemes, AreaExtra>();
  const render = new VuePlugin<Schemes, AreaExtra>();

  render.addPreset(
    Presets.classic.setup({
      customize: {
        node() {
          return StoryNode;
        },
        socket() {
          return StorySocket;
        },
        connection() {
          return StoryConnectionView;
        },
      },
    }),
  );
  connection.addPreset(() =>
    new BidirectFlow({
      makeConnection(from, to) {
        const pair = getSourceTarget(from, to);
        if (!pair) return false;
        const [source, target] = pair;
        if (source.nodeId === target.nodeId) return false;
        emit("connect", {
          source: source.nodeId,
          target: target.nodeId,
          sourceHandle: source.key,
          targetHandle: target.key,
        });
        return true;
      },
    }),
  );

  ed.use(ar);
  ar.use(connection);
  ar.use(render);
  AreaExtensions.zIndexNodesOrder(ar);
  render.addPipe((ctx) => {
    if (ctx.type !== "connectionpath") return ctx;
    const data = (
      ctx as {
        data: {
          payload: StoryConnection;
          points: { x: number; y: number }[];
          path?: string;
        };
      }
    ).data;
    const pts = data.points;
    if (pts?.length >= 2) {
      data.path = storyConnectionPath(
        pts[0],
        pts[1],
        data.payload.sourceOutput,
        data.payload.targetInput,
      );
    }
    return ctx;
  });

  ar.addPipe((ctx) => {
    const sig = ctx as { type: string; data?: { socket?: { side: "input" | "output" }; id?: string } };
    if (sig.type === "connectionpick") {
      applyPickClass(sig.data?.socket?.side ?? null);
    }
    if (sig.type === "connectiondrop") {
      applyPickClass(null);
    }
    if (ctx.type === "nodepicked" && !syncing) {
      draggingNode = true;
      const view = ar.nodeViews.get(ctx.data.id);
      dragStart = view ? { ...view.position } : null;
      const node = ed.getNode(ctx.data.id);
      if (node) emit("nodeClick", { node: { data: node.payload } });
    }
    if (ctx.type === "nodedragged" && !syncing) {
      const view = ar.nodeViews.get(ctx.data.id);
      const moved =
        view &&
        dragStart &&
        (view.position.x !== dragStart.x || view.position.y !== dragStart.y);
      draggingNode = false;
      dragStart = null;
      if (moved && view) {
        emit("nodeDragStop", {
          node: { id: ctx.data.id, position: { ...view.position } },
        });
      }
      void markSelection();
    }
    return ctx;
  });

  editor = ed;
  area = ar;
}

async function rebuild() {
  if (!editor || !area || destroyed) return;
  syncing = true;
  try {
    for (const c of [...editor.getConnections()]) {
      await editor.removeConnection(c.id);
    }
    for (const n of [...editor.getNodes()]) {
      await editor.removeNode(n.id);
    }
    for (const item of props.nodes) {
      const node = new StoryReteNode(item.data);
      node.selected = item.id === props.selectedId;
      await editor.addNode(node);
      await area.translate(node.id, { ...item.position });
    }
    for (const e of props.edges) {
      const src = editor.getNode(e.source);
      const tgt = editor.getNode(e.target);
      if (!src || !tgt) continue;
      const sh = e.sourceHandle || "bottom";
      const th = e.targetHandle || "top";
      if (!src.hasOutput(sh) || !tgt.hasInput(th)) continue;
      const conn = new StoryConnection(src, sh, tgt, th);
      conn.id = e.id;
      conn.kind = e.kind;
      conn.edgeLabel = e.label ?? "";
      conn.stroke = e.style?.stroke || "#3d6b4f";
      conn.locked = !!e.locked;
      conn.onClick = () => emit("edgeClick", { edge: { id: e.id } });
      conn.onDblClick = () => emit("edgeDoubleClick", { edge: { id: e.id } });
      await editor.addConnection(conn);
    }
  } finally {
    syncing = false;
  }
}

async function markSelection() {
  if (!editor || !area) return;
  for (const n of editor.getNodes()) {
    const next = n.id === props.selectedId;
    if (n.selected === next) continue;
    n.selected = next;
    await area.update("node", n.id);
  }
}

async function fitView(opts?: { nodes?: string[] }) {
  if (!editor || !area) return;
  const all = editor.getNodes();
  const ids = opts?.nodes;
  const nodes = ids?.length ? all.filter((n) => ids.includes(n.id)) : all;
  if (!nodes.length) return;
  await AreaExtensions.zoomAt(area, nodes, { scale: 0.85 });
}

async function zoomBy(factor: number) {
  if (!area || !host.value) return;
  const rect = host.value.getBoundingClientRect();
  const k = area.area.transform.k;
  await area.area.zoom(k * factor, rect.width / 2, rect.height / 2);
}

onMounted(async () => {
  await createEditor();
  await rebuild();
});

onUnmounted(() => {
  destroyed = true;
  applyPickClass(null);
  area?.destroy();
  editor = null;
  area = null;
});

function graphKey(nodes: FlowCanvasNode[], edges: FlowCanvasEdge[]) {
  let s = `${nodes.length}:${edges.length}`;
  for (const n of nodes) {
    s += `|${n.id}:${n.position.x}:${n.position.y}:${n.data.kind}:${n.data.label}:${n.data.outline ?? ""}:${n.data.word_count ?? 0}:${n.data.volumeCollapsed ? 1 : 0}:${n.data.collapsedChapterCount ?? 0}`;
  }
  for (const e of edges) {
    s += `>${e.id}:${e.source}:${e.target}:${e.sourceHandle}:${e.targetHandle}:${e.label ?? ""}`;
  }
  return s;
}

watch(
  () => graphKey(props.nodes, props.edges),
  () => {
    if (draggingNode) return;
    void rebuild();
  },
);

watch(
  () => props.selectedId,
  () => {
    if (draggingNode) return;
    void markSelection();
  },
);

defineExpose({ fitView, zoomBy });
</script>

<template>
  <div class="relative h-full w-full">
    <div ref="host" class="rete-host h-full w-full" />
    <div class="absolute bottom-3 left-3 z-10 flex flex-col gap-1">
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded border bg-background/95 shadow-sm"
        :title="t('workspace.zoomIn')"
        :aria-label="t('workspace.zoomIn')"
        @click="zoomBy(1.2)"
      >
        <Plus class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded border bg-background/95 shadow-sm"
        :title="t('workspace.zoomOut')"
        :aria-label="t('workspace.zoomOut')"
        @click="zoomBy(1 / 1.2)"
      >
        <Minus class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        class="flex h-7 w-7 items-center justify-center rounded border bg-background/95 shadow-sm"
        :title="t('workspace.fitView')"
        :aria-label="t('workspace.fitView')"
        @click="fitView()"
      >
        <Maximize2 class="h-3.5 w-3.5" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.rete-host {
  position: relative;
  overflow: hidden;
  background-color: oklch(0.97 0.01 145);
  background-image: radial-gradient(#d5ddd4 1px, transparent 1px);
  background-size: 18px 18px;
}
.rete-host :deep(> div) {
  will-change: transform;
}
</style>
<style>
.rete-pick-out .story-port-hit--out {
  pointer-events: none !important;
}
.rete-pick-in .story-port-hit--in {
  pointer-events: none !important;
}
</style>
