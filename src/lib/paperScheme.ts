import { ref } from "vue";

export const PAPER_SCHEME_LIGHT = ["white", "cream", "green", "blue"] as const;
export const PAPER_SCHEME_DARK = ["night", "charcoal", "forest", "midnight"] as const;
export const PAPER_SCHEME_IDS = [...PAPER_SCHEME_LIGHT, ...PAPER_SCHEME_DARK] as const;
export type PaperSchemeId = (typeof PAPER_SCHEME_IDS)[number];

export type PaperSchemeVars = {
  paper: string;
  ink: string;
  muted: string;
  border: string;
  wash: string;
  panel: string;
};

const SCHEMES: Record<PaperSchemeId, PaperSchemeVars> = {
  white: {
    paper: "#ffffff",
    ink: "#111111",
    muted: "#737373",
    border: "#d4d4d4",
    wash: "rgba(0, 0, 0, 0.06)",
    panel: "#fafafa",
  },
  cream: {
    paper: "#f6f0e4",
    ink: "#3d2b1f",
    muted: "#8a7a68",
    border: "#e0d4b8",
    wash: "rgba(80, 50, 20, 0.08)",
    panel: "#efe6d4",
  },
  green: {
    paper: "#eef6e6",
    ink: "#1e2e18",
    muted: "#6b7d62",
    border: "#c5d6b4",
    wash: "rgba(40, 70, 20, 0.08)",
    panel: "#e4efd8",
  },
  blue: {
    paper: "#eef3f8",
    ink: "#1a2430",
    muted: "#6b7785",
    border: "#c5d0dc",
    wash: "rgba(20, 40, 70, 0.08)",
    panel: "#e4ebf2",
  },
  night: {
    paper: "#141414",
    ink: "#f0f0f0",
    muted: "#a3a3a3",
    border: "#3f3f3f",
    wash: "rgba(255, 255, 255, 0.08)",
    panel: "#1c1c1c",
  },
  charcoal: {
    paper: "#1c1916",
    ink: "#ebe4d8",
    muted: "#a89f90",
    border: "#4a433a",
    wash: "rgba(255, 236, 210, 0.08)",
    panel: "#25211c",
  },
  forest: {
    paper: "#152016",
    ink: "#dce8d4",
    muted: "#8fa086",
    border: "#3a4a38",
    wash: "rgba(210, 240, 200, 0.08)",
    panel: "#1c2a1d",
  },
  midnight: {
    paper: "#141a22",
    ink: "#d8e2ee",
    muted: "#8b97a6",
    border: "#3a4554",
    wash: "rgba(200, 220, 245, 0.08)",
    panel: "#1b232e",
  },
};

const STORAGE_KEY = "novework.paperScheme";

export function parsePaperScheme(raw: string | null | undefined): PaperSchemeId {
  return PAPER_SCHEME_IDS.includes(raw as PaperSchemeId) ? (raw as PaperSchemeId) : "white";
}

export function paperSchemeVars(id: PaperSchemeId): PaperSchemeVars {
  return SCHEMES[id];
}

export function paperSchemeStyle(id: PaperSchemeId): Record<string, string> {
  const s = SCHEMES[id];
  return {
    "--cb-paper": s.paper,
    "--cb-ink": s.ink,
    "--cb-muted": s.muted,
    "--cb-border": s.border,
    "--cb-wash": s.wash,
    "--cb-panel": s.panel,
  };
}

function readStored(): PaperSchemeId {
  try {
    return parsePaperScheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "white";
  }
}

const paperScheme = ref<PaperSchemeId>(readStored());

export function usePaperScheme() {
  return {
    scheme: paperScheme,
    setScheme(id: PaperSchemeId) {
      const next = parsePaperScheme(id);
      paperScheme.value = next;
      try {
        localStorage.setItem(STORAGE_KEY, next);
      } catch {
        /* ignore quota / private mode */
      }
    },
  };
}
