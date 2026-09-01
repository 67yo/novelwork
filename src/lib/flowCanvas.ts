import { ClassicPreset } from "rete";
import type { TreeNode } from "@/lib/api";
import { socketsForKind } from "@/lib/flowSockets";

const port = new ClassicPreset.Socket("port");

export type StoryNodeData = TreeNode & {
  cover_url?: string;
  volumeCollapsed?: boolean;
  collapsedChapterCount?: number;
  toggleVolumeCollapse?: (id: string) => void;
};

export type FlowCanvasNode = {
  id: string;
  position: { x: number; y: number };
  selected?: boolean;
  draggable?: boolean;
  data: StoryNodeData;
};

export type FlowCanvasEdge = {
  id: string;
  source: string;
  target: string;
  sourceHandle: string;
  targetHandle: string;
  kind: string;
  label?: string;
  locked?: boolean;
  style?: { stroke?: string };
};

export type FlowConnect = {
  source: string;
  target: string;
  sourceHandle: string | null;
  targetHandle: string | null;
};

export class StoryReteNode extends ClassicPreset.Node {
  payload: StoryNodeData;
  width = 220;
  height = 110;
  constructor(payload: StoryNodeData) {
    super(payload.label || payload.kind);
    this.id = payload.id;
    this.payload = payload;
    for (const key of socketsForKind(payload.kind)) {
      this.addInput(key, new ClassicPreset.Input(port, key, true));
      this.addOutput(key, new ClassicPreset.Output(port, key, true));
    }
  }
}

export class StoryConnection extends ClassicPreset.Connection<StoryReteNode, StoryReteNode> {
  kind = "chapter";
  edgeLabel = "";
  stroke = "#3d6b4f";
  locked = false;
  onClick?: () => void;
  onDblClick?: () => void;
}
