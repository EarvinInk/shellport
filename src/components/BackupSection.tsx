import { DatabaseBackup, FileUp, Lock } from "lucide-react";
import { useState } from "react";
import { api, errMsg } from "../lib/ipc";
import type { BackupSummary } from "../lib/types";
import { useApp } from "../store";
import { Button, Checkbox, Field, Input } from "./ui";

/** Settings section: export everything to one encrypted file, or restore from one. */
export function BackupSection() {
  const { vault, toast, error, setUnlockPrompt, refreshHosts, refreshDbs, refreshSecrets, refreshLibrary, loadSettings } = useApp();

  // Export
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [withSecrets, setWithSecrets] = useState(false);
  const [withHistory, setWithHistory] = useState(false);
  const [exporting, setExporting] = useState(false);
  const exportProblem =
    pass.length < 8 ? "Use a passphrase of at least 8 characters" : pass !== pass2 ? "The passphrases don't match" : withSecrets && !vault.unlocked ? "Unlock the vault to include secrets" : null;

  const doExport = async () => {
    setExporting(true);
    try {
      const path = await api.backupExport(pass, withSecrets, withHistory);
      if (path) {
        toast(`Backup saved to ${path}`, "success");
        setPass("");
        setPass2("");
      }
    } catch (e) {
      error(e);
    } finally {
      setExporting(false);
    }
  };

  // Restore
  const [file, setFile] = useState<string | null>(null);
  const [rpass, setRpass] = useState("");
  const [summary, setSummary] = useState<BackupSummary | null>(null);
  const [replaceSettings, setReplaceSettings] = useState(false);
  const [busy, setBusy] = useState(false);
  const [restoreErr, setRestoreErr] = useState<string | null>(null);

  const pick = async () => {
    const f = await api.backupPick().catch((e) => (error(e), null));
    if (f) {
      setFile(f);
      setSummary(null);
      setRestoreErr(null);
    }
  };
  const preview = async () => {
    if (!file) return;
    setBusy(true);
    setRestoreErr(null);
    try {
      setSummary(await api.backupPreview(file, rpass));
    } catch (e) {
      setRestoreErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const restore = async () => {
    if (!file) return;
    setBusy(true);
    setRestoreErr(null);
    try {
      const r = await api.backupImport(file, rpass, replaceSettings);
      await Promise.all([refreshHosts(), refreshDbs(), refreshSecrets(), refreshLibrary(), r.settings ? loadSettings() : Promise.resolve()]);
      toast(
        `Restored ${r.hosts} host${r.hosts === 1 ? "" : "s"}, ${r.databases} database${r.databases === 1 ? "" : "s"}` +
          (r.secrets ? `, ${r.secrets} secret${r.secrets === 1 ? "" : "s"}` : "") +
          (r.commands ? `, ${r.commands} command${r.commands === 1 ? "" : "s"}` : "") +
          (r.workspaces ? `, ${r.workspaces} workspace${r.workspaces === 1 ? "" : "s"}` : "") +
          (r.history ? `, ${r.history} history entr${r.history === 1 ? "y" : "ies"}` : "") +
          (r.settings ? " and settings" : ""),
        "success",
      );
      setFile(null);
      setRpass("");
      setSummary(null);
    } catch (e) {
      const msg = errMsg(e);
      setRestoreErr(msg);
      if (/unlock the vault/i.test(msg)) setUnlockPrompt(true);
    } finally {
      setBusy(false);
    }
  };

  const secretsBlocked = summary?.secrets && summary.secrets.total > 0 && (!summary.vaultExists || !vault.unlocked);

  return (
    <section className="space-y-4">
      <div>
        <h2 className="text-sm font-semibold">Backup & restore</h2>
        <p className="mt-1 text-xs text-fg-muted">
          One encrypted file with your hosts, database connections and settings, for moving to another machine or keeping a copy. It never leaves this computer unless you move it.
        </p>
      </div>

      <div className="space-y-3 rounded-lg border border-border p-4">
        <div className="flex items-center gap-2 text-sm font-medium">
          <DatabaseBackup size={15} /> Create a backup
        </div>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Backup passphrase" hint="Needed to restore. It can't be recovered.">
            <Input type="password" value={pass} onChange={(e) => setPass(e.target.value)} autoComplete="new-password" />
          </Field>
          <Field label="Repeat passphrase">
            <Input type="password" value={pass2} onChange={(e) => setPass2(e.target.value)} autoComplete="new-password" />
          </Field>
        </div>
        <p className="text-xs text-fg-subtle">Always includes hosts, database connections, saved commands, workspaces and settings.</p>
        <Checkbox label="Include vault secrets (passwords and keys)" checked={withSecrets} onChange={setWithSecrets} />
        <Checkbox label="Include activity & history (queries, runs, transfers)" checked={withHistory} onChange={setWithHistory} />
        {withSecrets && !vault.unlocked && (
          <div className="flex items-center gap-2 text-xs text-warn">
            <Lock size={12} /> The vault is locked.
            <button className="underline cursor-pointer" onClick={() => setUnlockPrompt(true)}>
              Unlock it
            </button>
          </div>
        )}
        <div className="flex items-center gap-3">
          <Button variant="primary" onClick={doExport} disabled={!!exportProblem || exporting}>
            {exporting ? "Encrypting…" : "Save backup…"}
          </Button>
          {exportProblem && (pass || pass2 || withSecrets) && <span className="text-xs text-fg-subtle">{exportProblem}</span>}
        </div>
      </div>

      <div className="space-y-3 rounded-lg border border-border p-4">
        <div className="flex items-center gap-2 text-sm font-medium">
          <FileUp size={15} /> Restore from a backup
        </div>
        <div className="flex items-center gap-3">
          <Button onClick={pick}>Choose file…</Button>
          <span className="min-w-0 truncate text-xs text-fg-muted" title={file ?? undefined}>
            {file ?? "No file chosen"}
          </span>
        </div>
        {file && (
          <div className="flex items-end gap-3">
            <Field label="Backup passphrase" className="flex-1">
              <Input
                type="password"
                value={rpass}
                onChange={(e) => {
                  setRpass(e.target.value);
                  setSummary(null);
                }}
                onKeyDown={(e) => e.key === "Enter" && rpass && preview()}
              />
            </Field>
            <Button onClick={preview} disabled={!rpass || busy}>
              {busy && !summary ? "Decrypting…" : "Check"}
            </Button>
          </div>
        )}
        {restoreErr && <div className="rounded-md bg-danger/10 p-2 text-xs text-danger">{restoreErr}</div>}
        {summary && (
          <div className="space-y-3">
            <div className="rounded-md bg-surface-1 p-3 text-xs text-fg-muted">
              <div className="mb-1 text-fg">
                Made {new Date(summary.createdAt * 1000).toLocaleString()} by {summary.app}
              </div>
              <ul className="space-y-0.5">
                <li>
                  Hosts: {summary.hosts.new} new of {summary.hosts.total}
                </li>
                <li>
                  Database connections: {summary.databases.new} new of {summary.databases.total}
                </li>
                <li>
                  Saved commands: {summary.commands}, workspaces: {summary.workspaces}
                </li>
                {summary.history > 0 && <li>History entries: {summary.history}</li>}
                <li>{summary.secrets ? `Vault secrets: ${summary.secrets.new} new of ${summary.secrets.total}` : "No vault secrets in this backup"}</li>
              </ul>
              <p className="mt-2">Items you already have are kept as they are; only new ones are added.</p>
            </div>
            {secretsBlocked && (
              <div className="flex items-center gap-2 text-xs text-warn">
                <Lock size={12} />
                {summary.vaultExists ? "Unlock the vault to restore its secrets." : "Create a vault (Vault tab) to restore its secrets."}
                {summary.vaultExists && (
                  <button className="underline cursor-pointer" onClick={() => setUnlockPrompt(true)}>
                    Unlock
                  </button>
                )}
              </div>
            )}
            <Checkbox label="Also replace my settings with the backup's" checked={replaceSettings} onChange={setReplaceSettings} />
            <Button variant="primary" onClick={restore} disabled={busy || !!secretsBlocked}>
              {busy ? "Restoring…" : "Restore"}
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}
