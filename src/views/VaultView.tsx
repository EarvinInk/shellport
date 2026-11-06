import { open } from "@tauri-apps/plugin-dialog";
import { Copy, Eye, EyeOff, FileKey, KeyRound, Lock, Plus, ShieldCheck, Sparkles, StickyNote, Trash2, Unlock } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Button, Checkbox, Empty, Field, IconButton, Input, Modal, Select, TextArea } from "../components/ui";
import { api, errMsg } from "../lib/ipc";
import type { SecretInput, SecretKind, SecretMeta } from "../lib/types";
import { useApp } from "../store";

const KIND_LABEL: Record<SecretKind, string> = {
  password: "Password",
  private_key: "Private key",
  token: "Token / API key",
  note: "Secure note",
};

function KindIcon({ k }: { k: SecretKind }) {
  if (k === "private_key") return <FileKey size={15} />;
  if (k === "note") return <StickyNote size={15} />;
  return <KeyRound size={15} />;
}

export function VaultView() {
  const { vault } = useApp();
  if (!vault.exists) return <CreateVault />;
  if (!vault.unlocked) return <UnlockVault />;
  return <SecretsManager />;
}

function CreateVault() {
  const { refreshVault } = useApp();
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [remember, setRemember] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (pw !== pw2) return setErr("Passwords do not match");
    setBusy(true);
    try {
      await api.vaultCreate(pw, remember);
      await refreshVault();
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex h-full items-center justify-center p-6">
      <form
        className="w-full max-w-sm space-y-3 rounded-xl border border-border bg-surface-2 p-6"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="flex items-center gap-2 text-lg font-semibold">
          <ShieldCheck className="text-accent" /> Create your vault
        </div>
        <p className="text-sm text-fg-muted">
          Passwords, private keys and tokens are encrypted with Argon2id + XChaCha20-Poly1305 using your master password. It cannot be recovered if
          lost.
        </p>
        <Input type="password" autoFocus placeholder="Master password (8+ chars)" value={pw} onChange={(e) => setPw(e.target.value)} />
        <Input type="password" placeholder="Confirm" value={pw2} onChange={(e) => setPw2(e.target.value)} />
        <Checkbox label="Unlock automatically via system keyring" checked={remember} onChange={setRemember} />
        {err && <div className="text-sm text-danger">{err}</div>}
        <Button variant="primary" className="w-full" disabled={busy || pw.length < 8}>
          {busy ? "Creating…" : "Create vault"}
        </Button>
      </form>
    </div>
  );
}

function UnlockVault() {
  const { refreshVault, vault } = useApp();
  const [pw, setPw] = useState("");
  const [remember, setRemember] = useState(vault.remembered);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.vaultUnlock(pw, remember);
      await refreshVault();
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex h-full items-center justify-center p-6">
      <form
        className="w-full max-w-sm space-y-3 rounded-xl border border-border bg-surface-2 p-6"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="flex items-center gap-2 text-lg font-semibold">
          <Lock className="text-accent" /> Vault locked
        </div>
        <Input type="password" autoFocus placeholder="Master password" value={pw} onChange={(e) => setPw(e.target.value)} />
        <Checkbox label="Remember on this device" checked={remember} onChange={setRemember} />
        {err && <div className="text-sm text-danger">{err}</div>}
        <div className="flex gap-2">
          {vault.remembered && (
            <Button
              type="button"
              onClick={async () => {
                try {
                  await api.vaultUnlockKeyring();
                  await refreshVault();
                } catch (e) {
                  setErr(errMsg(e));
                }
              }}
            >
              <KeyRound size={14} /> Keyring
            </Button>
          )}
          <Button variant="primary" className="flex-1" disabled={busy || !pw}>
            <Unlock size={14} /> {busy ? "Unlocking…" : "Unlock"}
          </Button>
        </div>
      </form>
    </div>
  );
}

function SecretsManager() {
  const { secrets, refreshSecrets, refreshVault, toast, error, vault, hosts } = useApp();
  const [editing, setEditing] = useState<SecretMeta | "new" | null>(null);
  const [q, setQ] = useState("");
  const [changePw, setChangePw] = useState(false);
  const [revealed, setRevealed] = useState<Record<string, string>>({});

  useEffect(() => {
    refreshSecrets();
  }, [refreshSecrets]);

  const list = secrets.filter((s) => !q || s.name.toLowerCase().includes(q.toLowerCase()));
  const usedBy = (id: string) =>
    hosts.filter(
      (h) =>
        ((h.auth.type === "password" || h.auth.type === "vault_key") && h.auth.secretId === id) ||
        (h.auth.type === "key_file" && h.auth.passphraseSecretId === id),
    );

  const copy = async (s: SecretMeta) => {
    try {
      const r = await api.secretReveal(s.id);
      await navigator.clipboard.writeText(r.value);
      toast(`Copied ${s.name}. Clipboard clears in 30s.`);
      setTimeout(() => navigator.clipboard.writeText("").catch(() => {}), 30000);
    } catch (e) {
      error(e);
    }
  };

  const toggleReveal = async (s: SecretMeta) => {
    if (revealed[s.id] !== undefined) {
      const { [s.id]: _, ...rest } = revealed;
      setRevealed(rest);
      return;
    }
    try {
      const r = await api.secretReveal(s.id);
      setRevealed({ ...revealed, [s.id]: r.value });
    } catch (e) {
      error(e);
    }
  };

  const copyPub = async (s: SecretMeta) => {
    try {
      await navigator.clipboard.writeText(await api.secretPublicKey(s.id));
      toast("Public key copied: paste it into ~/.ssh/authorized_keys on the server");
    } catch (e) {
      error(e);
    }
  };

  const generate = async () => {
    try {
      const m = await api.secretGenerateKey(`ed25519-${new Date().toISOString().slice(0, 10)}`);
      await refreshSecrets();
      toast(`Generated ${m.name}`, "success");
    } catch (e) {
      error(e);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b border-border px-4 py-3">
        <ShieldCheck className="text-accent" size={18} />
        <div className="font-semibold">Vault</div>
        <Badge tone="ok">unlocked</Badge>
        {vault.remembered && <Badge tone="info">keyring</Badge>}
        <div className="flex-1" />
        <Input className="w-56" placeholder="Search secrets" value={q} onChange={(e) => setQ(e.target.value)} />
        <Button onClick={generate}>
          <Sparkles size={14} /> Generate key
        </Button>
        <Button variant="primary" onClick={() => setEditing("new")}>
          <Plus size={14} /> Add secret
        </Button>
        <Button variant="ghost" onClick={() => setChangePw(true)}>
          Change password
        </Button>
        {vault.remembered && (
          <Button
            variant="ghost"
            onClick={() =>
              api
                .vaultForgetDevice()
                .then(refreshVault)
                .then(() => toast("Removed key from system keyring"))
                .catch(error)
            }
          >
            Forget device
          </Button>
        )}
        <Button
          variant="ghost"
          onClick={async () => {
            await api.vaultLock();
            await refreshVault();
          }}
        >
          <Lock size={14} /> Lock
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {list.length === 0 ? (
          <Empty icon={<KeyRound size={32} />} title="No secrets yet">
            Add passwords, private keys or API tokens, then reference them from hosts.
          </Empty>
        ) : (
          <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
            {list.map((s) => (
              <div key={s.id} className="rounded-lg border border-border bg-surface-2 p-3">
                <div className="flex items-center gap-2">
                  <div className="flex h-8 w-8 items-center justify-center rounded-md bg-accent/15 text-accent">
                    <KindIcon k={s.kind} />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{s.name}</div>
                    <div className="truncate text-xs text-fg-subtle">
                      {KIND_LABEL[s.kind]}
                      {s.username ? ` · ${s.username}` : ""}
                      {s.hasPassphrase ? " · passphrase" : ""}
                      {usedBy(s.id).length > 0 ? ` · used by ${usedBy(s.id).map((h) => h.name).join(", ")}` : ""}
                    </div>
                  </div>
                  {s.kind === "private_key" && (
                    <Button size="sm" variant="ghost" onClick={() => copyPub(s)}>
                      Public key
                    </Button>
                  )}
                  <IconButton title={revealed[s.id] !== undefined ? "Hide" : "Reveal"} onClick={() => toggleReveal(s)}>
                    {revealed[s.id] !== undefined ? <EyeOff size={14} /> : <Eye size={14} />}
                  </IconButton>
                  <IconButton title="Copy" onClick={() => copy(s)}>
                    <Copy size={14} />
                  </IconButton>
                  <Button size="sm" variant="ghost" onClick={() => setEditing(s)}>
                    Edit
                  </Button>
                </div>
                {revealed[s.id] !== undefined && (
                  <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded bg-surface-1 p-2 font-mono text-xs">{revealed[s.id]}</pre>
                )}
                {s.notes && <div className="mt-2 text-xs text-fg-muted">{s.notes}</div>}
              </div>
            ))}
          </div>
        )}
      </div>
      {editing && <SecretEditor secret={editing} onClose={() => setEditing(null)} />}
      {changePw && <ChangePassword onClose={() => setChangePw(false)} />}
    </div>
  );
}

function SecretEditor({ secret, onClose }: { secret: SecretMeta | "new"; onClose: () => void }) {
  const { refreshSecrets, error, toast } = useApp();
  const isNew = secret === "new";
  const [f, setF] = useState<SecretInput>(
    isNew
      ? { name: "", kind: "password", username: "", value: "", passphrase: "", notes: "" }
      : { id: secret.id, name: secret.name, kind: secret.kind, username: secret.username ?? "", value: null, passphrase: null, notes: secret.notes },
  );
  const [confirm, setConfirm] = useState(false);

  const save = async () => {
    try {
      await api.secretSave({ ...f, value: f.value === "" && !isNew ? null : f.value, passphrase: f.passphrase === "" && !isNew ? null : f.passphrase });
      await refreshSecrets();
      toast("Secret saved", "success");
      onClose();
    } catch (e) {
      error(e);
    }
  };

  const loadFile = async () => {
    const path = await open({ title: "Import private key", defaultPath: "~/.ssh" });
    if (typeof path !== "string") return;
    try {
      setF({ ...f, value: await api.localReadText(path), name: f.name || path.split("/").pop() || "" });
    } catch (e) {
      error(e);
    }
  };

  return (
    <Modal
      title={isNew ? "Add secret" : `Edit ${f.name}`}
      onClose={onClose}
      footer={
        <>
          {!isNew &&
            (confirm ? (
              <Button
                variant="danger"
                onClick={async () => {
                  try {
                    await api.secretDelete(f.id!);
                    await refreshSecrets();
                    onClose();
                  } catch (e) {
                    error(e);
                  }
                }}
              >
                Confirm delete
              </Button>
            ) : (
              <Button variant="ghost" onClick={() => setConfirm(true)}>
                <Trash2 size={14} /> Delete
              </Button>
            ))}
          <div className="flex-1" />
          <Button variant="primary" onClick={save} disabled={!f.name}>
            Save
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <Field label="Name">
            <Input autoFocus value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} />
          </Field>
          <Field label="Type">
            <Select value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value as SecretKind })}>
              {Object.entries(KIND_LABEL).map(([k, l]) => (
                <option key={k} value={k}>
                  {l}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <Field label="Username (optional)">
          <Input value={f.username ?? ""} onChange={(e) => setF({ ...f, username: e.target.value })} />
        </Field>
        {f.kind === "private_key" || f.kind === "note" ? (
          <Field
            label={f.kind === "private_key" ? "Private key (OpenSSH / PEM)" : "Note"}
            hint={!isNew ? "Leave empty to keep the current value." : undefined}
          >
            <TextArea rows={8} value={f.value ?? ""} onChange={(e) => setF({ ...f, value: e.target.value })} placeholder={f.kind === "private_key" ? "-----BEGIN OPENSSH PRIVATE KEY-----" : ""} />
            {f.kind === "private_key" && (
              <Button size="sm" variant="ghost" className="self-start" onClick={loadFile} type="button">
                Load from file…
              </Button>
            )}
          </Field>
        ) : (
          <Field label={f.kind === "token" ? "Token" : "Password"} hint={!isNew ? "Leave empty to keep the current value." : undefined}>
            <Input type="password" value={f.value ?? ""} onChange={(e) => setF({ ...f, value: e.target.value })} />
          </Field>
        )}
        {f.kind === "private_key" && (
          <Field label="Key passphrase (if encrypted)">
            <Input type="password" value={f.passphrase ?? ""} onChange={(e) => setF({ ...f, passphrase: e.target.value })} />
          </Field>
        )}
        <Field label="Notes">
          <Input value={f.notes} onChange={(e) => setF({ ...f, notes: e.target.value })} />
        </Field>
      </div>
    </Modal>
  );
}

function ChangePassword({ onClose }: { onClose: () => void }) {
  const { toast } = useApp();
  const [old, setOld] = useState("");
  const [n1, setN1] = useState("");
  const [n2, setN2] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const submit = async () => {
    if (n1 !== n2) return setErr("New passwords do not match");
    try {
      await api.vaultChangePassword(old, n1);
      toast("Master password changed", "success");
      onClose();
    } catch (e) {
      setErr(errMsg(e));
    }
  };
  return (
    <Modal
      title="Change master password"
      onClose={onClose}
      width="max-w-sm"
      footer={
        <Button variant="primary" onClick={submit} disabled={!old || n1.length < 8}>
          Change
        </Button>
      }
    >
      <div className="space-y-2">
        <Input type="password" placeholder="Current password" value={old} onChange={(e) => setOld(e.target.value)} autoFocus />
        <Input type="password" placeholder="New password" value={n1} onChange={(e) => setN1(e.target.value)} />
        <Input type="password" placeholder="Confirm new password" value={n2} onChange={(e) => setN2(e.target.value)} />
        {err && <div className="text-sm text-danger">{err}</div>}
      </div>
    </Modal>
  );
}
