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

const COLORS = ["", "#8b5cf6", "#3b82f6", "#14b8a6", "#f43f5e", "#f59e0b", "#22c55e"];

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
      // Database tunnels through this host fall back to direct connections.
      await useApp.getState().refreshDbs();
      setHostEditor(null);
    } catch (e) {
      error(e);
    }
  };

  const browseKey = async () => {
    const f = await openDialog({ title: "Select private key", defaultPath: `${await api.localHome()}/.ssh`, multiple: false, directory: false });
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
      width="max-w-3xl"
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
      <div className="space-y-6">
        <Section title="General">
          <div className="grid grid-cols-2 gap-4">
            <Field label="Name">
              <Input value={h.name} onChange={(e) => set("name", e.target.value)} placeholder="prod-web-1" autoFocus />
            </Field>
            <Field label="Group">
              <Input value={h.group} onChange={(e) => set("group", e.target.value)} placeholder="Production" list="host-groups" />
              <datalist id="host-groups">
                {[...new Set(hosts.map((x) => x.group).filter(Boolean))].map((g) => (
                  <option key={g} value={g} />
                ))}
              </datalist>
            </Field>
            <Field label="Tags" hint="Comma separated">
              <Input value={h.tags.join(", ")} onChange={(e) => set("tags", e.target.value.split(",").map((t) => t.trim()))} placeholder="web, eu-west" />
            </Field>
            <Field label="Color">
              <div className="flex h-9 items-center gap-2">
                {COLORS.map((c) => {
                  const selected = (h.color ?? "") === c;
                  return (
                    <button
                      key={c || "none"}
                      type="button"
                      onClick={() => set("color", c || null)}
                      title={c || "No color"}
                      className={
                        "h-6 w-6 rounded-full border cursor-pointer transition " +
                        (selected ? "border-fg ring-2 ring-fg/40 ring-offset-2 ring-offset-surface-2" : "border-border hover:scale-110")
                      }
                      style={{ background: c || "transparent" }}
                    />
                  );
                })}
              </div>
            </Field>
          </div>
        </Section>

        <Section title="Connection">
          <div className="grid grid-cols-[1fr_7rem] gap-4">
            <Field label="Hostname / IP">
              <Input value={h.hostname} onChange={(e) => set("hostname", e.target.value)} placeholder="10.0.0.5 or server.example.com" />
            </Field>
            <Field label="Port">
              <Input type="number" value={h.port} onChange={(e) => set("port", Number(e.target.value) || 22)} />
            </Field>
          </div>
          <div className="mt-4 grid grid-cols-2 gap-4">
            <Field label="Username">
              <Input value={h.username} onChange={(e) => set("username", e.target.value)} placeholder="root" />
            </Field>
            <Field label="Jump host (ProxyJump)">
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
          </div>
        </Section>

        <Section title="Authentication">
          <div className="space-y-4">
            <Field label="Method">
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
                <option value="vault_key">Private key from vault</option>
                <option value="key_file">Private key file on disk</option>
                <option value="password">Password (stored in vault)</option>
              </Select>
            </Field>
            {vaultNeeded && (
              <div className="flex items-center gap-3 rounded-lg border border-warn/30 bg-warn/10 px-3 py-2.5 text-sm text-warn">
                <Lock size={15} /> <span className="flex-1">The vault is locked. Unlock it to choose a stored secret.</span>
                <Button size="sm" onClick={() => setUnlockPrompt(true)}>
                  Unlock
                </Button>
              </div>
            )}
            {h.auth.type === "password" && !vaultNeeded && (
              <div className="grid grid-cols-2 gap-4">
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
                <Field label="Or enter a new password" hint="Saved to the vault when you save the host">
                  <Input type="password" value={inlinePw} onChange={(e) => setInlinePw(e.target.value)} />
                </Field>
              </div>
            )}
            {h.auth.type === "vault_key" && !vaultNeeded && (
              <Field label="Private key" hint={secretsOf("private_key").length === 0 ? "No keys in the vault yet. Add or generate one in the Vault tab." : undefined}>
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
              <div className="grid grid-cols-2 gap-4">
                <Field label="Key file path">
                  <div className="flex gap-2">
                    <Input value={h.auth.path} onChange={(e) => h.auth.type === "key_file" && setAuth({ ...h.auth, path: e.target.value })} />
                    <Button type="button" onClick={browseKey} title="Browse">
                      <FolderSearch size={15} />
                    </Button>
                  </div>
                </Field>
                <Field label="Passphrase (from vault)" hint={!vault.unlocked ? "Unlock the vault to pick one" : undefined}>
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
              <div className="flex items-center gap-2 text-sm text-fg-muted">
                <KeyRound size={14} /> Uses the identities loaded in your ssh-agent ($SSH_AUTH_SOCK).
              </div>
            )}
          </div>
        </Section>

        <Section title="Advanced">
          <div className="grid grid-cols-[1fr_8rem] gap-4">
            <Field label="Startup command" hint="Runs after login in terminal sessions">
              <Input value={h.startupCommand ?? ""} onChange={(e) => set("startupCommand", e.target.value || null)} placeholder="cd /srv/app" />
            </Field>
            <Field label="Keepalive (s)">
              <Input type="number" value={h.keepaliveSecs ?? 30} onChange={(e) => set("keepaliveSecs", Number(e.target.value) || null)} />
            </Field>
          </div>
          <Field label="Remote Docker socket" className="mt-4">
            <Input value={h.dockerSocket ?? ""} onChange={(e) => set("dockerSocket", e.target.value || null)} placeholder="/var/run/docker.sock" />
          </Field>
        </Section>

        <Section
          title="Port forwards"
          action={
            <Button
              size="sm"
              variant="ghost"
              onClick={() => set("forwards", [...h.forwards, { id: "", localPort: 8080, remoteHost: "127.0.0.1", remotePort: 80, autoStart: false }])}
            >
              <Plus size={13} /> Add forward
            </Button>
          }
        >
          {h.forwards.length === 0 ? (
            <div className="text-sm text-fg-subtle">None. Forwards map 127.0.0.1:local on this machine to a host:port reachable from the server.</div>
          ) : (
            <div className="space-y-2">
              <div className="grid grid-cols-[7rem_1fr_7rem_5rem_2rem] gap-2 text-xs text-fg-subtle">
                <span>Local port</span>
                <span>Remote host</span>
                <span>Remote port</span>
                <span>Auto-start</span>
                <span />
              </div>
              {h.forwards.map((f, i) => (
                <div key={i} className="grid grid-cols-[7rem_1fr_7rem_5rem_2rem] items-center gap-2">
                  <Input type="number" value={f.localPort} onChange={(e) => updateFwd(i, { localPort: Number(e.target.value) })} />
                  <Input value={f.remoteHost} onChange={(e) => updateFwd(i, { remoteHost: e.target.value })} />
                  <Input type="number" value={f.remotePort} onChange={(e) => updateFwd(i, { remotePort: Number(e.target.value) })} />
                  <label className="flex h-9 items-center justify-center">
                    <input type="checkbox" className="h-4 w-4 accent-[var(--color-accent)]" checked={f.autoStart} onChange={(e) => updateFwd(i, { autoStart: e.target.checked })} />
                  </label>
                  <IconButton title="Remove" onClick={() => set("forwards", h.forwards.filter((_, j) => j !== i))}>
                    <Trash2 size={14} />
                  </IconButton>
                </div>
              ))}
            </div>
          )}
        </Section>
      </div>
    </Modal>
  );
}

function Section({ title, action, children }: { title: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section>
      <div className="mb-3 flex items-center justify-between border-b border-border pb-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">{title}</h3>
        {action}
      </div>
      {children}
    </section>
  );
}
