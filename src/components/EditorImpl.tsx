// The shared text editor (M3.2), built on CodeMirror 6. Loaded on first use
// through `Editor.tsx`, so the app starts without it.
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { sql } from "@codemirror/lang-sql";
import { markdown } from "@codemirror/lang-markdown";
import { bracketMatching, foldGutter, HighlightStyle, indentOnInput, LanguageSupport, StreamLanguage, syntaxHighlighting, type StreamParser } from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, highlightActiveLineGutter, keymap, lineNumbers, placeholder as placeholderExt } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { useEffect, useImperativeHandle, useRef } from "react";
import type { EditorHandle, EditorProps } from "./Editor";

const theme = EditorView.theme(
  {
    "&": { color: "var(--color-fg)", backgroundColor: "var(--color-surface-0)", height: "100%", fontSize: "13px" },
    ".cm-scroller": { fontFamily: "'JetBrains Mono', ui-monospace, monospace", lineHeight: "1.55" },
    ".cm-content": { caretColor: "var(--color-accent)", padding: "8px 0" },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--color-accent)" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "color-mix(in srgb, var(--color-accent) 30%, transparent)" },
    ".cm-gutters": { backgroundColor: "var(--color-surface-1)", color: "var(--color-fg-subtle)", borderRight: "1px solid var(--color-border)" },
    ".cm-activeLineGutter": { backgroundColor: "var(--color-surface-2)", color: "var(--color-fg-muted)" },
    ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--color-surface-3) 45%, transparent)" },
    ".cm-selectionMatch": { backgroundColor: "color-mix(in srgb, var(--color-accent) 15%, transparent)" },
    ".cm-matchingBracket": { backgroundColor: "var(--color-surface-4)", outline: "none" },
    ".cm-placeholder": { color: "var(--color-fg-subtle)" },
    ".cm-panels": { backgroundColor: "var(--color-surface-1)", color: "var(--color-fg)", borderTop: "1px solid var(--color-border)" },
    ".cm-panels input, .cm-panels button": { fontSize: "12px" },
    ".cm-textfield": { backgroundColor: "var(--color-surface-2)", border: "1px solid var(--color-border)", color: "var(--color-fg)", borderRadius: "4px" },
    ".cm-button": { backgroundImage: "none", backgroundColor: "var(--color-surface-3)", border: "1px solid var(--color-border)", color: "var(--color-fg)", borderRadius: "4px" },
    ".cm-searchMatch": { backgroundColor: "color-mix(in srgb, var(--color-warn) 25%, transparent)" },
    ".cm-searchMatch-selected": { backgroundColor: "color-mix(in srgb, var(--color-warn) 45%, transparent)" },
    ".cm-foldPlaceholder": { backgroundColor: "var(--color-surface-3)", border: "none", color: "var(--color-fg-muted)" },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.operatorKeyword, t.modifier], color: "#c792ea" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "#a5d6a7" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "#f8c555" },
  { tag: [t.comment, t.lineComment, t.blockComment], color: "var(--color-fg-subtle)", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "#82aaff" },
  { tag: [t.typeName, t.className, t.namespace], color: "#ffcb6b" },
  { tag: [t.propertyName, t.attributeName], color: "#80cbc4" },
  { tag: [t.variableName, t.special(t.variableName)], color: "var(--color-fg)" },
  { tag: [t.definition(t.variableName), t.labelName], color: "#f78c6c" },
  { tag: [t.heading, t.strong], color: "#82aaff", fontWeight: "600" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: [t.link, t.url], color: "#89ddff", textDecoration: "underline" },
  { tag: [t.meta, t.processingInstruction], color: "#89ddff" },
  { tag: t.invalid, color: "var(--color-danger)" },
]);

/** Legacy (stream) modes, loaded only when a file needs one. */
const legacy: Record<string, () => Promise<StreamParser<unknown>>> = {
  shell: () => import("@codemirror/legacy-modes/mode/shell").then((m) => m.shell as StreamParser<unknown>),
  yaml: () => import("@codemirror/legacy-modes/mode/yaml").then((m) => m.yaml as StreamParser<unknown>),
  toml: () => import("@codemirror/legacy-modes/mode/toml").then((m) => m.toml as StreamParser<unknown>),
  nginx: () => import("@codemirror/legacy-modes/mode/nginx").then((m) => m.nginx as StreamParser<unknown>),
  dockerfile: () => import("@codemirror/legacy-modes/mode/dockerfile").then((m) => m.dockerFile as StreamParser<unknown>),
  properties: () => import("@codemirror/legacy-modes/mode/properties").then((m) => m.properties as StreamParser<unknown>),
  python: () => import("@codemirror/legacy-modes/mode/python").then((m) => m.python as StreamParser<unknown>),
  javascript: () => import("@codemirror/legacy-modes/mode/javascript").then((m) => m.javascript as StreamParser<unknown>),
  json: () => import("@codemirror/legacy-modes/mode/javascript").then((m) => m.json as StreamParser<unknown>),
  xml: () => import("@codemirror/legacy-modes/mode/xml").then((m) => m.html as StreamParser<unknown>),
  lua: () => import("@codemirror/legacy-modes/mode/lua").then((m) => m.lua as StreamParser<unknown>),
  go: () => import("@codemirror/legacy-modes/mode/go").then((m) => m.go as StreamParser<unknown>),
  rust: () => import("@codemirror/legacy-modes/mode/rust").then((m) => m.rust as StreamParser<unknown>),
  diff: () => import("@codemirror/legacy-modes/mode/diff").then((m) => m.diff as StreamParser<unknown>),
  css: () => import("@codemirror/legacy-modes/mode/css").then((m) => m.css as StreamParser<unknown>),
};

/** Which language a file name suggests ("sql" and "markdown" have full support; the rest are stream modes). */
export function languageFor(name: string): string | null {
  const n = name.toLowerCase().split("/").pop() ?? "";
  const ext = n.includes(".") ? n.slice(n.lastIndexOf(".") + 1) : "";
  if (n === "dockerfile" || n.startsWith("dockerfile.") || n.endsWith(".dockerfile")) return "dockerfile";
  if (/^(\.?(bash|zsh)rc|\.profile|\.bash_profile|\.bash_aliases|\.env.*)$/.test(n)) return n.startsWith(".env") ? "properties" : "shell";
  if (n === "nginx.conf" || n.includes("nginx")) return "nginx";
  const byExt: Record<string, string> = {
    sql: "sql", md: "markdown", markdown: "markdown",
    sh: "shell", bash: "shell", zsh: "shell", ksh: "shell",
    yml: "yaml", yaml: "yaml", toml: "toml",
    ini: "properties", conf: "properties", cfg: "properties", properties: "properties", env: "properties", service: "properties", timer: "properties",
    py: "python", js: "javascript", mjs: "javascript", cjs: "javascript", ts: "javascript", json: "json",
    xml: "xml", html: "xml", htm: "xml", svg: "xml", lua: "lua", go: "go", rs: "rust", diff: "diff", patch: "diff", css: "css",
  };
  return byExt[ext] ?? null;
}

async function languageExtension(lang: string | null | undefined): Promise<Extension> {
  if (!lang) return [];
  if (lang === "sql") return sql();
  if (lang === "markdown") return markdown();
  const load = legacy[lang];
  if (!load) return [];
  return new LanguageSupport(StreamLanguage.define(await load()));
}

export default function EditorImpl({ value, onChange, language, fileName, readOnly, onSave, onRun, placeholder, autoFocus, wrap, className, ref }: EditorProps & { ref?: React.Ref<EditorHandle> }) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const langComp = useRef(new Compartment());
  const roComp = useRef(new Compartment());
  const wrapComp = useRef(new Compartment());
  // Latest callbacks, read by the keymap without rebuilding the editor.
  const cb = useRef({ onChange, onSave, onRun });
  cb.current = { onChange, onSave, onRun };

  useEffect(() => {
    const keys = keymap.of([
      { key: "Mod-s", preventDefault: true, run: () => (cb.current.onSave?.(), true) },
      {
        key: "Mod-Enter",
        preventDefault: true,
        run: (v) => {
          if (!cb.current.onRun) return false;
          const sel = v.state.selection.main;
          cb.current.onRun(sel.empty ? "" : v.state.sliceDoc(sel.from, sel.to));
          return true;
        },
      },
      indentWithTab,
      ...defaultKeymap,
      ...historyKeymap,
      ...searchKeymap,
    ]);
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          lineNumbers(),
          highlightActiveLineGutter(),
          foldGutter(),
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          search({ top: true }),
          keys,
          theme,
          syntaxHighlighting(highlight),
          placeholder ? placeholderExt(placeholder) : [],
          langComp.current.of([]),
          roComp.current.of(EditorState.readOnly.of(!!readOnly)),
          wrapComp.current.of(wrap ? EditorView.lineWrapping : []),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) cb.current.onChange?.(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    if (autoFocus) v.focus();
    return () => {
      v.destroy();
      view.current = null;
    };
    // The editor is built once; later prop changes go through compartments below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Text replaced from outside (reload, history pick): swap it in without an undo step loop.
  useEffect(() => {
    const v = view.current;
    if (v && value !== v.state.doc.toString()) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } });
  }, [value]);

  useEffect(() => {
    let live = true;
    languageExtension(language ?? (fileName ? languageFor(fileName) : null)).then((ext) => {
      if (live) view.current?.dispatch({ effects: langComp.current.reconfigure(ext) });
    });
    return () => {
      live = false;
    };
  }, [language, fileName]);

  useEffect(() => {
    view.current?.dispatch({ effects: roComp.current.reconfigure(EditorState.readOnly.of(!!readOnly)) });
  }, [readOnly]);

  useEffect(() => {
    view.current?.dispatch({ effects: wrapComp.current.reconfigure(wrap ? EditorView.lineWrapping : []) });
  }, [wrap]);

  useImperativeHandle(ref, () => ({
    focus: () => view.current?.focus(),
    selection: () => {
      const v = view.current;
      if (!v) return "";
      const s = v.state.selection.main;
      return s.empty ? "" : v.state.sliceDoc(s.from, s.to);
    },
    insert: (text: string) => {
      const v = view.current;
      if (!v) return;
      v.dispatch(v.state.replaceSelection(text));
      v.focus();
    },
  }));

  return <div ref={host} className={className} style={{ minHeight: 0, overflow: "hidden" }} />;
}
