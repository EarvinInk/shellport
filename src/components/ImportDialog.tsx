import { ask } from "@tauri-apps/plugin-dialog";
import { FileDown, FileUp } from "lucide-react";
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

  const importFile = async () => {
    try {
      const p = await api.localPickFile("Choose a hosts file (CSV or JSON)");
      if (!p) return;
      const pv = await api.hostsImportPreview(p);
      const fresh = pv.names.length - pv.existing.length;
      if (fresh === 0) return toast("Every host in that file is already here", "info");
      const list = pv.names.filter((n) => !pv.existing.includes(n));
      const ok = await ask(
        `Import ${fresh} host${fresh === 1 ? "" : "s"}?\n\n${list.slice(0, 12).join(", ")}${list.length > 12 ? `, and ${list.length - 12} more` : ""}` +
          (pv.existing.length ? `\n\n${pv.existing.length} already here will be skipped.` : "") +
          "\n\nPasswords and keys are not part of the file; choose them in each host's settings.",
        { title: "Import hosts", okLabel: "Import" },
      );
      if (!ok) return;
      const n = await api.hostsImportFile(p);
      await refreshHosts();
      toast(`Imported ${n} host${n === 1 ? "" : "s"}`, "success");
      setImportOpen(false);
    } catch (e) {
      error(e);
    }
  };
  const exportAs = (format: "json" | "csv") =>
    api
      .hostsExport(format)
      .then((p) => p && toast(`Exported to ${p}`, "success"))
      .catch(error);

  return (
    <Modal
      title="Import hosts"
      onClose={() => setImportOpen(false)}
      width="max-w-2xl"
      footer={
        <>
          <span className="mr-auto flex items-center gap-1 text-xs text-fg-subtle">
            Export all hosts:
            <Button size="sm" variant="ghost" onClick={() => exportAs("json")}>
              <FileDown size={12} /> JSON
            </Button>
            <Button size="sm" variant="ghost" onClick={() => exportAs("csv")}>
              <FileDown size={12} /> CSV
            </Button>
          </span>
          <Button variant="ghost" onClick={() => setImportOpen(false)}>
            Cancel
          </Button>
          <Button variant="primary" disabled={sel.size === 0} onClick={doImport}>
            Import {sel.size}
          </Button>
        </>
      }
    >
      <div className="mb-4 flex items-center gap-3 rounded-lg border border-border bg-surface-1 px-3 py-2 text-sm">
        <FileUp size={15} className="shrink-0 text-fg-subtle" />
        <span className="flex-1 text-fg-muted">Have a CSV or JSON list (from Shellport or a spreadsheet)?</span>
        <Button size="sm" onClick={importFile}>
          Import a file…
        </Button>
      </div>
      <div className="mb-1 text-xs font-semibold uppercase tracking-wider text-fg-subtle">From an SSH config file</div>
      <p className="mb-2 text-xs text-fg-subtle">Hosts imported from ~/.ssh/config stay in sync with it; edits in either place show up in both.</p>
      <div className="mb-3 flex gap-2">
        <Input value={path} readOnly />
        <Button
          onClick={async () => {
            try {
              const p = await api.localPickFile("Choose an SSH config file", "~/.ssh");
              if (!p) return;
              setPath(p);
              load(p);
            } catch (e) {
              error(e);
            }
          }}
        >
          Browse…
        </Button>
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
