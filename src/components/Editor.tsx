import { lazy, Suspense } from "react";

/** What callers can do with an open editor. */
export interface EditorHandle {
  focus: () => void;
  /** The selected text ("" when nothing is selected). */
  selection: () => string;
  /** Replace the selection (or insert at the cursor). */
  insert: (text: string) => void;
}

export interface EditorProps {
  value: string;
  onChange?: (text: string) => void;
  /** "sql", "markdown", or a stream mode name; otherwise picked from `fileName`. */
  language?: string;
  fileName?: string;
  readOnly?: boolean;
  /** Ctrl+S. */
  onSave?: () => void;
  /** Ctrl+Enter, with the selected text ("" for none). */
  onRun?: (selection: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  /** Soft-wrap long lines. */
  wrap?: boolean;
  className?: string;
}

const Impl = lazy(() => import("./EditorImpl"));

/** The shared code editor (CodeMirror 6, loaded on first use). */
export function Editor({ ref, ...props }: EditorProps & { ref?: React.Ref<EditorHandle> }) {
  return (
    <Suspense fallback={<div className={props.className} style={{ background: "var(--color-surface-0)" }} />}>
      <Impl {...props} ref={ref} />
    </Suspense>
  );
}
