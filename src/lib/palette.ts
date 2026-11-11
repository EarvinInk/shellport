import type { ReactNode } from "react";

/** One thing the palette can do. */
export interface PaletteItem {
  /** Stable id, used to rank by how often it is chosen. */
  id: string;
  title: string;
  subtitle?: string;
  group: string;
  /** Extra words to match (tags, hostnames…). */
  keywords?: string;
  icon?: ReactNode;
  /** Shown with a Production badge. */
  production?: boolean;
  run: () => void;
}

type Source = () => PaletteItem[];
const sources = new Map<string, Source>();

/**
 * Features add their actions here instead of building their own menus
 * (app rule 3). Sources are called each time the palette opens.
 */
export function registerPaletteSource(id: string, source: Source): () => void {
  sources.set(id, source);
  return () => sources.delete(id);
}

export function paletteItems(): PaletteItem[] {
  return [...sources.values()].flatMap((s) => {
    try {
      return s();
    } catch {
      return [];
    }
  });
}

const USAGE_KEY = "shellport.palette.usage";

function readUsage(): Record<string, number> {
  try {
    return JSON.parse(localStorage.getItem(USAGE_KEY) ?? "{}");
  } catch {
    return {};
  }
}

export function recordUse(id: string) {
  try {
    const u = readUsage();
    u[id] = (u[id] ?? 0) + 1;
    localStorage.setItem(USAGE_KEY, JSON.stringify(u));
  } catch {
    /* storage unavailable */
  }
}

/** Fuzzy score: every query character in order; earlier, word-start and consecutive matches score higher. */
function fuzzy(query: string, text: string): number {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 1;
  const direct = t.indexOf(q);
  if (direct >= 0) return 100 - Math.min(direct, 50) + (direct === 0 || /\W/.test(t[direct - 1]) ? 30 : 0);
  let score = 0;
  let ti = 0;
  let streak = 0;
  for (const c of q) {
    const at = t.indexOf(c, ti);
    if (at < 0) return 0;
    streak = at === ti ? streak + 1 : 0;
    score += 1 + streak * 2 + (at === 0 || /\W/.test(t[at - 1]) ? 3 : 0);
    ti = at + 1;
  }
  return score;
}

/** Matching items, best first; with no query, most used first. */
export function searchPalette(items: PaletteItem[], query: string, limit = 60): PaletteItem[] {
  const usage = readUsage();
  return items
    .map((it) => {
      const s = Math.max(fuzzy(query, it.title) * 1.5, fuzzy(query, `${it.subtitle ?? ""} ${it.keywords ?? ""} ${it.group}`));
      return { it, s: s > 0 ? s + Math.log2(1 + (usage[it.id] ?? 0)) * 4 : 0 };
    })
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .slice(0, limit)
    .map((x) => x.it);
}
