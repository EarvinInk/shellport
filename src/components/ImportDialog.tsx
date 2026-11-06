import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import type { SshConfigEntry } from "../lib/types";
import { useApp } from "../store";
import { Button, Input, Modal } from "./ui";

export function ImportDialog() {
  const { importOpen, setImportOpen, refreshHosts, toast, error, hosts } = useApp();
  const [path, setPath] = useState("~/.ssh/config");
  const [entries, setEntries] = useState<SshConfigEntry[]>([]);
  const [sel, setSel] = useState<Set<string>>(new Set());

  const load = async (p: string) => {
    try {
      const e = await api.sshConfigPreview(p);
      setEntries(e);
      const existing = new Set(hosts.map((h) => h.name));
      setSel(new Set(e.filter((x) => !existing.has(x.alias)).map((x) => x.alias)));
    } catch (e) {
      error(e);
    }
  };

  useEffect(() => {
    if (importOpen) load(path);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [importOpen]);

  if (!importOpen) return null;
  const existing = new Set(hosts.map((h) => h.name));

  const doImport = async () => {
    try {
      const n = await api.sshConfigImport([...sel], path);
      await refreshHosts();
      toast(`Imported ${n} host${n === 1 ? "" : "s"}`, "success");
      setImportOpen(false);
    } catch (e) {
      error(e);
    }
  };

  return (
    <Modal
      title="Import from SSH config"
      onClose={() => setImportOpen(false)}
      width="max-w-2xl"
      footer={
        <>
          <Button variant="ghost" onClick={() => setImportOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" disabled={sel.size === 0} onClick={doImport}>
            Import {sel.size}
          </Button>
        </>
      }
    >
      <div className="mb-3 flex gap-2">
        <Input value={path} onChange={(e) => setPath(e.target.value)} />
        <Button onClick={() => load(path)}>Load</Button>
      </div>
      {entries.length === 0 ? (
        <div className="py-6 text-center text-sm text-fg-subtle">No concrete Host entries found.</div>
      ) : (
        <table className="w-full text-sm">
          <thead className="text-left text-xs text-fg-subtle">
            <tr>
              <th className="w-8 py-1">
                <input
                  type="checkbox"
                  checked={sel.size === entries.length}
                  onChange={(e) => setSel(e.target.checked ? new Set(entries.map((x) => x.alias)) : new Set())}
                />
              </th>
              <th>Alias</th>
              <th>Target</th>
              <th>Key</th>
              <th>Jump</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((e) => (
              <tr key={e.alias} className="border-t border-border">
                <td className="py-1.5">
                  <input
                    type="checkbox"
                    checked={sel.has(e.alias)}
                    onChange={(ev) => {
                      const s = new Set(sel);
                      if (ev.target.checked) s.add(e.alias);
                      else s.delete(e.alias);
                      setSel(s);
                    }}
                  />
                </td>
                <td>
                  {e.alias}
                  {existing.has(e.alias) && <span className="ml-2 text-xs text-fg-subtle">(exists)</span>}
                </td>
                <td className="text-fg-muted">
                  {e.user ? `${e.user}@` : ""}
                  {e.hostname ?? e.alias}
                  {e.port && e.port !== 22 ? `:${e.port}` : ""}
                </td>
                <td className="max-w-40 truncate text-fg-muted">{e.identityFile ?? "agent"}</td>
                <td className="text-fg-muted">{e.proxyJump ?? ""}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Modal>
  );
}
