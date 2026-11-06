import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderSearch, KeyRound, Lock, Play, Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errMsg } from "../lib/ipc";
import type { AuthMethod, Host, PortForward, SecretKind } from "../lib/types";
import { useApp } from "../store";
import { Button, Field, IconButton, Input, Modal, Select } from "./ui";

const blank = (): Host => ({
  id: "",
  name: "",
  group: "",
  hostname: "",
  port: 22,
  username: "",
  auth: { type: "agent" },
  jumpHostId: null,
  dockerSocket: null,
  tags: [],
  color: null,
  forwards: [],
  keepaliveSecs: null,
  startupCommand: null,
});

const COLORS = ["", "#14b8a6", "#3b82f6", "#a855f7", "#f43f5e", "#f59e0b", "#22c55e"];

export function HostEditor() {
  const { hostEditor, setHostEditor, hosts, refreshHosts, secrets, vault, setUnlockPrompt, toast, error, refreshSecrets } = useApp();
  const [h, setH] = useState<Host>(blank);
  const [inlinePw, setInlinePw] = useState("");
  const [testing, setTesting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    if (hostEditor === "new") setH(blank());
    else if (hostEditor) setH(structuredClone(hostEditor));
    setInlinePw("");
    setConfirmDelete(false);
  }, [hostEditor]);

  if (!hostEditor) return null;
  const isNew = hostEditor === "new";
  const set = <K extends keyof Host>(k: K, v: Host[K]) => setH((p) => ({ ...p, [k]: v }));
  const setAuth = (a: AuthMethod) => set("auth", a);
  const secretsOf = (kind: SecretKind) => secrets.filter((s) => s.kind === kind);

  const persist = async (): Promise<Host | null> => {
    try {
      let host = { ...h, tags: h.tags.filter(Boolean) };
      if (host.auth.type === "password" && inlinePw) {
        const meta = await api.secretSave({
          name: `${host.name || host.hostname} password`,
          kind: "password",
          username: host.username,
          value: inlinePw,
          notes: "",
        });
        host = { ...host, auth: { type: "password", secretId: meta.id } };
        await refreshSecrets();
      }
      const saved = await api.hostSave(host);
      await refreshHosts();
      setH(saved);
      return saved;
    } catch (e) {
      error(e);
      return null;
    }
  };

  const save = async () => {
    if (await persist()) {
      toast(`Saved ${h.name || h.hostname}`, "success");
      setHostEditor(null);
    }
  };

  const test = async () => {
    const saved = await persist();
    if (!saved) return;
    setTesting(true);
    try {
      const out = await api.hostTest(saved.id);
      toast(`Connected: ${out || "ok"}`, "success");
    } catch (e) {
      toast(`Connection failed: ${errMsg(e)}`, "error");
    } finally {
      setTesting(false);
    }
  };

  const del = async () => {
    try {
      await api.hostDelete(h.id);
      await refreshHosts();
      setHostEditor(null);
    } catch (e) {
      error(e);
    }
  };

  const browseKey = async () => {
    const f = await openDialog({ title: "Select private key", defaultPath: "~/.ssh", multiple: false, directory: false });
    if (typeof f === "string") setAuth({ type: "key_file", path: f, passphraseSecretId: h.auth.type === "key_file" ? h.auth.passphraseSecretId : null });
  };

  const updateFwd = (i: number, f: Partial<PortForward>) =>
    set(
      "forwards",
      h.forwards.map((x, j) => (j === i ? { ...x, ...f } : x)),
    );

  const vaultNeeded = (h.auth.type === "password" || h.auth.type === "vault_key" || (h.auth.type === "key_file" && h.auth.passphraseSecretId)) && !vault.unlocked;

  return (
    <Modal
      title={isNew ? "New host" : `Edit ${hostEditor.name}`}
      onClose={() => setHostEditor(null)}
      width="max-w-2xl"
      footer={
        <>
          {!isNew &&
            (confirmDelete ? (
              <Button variant="danger" onClick={del}>
                Confirm delete
              </Button>
            ) : (
              <Button variant="ghost" onClick={() => setConfirmDelete(true)}>
                <Trash2 size={14} /> Delete
              </Button>
            ))}
          <div className="flex-1" />
          <Button onClick={test} disabled={testing || !h.hostname}>
            <Play size={14} /> {testing ? "Testing…" : "Save & test"}
          </Button>
          <Button variant="primary" onClick={save} disabled={!h.hostname}>
            Save
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-6 gap-3">
        <Field label="Name" className="col-span-3">
          <Input value={h.name} onChange={(e) => set("name", e.target.value)} placeholder="prod-web-1" autoFocus />
        </Field>
        <Field label="Group" className="col-span-2">
          <Input value={h.group} onChange={(e) => set("group", e.target.value)} placeholder="Production" list="host-groups" />
          <datalist id="host-groups">
            {[...new Set(hosts.map((x) => x.group).filter(Boolean))].map((g) => (
              <option key={g} value={g} />
            ))}
          </datalist>
        </Field>
        <Field label="Color" className="col-span-1">
          <div className="flex h-8 items-center gap-1">
            {COLORS.map((c) => (
              <button
                key={c || "none"}
                onClick={() => set("color", c || null)}
                className="h-4 w-4 rounded-full border border-border cursor-pointer"
                style={{ background: c || "transparent", outline: (h.color ?? "") === c ? "2px solid white" : undefined }}
                title={c || "none"}
              />
            ))}
          </div>
        </Field>
        <Field label="Hostname / IP" className="col-span-3">
          <Input value={h.hostname} onChange={(e) => set("hostname", e.target.value)} placeholder="10.0.0.5" />
        </Field>
        <Field label="Port" className="col-span-1">
          <Input type="number" value={h.port} onChange={(e) => set("port", Number(e.target.value) || 22)} />
        </Field>
        <Field label="Username" className="col-span-2">
          <Input value={h.username} onChange={(e) => set("username", e.target.value)} placeholder="root" />
        </Field>

        <Field label="Authentication" className="col-span-2">
          <Select
            value={h.auth.type}
            onChange={(e) => {
              const t = e.target.value as AuthMethod["type"];
              if (t === "password") setAuth({ type: "password", secretId: null });
              if (t === "vault_key") setAuth({ type: "vault_key", secretId: null });
              if (t === "key_file") setAuth({ type: "key_file", path: "~/.ssh/id_ed25519", passphraseSecretId: null });
              if (t === "agent") setAuth({ type: "agent" });
            }}
          >
            <option value="agent">ssh-agent</option>
            <option value="vault_key">Key from vault</option>
            <option value="key_file">Key file on disk</option>
            <option value="password">Password (vault)</option>
          </Select>
        </Field>
        <div className="col-span-4">
          {vaultNeeded && (
            <div className="mt-5 flex items-center gap-2 text-sm text-warn">
              <Lock size={14} /> Vault is locked.
              <Button size="sm" onClick={() => setUnlockPrompt(true)}>
                Unlock
              </Button>
            </div>
          )}
          {h.auth.type === "password" && !vaultNeeded && (
            <div className="grid grid-cols-2 gap-2">
              <Field label="Stored password">
                <Select value={h.auth.secretId ?? ""} onChange={(e) => setAuth({ type: "password", secretId: e.target.value || null })}>
                  <option value="">— none —</option>
                  {secretsOf("password").map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                </Select>
              </Field>
              <Field label="…or new password">
                <Input type="password" value={inlinePw} onChange={(e) => setInlinePw(e.target.value)} placeholder="saved to vault" />
              </Field>
            </div>
          )}
          {h.auth.type === "vault_key" && !vaultNeeded && (
            <Field label="Private key" hint={secretsOf("private_key").length === 0 ? "Add or generate a key in the Vault first." : undefined}>
              <Select value={h.auth.secretId ?? ""} onChange={(e) => setAuth({ type: "vault_key", secretId: e.target.value || null })}>
                <option value="">— select key —</option>
                {secretsOf("private_key").map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </Select>
            </Field>
          )}
          {h.auth.type === "key_file" && (
            <div className="grid grid-cols-2 gap-2">
              <Field label="Key path">
                <div className="flex gap-1">
                  <Input value={h.auth.path} onChange={(e) => h.auth.type === "key_file" && setAuth({ ...h.auth, path: e.target.value })} />
                  <IconButton title="Browse" onClick={browseKey} className="h-8 w-8">
                    <FolderSearch size={15} />
                  </IconButton>
                </div>
              </Field>
              <Field label="Passphrase (vault)">
                <Select
                  value={h.auth.passphraseSecretId ?? ""}
                  disabled={!vault.unlocked}
                  onChange={(e) => h.auth.type === "key_file" && setAuth({ ...h.auth, passphraseSecretId: e.target.value || null })}
                >
                  <option value="">— none —</option>
                  {secretsOf("password").map((s) => (
                    <option key={s.id} value={s.id}>
                      {s.name}
                    </option>
                  ))}
                </Select>
              </Field>
            </div>
          )}
          {h.auth.type === "agent" && (
            <div className="mt-6 flex items-center gap-2 text-xs text-fg-subtle">
              <KeyRound size={13} /> Uses identities loaded in ssh-agent ($SSH_AUTH_SOCK).
            </div>
          )}
        </div>

        <Field label="Jump host (ProxyJump)" className="col-span-3">
          <Select value={h.jumpHostId ?? ""} onChange={(e) => set("jumpHostId", e.target.value || null)}>
            <option value="">— direct —</option>
            {hosts
              .filter((x) => x.id !== h.id)
              .map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name}
                </option>
              ))}
          </Select>
        </Field>
        <Field label="Remote Docker socket" className="col-span-3">
          <Input value={h.dockerSocket ?? ""} onChange={(e) => set("dockerSocket", e.target.value || null)} placeholder="/var/run/docker.sock" />
        </Field>
        <Field label="Startup command" className="col-span-4">
          <Input value={h.startupCommand ?? ""} onChange={(e) => set("startupCommand", e.target.value || null)} placeholder="cd /srv/app" />
        </Field>
        <Field label="Keepalive (s)" className="col-span-2">
          <Input type="number" value={h.keepaliveSecs ?? 30} onChange={(e) => set("keepaliveSecs", Number(e.target.value) || null)} />
        </Field>
        <Field label="Tags (comma separated)" className="col-span-6">
          <Input value={h.tags.join(", ")} onChange={(e) => set("tags", e.target.value.split(",").map((t) => t.trim()))} />
        </Field>

        <div className="col-span-6">
          <div className="mb-1 flex items-center justify-between">
            <span className="text-xs font-medium text-fg-muted">Local port forwards (127.0.0.1:local → remote host:port)</span>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => set("forwards", [...h.forwards, { id: "", localPort: 8080, remoteHost: "127.0.0.1", remotePort: 80, autoStart: false }])}
            >
              <Plus size={13} /> Add
            </Button>
          </div>
          {h.forwards.map((f, i) => (
            <div key={i} className="mb-1 flex items-center gap-2">
              <Input type="number" className="w-24" value={f.localPort} onChange={(e) => updateFwd(i, { localPort: Number(e.target.value) })} />
              <span className="text-fg-subtle">→</span>
              <Input className="flex-1" value={f.remoteHost} onChange={(e) => updateFwd(i, { remoteHost: e.target.value })} />
              <span className="text-fg-subtle">:</span>
              <Input type="number" className="w-24" value={f.remotePort} onChange={(e) => updateFwd(i, { remotePort: Number(e.target.value) })} />
              <IconButton title="Remove" onClick={() => set("forwards", h.forwards.filter((_, j) => j !== i))}>
                <Trash2 size={14} />
              </IconButton>
            </div>
          ))}
        </div>
      </div>
    </Modal>
  );
}
