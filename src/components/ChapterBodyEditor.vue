<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import {
  Compartment,
  EditorSelection,
  EditorState,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  drawSelection,
  EditorView,
  gutter,
  GutterMarker,
  keymap,
  placeholder as cmPlaceholder,
  type BlockInfo,
  type ViewUpdate,
} from "@codemirror/view";

const LINE = 32;
const SPARKLE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9.9 2.2 11 7.1a2 2 0 0 0 1.5 1.5l4.9 1.1-4.9 1.1A2 2 0 0 0 11 12.3L9.9 17.2 8.8 12.3a2 2 0 0 0-1.5-1.5L2.4 9.7l4.9-1.1A2 2 0 0 0 8.8 7.1z"/><path d="M19 13.5v3"/><path d="M17.5 15h3"/><path d="M19 3v2"/><path d="M18 4h2"/></svg>';

const props = withDefaults(
  defineProps<{
    modelValue: string;
    placeholder?: string;
    disabled?: boolean;
    nouns?: string[];
    rewriteLabel?: string;
    rewriteDisabled?: boolean;
  }>(),
  {
    placeholder: "",
    disabled: false,
    nouns: () => [],
    rewriteLabel: "",
    rewriteDisabled: false,
  },
);

const emit = defineEmits<{
  "update:modelValue": [string];
  rewrite: [index: number];
}>();

const hostEl = ref<HTMLElement | null>(null);

let view: EditorView | null = null;
const editComp = new Compartment();
const phComp = new Compartment();
const nounComp = new Compartment();
let nounsLive: string[] = [];
let rewriteLabelLive = "";
let rewriteDisabledLive = false;

const setHoveredLine = StateEffect.define<number | null>();
const hoveredLine = StateField.define<number | null>({
  create: () => null,
  update(value, tr) {
    for (const e of tr.effects) {
      if (e.is(setHoveredLine)) return e.value;
    }
    return value;
  },
});

class RewriteMarker extends GutterMarker {
  constructor(
    readonly lineIndex: number,
    readonly hot: boolean,
  ) {
    super();
  }
  eq(other: RewriteMarker) {
    return other.lineIndex === this.lineIndex && other.hot === this.hot;
  }
  toDOM() {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "cm-para-rewrite-btn" + (this.hot ? " is-hot" : "");
    btn.title = rewriteLabelLive;
    btn.setAttribute("aria-label", rewriteLabelLive);
    btn.innerHTML = SPARKLE_SVG;
    btn.addEventListener("mousedown", (e) => {
      e.preventDefault();
      e.stopPropagation();
    });
    btn.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (rewriteDisabledLive) return;
      emit("rewrite", this.lineIndex);
    });
    return btn;
  }
}

function rewriteGutter() {
  return gutter({
    class: "cm-para-rewrite",
    renderEmptyElements: false,
    lineMarker(view, line: BlockInfo) {
      const docLine = view.state.doc.lineAt(line.from);
      if (line.from !== docLine.from) return null;
      if (!docLine.text.trim()) return null;
      const hot = view.state.field(hoveredLine) === docLine.number;
      return new RewriteMarker(docLine.number - 1, hot);
    },
    lineMarkerChange(update) {
      return update.transactions.some((tr) => tr.effects.some((e) => e.is(setHoveredLine)));
    },
  });
}

function nounSource(context: CompletionContext) {
  if (context.view?.composing) return null;
  const word = context.matchBefore(/[\u4e00-\u9fffA-Za-z0-9_·]{1,24}$/);
  if (!word || (word.from === word.to && !context.explicit)) return null;
  const q = word.text;
  const options = nounsLive
    .filter((n) => n.includes(q))
    .slice(0, 24)
    .map((label) => ({ label, type: "text" as const }));
  if (!options.length) return null;
  return { from: word.from, options };
}

function nounExt() {
  return autocompletion({
    override: [nounSource],
    icons: false,
    activateOnTyping: true,
  });
}

const paperTheme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "transparent" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: 'ui-serif, "Songti SC", "Noto Serif SC", "Source Han Serif SC", Georgia, serif',
    lineHeight: `${LINE}px`,
  },
  ".cm-gutters": {
    backgroundColor: "transparent",
    border: "none",
  },
  ".cm-para-rewrite": {
    width: "22px",
  },
  ".cm-para-rewrite .cm-gutterElement": {
    padding: "0",
  },
  ".cm-para-rewrite-btn": {
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    width: "20px",
    height: `${LINE}px`,
    margin: "0",
    padding: "0",
    border: "none",
    borderRadius: "4px",
    background: "transparent",
    color: "#9b2c3c",
    opacity: "0",
    cursor: "pointer",
  },
  ".cm-para-rewrite-btn.is-hot, .cm-para-rewrite-btn:hover": {
    opacity: "1",
    background: "rgba(180, 70, 60, 0.12)",
  },
  ".cm-content": {
    caretColor: "#9b2c3c",
    fontSize: "16px",
    lineHeight: `${LINE}px`,
    padding: "8px 28px 72px 16px",
    minHeight: "100%",
    borderLeft: "1px solid rgba(196, 64, 54, 0.55)",
    backgroundImage: `repeating-linear-gradient(to bottom, transparent 0, transparent ${LINE - 1}px, rgba(70, 100, 160, 0.22) ${LINE - 1}px, rgba(70, 100, 160, 0.22) ${LINE}px)`,
    backgroundPosition: "0 8px",
  },
  ".cm-line": { padding: "0" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "#9b2c3c" },
  ".cm-selectionBackground": { background: "rgba(180, 70, 60, 0.16)" },
  "&.cm-focused .cm-selectionBackground": { background: "rgba(180, 70, 60, 0.22)" },
  ".cm-tooltip-autocomplete": {
    fontFamily: "ui-sans-serif, system-ui, sans-serif",
    fontSize: "13px",
  },
});

function onUpdate(u: ViewUpdate) {
  if (u.docChanged) {
    emit("update:modelValue", u.state.doc.toString());
  }
}

function hoverLineAt(clientX: number, clientY: number) {
  if (!view || rewriteDisabledLive) {
    setHover(null);
    return;
  }
  const pos = view.posAtCoords({ x: clientX, y: clientY });
  if (pos == null) {
    setHover(null);
    return;
  }
  const line = view.state.doc.lineAt(pos);
  setHover(line.text.trim() ? line.number : null);
}

function setHover(n: number | null) {
  if (!view) return;
  if (view.state.field(hoveredLine) === n) return;
  view.dispatch({ effects: setHoveredLine.of(n) });
}

function onMouseMove(ev: MouseEvent) {
  hoverLineAt(ev.clientX, ev.clientY);
}

function onMouseLeave() {
  setHover(null);
}

function mountEditor() {
  const parent = hostEl.value;
  if (!parent) return;
  nounsLive = props.nouns;
  rewriteLabelLive = props.rewriteLabel;
  rewriteDisabledLive = props.rewriteDisabled;
  view = new EditorView({
    parent,
    state: EditorState.create({
      doc: props.modelValue,
      extensions: [
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        EditorView.lineWrapping,
        drawSelection(),
        EditorView.contentAttributes.of({ spellcheck: "false" }),
        hoveredLine,
        rewriteGutter(),
        paperTheme,
        editComp.of(EditorView.editable.of(!props.disabled)),
        phComp.of(cmPlaceholder(props.placeholder)),
        nounComp.of(nounExt()),
        EditorView.updateListener.of(onUpdate),
      ],
    }),
  });
  view.dom.addEventListener("mousemove", onMouseMove);
  view.dom.addEventListener("mouseleave", onMouseLeave);
}

onMounted(() => {
  mountEditor();
});

onBeforeUnmount(() => {
  view?.dom.removeEventListener("mousemove", onMouseMove);
  view?.dom.removeEventListener("mouseleave", onMouseLeave);
  view?.destroy();
  view = null;
});

watch(
  () => props.modelValue,
  (text) => {
    if (!view) return;
    if (view.state.doc.toString() === text) return;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: text },
    });
  },
);

watch(
  () => props.disabled,
  (off) => {
    view?.dispatch({ effects: editComp.reconfigure(EditorView.editable.of(!off)) });
  },
);

watch(
  () => props.placeholder,
  (ph) => {
    view?.dispatch({ effects: phComp.reconfigure(cmPlaceholder(ph)) });
  },
);

watch(
  () => props.nouns,
  (list) => {
    nounsLive = list;
    view?.dispatch({ effects: nounComp.reconfigure(nounExt()) });
  },
);

watch(
  () => props.rewriteLabel,
  (label) => {
    rewriteLabelLive = label;
  },
);

watch(
  () => props.rewriteDisabled,
  (off) => {
    rewriteDisabledLive = off;
    if (off) setHover(null);
  },
);

function getCursor(): number {
  return view?.state.selection.main.head ?? 0;
}

function setSelection(from: number, to = from) {
  if (!view) return;
  const len = view.state.doc.length;
  const a = Math.max(0, Math.min(from, len));
  const b = Math.max(a, Math.min(to, len));
  view.dispatch({
    selection: a === b ? EditorSelection.cursor(a) : EditorSelection.range(a, b),
    scrollIntoView: true,
  });
  view.focus();
}

function focus() {
  view?.focus();
}

defineExpose({ getCursor, setSelection, focus });
</script>

<template>
  <div class="chapter-paper relative min-h-0 w-full flex-1">
    <div class="chapter-paper-sheet relative h-full min-h-0 overflow-hidden rounded-md">
      <div ref="hostEl" class="absolute inset-0 min-h-0" />
    </div>
  </div>
</template>

<style scoped>
.chapter-paper-sheet {
  background-color: #f3ead6;
  box-shadow:
    inset 0 0 0 1px rgba(120, 90, 40, 0.14),
    0 1px 2px rgba(80, 50, 20, 0.06);
}
:global(.dark) .chapter-paper-sheet {
  background-color: #2c261c;
  box-shadow: inset 0 0 0 1px rgba(220, 190, 140, 0.12);
}
:deep(.cm-editor) {
  height: 100%;
}
:global(.dark) .chapter-paper :deep(.cm-content) {
  caret-color: #e8c9a8;
  border-left-color: rgba(220, 110, 90, 0.45);
  background-image: repeating-linear-gradient(
    to bottom,
    transparent 0,
    transparent 31px,
    rgba(210, 190, 150, 0.16) 31px,
    rgba(210, 190, 150, 0.16) 32px
  );
  background-position: 0 8px;
}
:global(.dark) .chapter-paper :deep(.cm-cursor) {
  border-left-color: #e8c9a8;
}
:global(.dark) .chapter-paper :deep(.cm-para-rewrite-btn) {
  color: #e8c9a8;
}
</style>
