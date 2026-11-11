import { ask } from "@tauri-apps/plugin-dialog";
import { Copy, Eye, Pencil, SquareTerminal } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { pasteToHost } from "../lib/terminalActions";
import { useApp } from "../store";
import { Markdown } from "./Markdown";
import { Editor } from "./Editor";
import { Button, Modal } from "./ui";

const TEMPLATE = `## About this server

What runs here, who owns it, anything worth knowing.

## Useful commands

\`\`\`
sudo systemctl status nginx
\`\`\`
`;

/** Markdown notes for one host; code blocks can be copied or pasted into its terminal. */
export function NotesDialog() {
  const { notesHost, setNotesHost, hosts, refreshHosts, toast, error } = useApp();
  const host = hosts.find((h) => h.id === notesHost);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState("");

  useEffect(() => {
    setText(host?.notes ?? "");
    setEditing(!host?.notes);
  }, [notesHost]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!host) return null;
  const dirty = text !== (host.notes ?? "");

  const save = async () => {
    try {
      await api.hostSave({ ...host, notes: text });
      await refreshHosts();
      setEditing(false);
      toast("Notes saved", "success");
    } catch (e) {
      error(e);
    }
  };

  const close = async () => {
    if (dirty && !(await ask("Discard unsaved changes to these notes?", { title: "Discard changes?", kind: "warning", okLabel: "Discard" }))) return;
    setNotesHost(null);
  };

  return (
    <Modal
      title={`${host.name} · notes`}
      onClose={close}
      width="max-w-3xl"
      footer={
        <>
          <span className="mr-auto text-xs text-fg-subtle">Markdown. Code blocks get Copy and Paste-in-terminal buttons. Included in backups.</span>
          {editing ? (
            <>
              {host.notes && (
                <Button variant="ghost" onClick={() => setEditing(false)}>
                  <Eye size={14} /> Preview
                </Button>
              )}
              <Button variant="primary" onClick={save} disabled={!dirty}>
                Save
              </Button>
            </>
          ) : (
            <Button onClick={() => setEditing(true)}>
              <Pencil size={14} /> Edit
            </Button>
          )}
        </>
      }
    >
      {editing ? (
        <Editor
          autoFocus
          value={text}
          onChange={setText}
          onSave={() => dirty && save()}
          language="markdown"
          wrap
          placeholder={TEMPLATE}
          className="h-96 overflow-hidden rounded-md border border-border"
        />
      ) : (
        <Markdown
          text={text}
          codeActions={(code) => (
            <>
              <Button size="sm" variant="subtle" className="h-6 px-2 text-[11px]" onClick={() => navigator.clipboard.writeText(code).then(() => toast("Copied"))}>
                <Copy size={11} /> Copy
              </Button>
              <Button
                size="sm"
                variant="subtle"
                className="h-6 px-2 text-[11px]"
                title="Paste into this host's terminal (does not press Enter)"
                onClick={async () => {
                  setNotesHost(null);
                  if (!(await pasteToHost(host, code))) toast("Couldn't reach a terminal on this host", "error");
                }}
              >
                <SquareTerminal size={11} /> Paste in terminal
              </Button>
            </>
          )}
        />
      )}
    </Modal>
  );
}
