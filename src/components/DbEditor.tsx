import { ask } from "@tauri-apps/plugin-dialog";
import { Lock, Play, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { DB_ENGINES, SSL_LABELS } from "../lib/dbEngines";
import { api, errMsg } from "../lib/ipc";
import type { DbEngine, DbProfile, DbSafety, SslMode } from "../lib/types";
import { useApp } from "../store";
import { Button, Checkbox, Field, Input, Modal, Select } from "./ui";

const blank = (): DbProfile => ({
  id: "",
  name: "",
  group: "",
  engine: "postgres",
  host: "",
  port: DB_ENGINES.postgres.defaultPort,
  database: "",
  username: "",
  passwordSecretId: null,
  viaHostId: null,
  sslMode: "prefer",
  safety: "confirm",
  options: {},
  color: null,
  tags: [],
});

const SAFETY: { value: DbSafety; label: string; help: string }[] = [
  { value: "off", label: "Off", help: "Run everything as typed." },
  { value: "confirm", label: "Confirm risky", help: "Ask before UPDATE/DELETE without WHERE, DROP or TRUNCATE." },
  { value: "read-only", label: "Read-only", help: "Writes are refused. Good for production." },
];

const COLORS = ["", "#8b5cf6", "#3b82f6", "#14b8a6", "#f43f5e", "#f59e0b", "#22c55e"];

export function DbEditor() {
  const { dbEditor, setDbEditor, dbs, hosts, refreshDbs, secrets, vault, setUnlockPrompt, toast, error, refreshSecrets } = useApp();
  const [d, setD] = useState<DbProfile>(blank);
  const [inlinePw, setInlinePw] = useState("");
  const [testing, setTesting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [touched, setTouched] = useState(false);

  useEffect(() => {
    if (dbEditor === "new") setD(blank());
    else if (dbEditor) setD(structuredClone(dbEditor));
    setInlinePw("");
    setConfirmDelete(false);
    setTouched(false);
  }, [dbEditor]);

  if (!dbEditor) return null;
  const isNew = dbEditor === "new";
  const engine = DB_ENGINES[d.engine];
  const set = <K extends keyof DbProfile>(k: K, v: DbProfile[K]) => setD((p) => ({ ...p, [k]: v }));
  const setOption = (k: string, v: string) =>
    setD((p) => {
      const options = { ...p.options };
      if (v) options[k] = v;
      else delete options[k];
      return { ...p, options };
    });

  /** Store a newly typed password in the vault and point the profile at it. */
  const withPassword = async (): Promise<DbProfile> => {
    if (!inlinePw) return d;
    const meta = await api.secretSave({
      name: `${d.name || d.host} database password`,
      kind: "password",
      username: d.username || null,
      value: inlinePw,
      notes: "",
    });
    await refreshSecrets();
    setInlinePw("");
    const next = { ...d, passwordSecretId: meta.id };
    setD(next);
    return next;
  };

  const save = async () => {
    try {
      const saved = await api.dbSave({ ...(await withPassword()), tags: d.tags.filter(Boolean) });
      await refreshDbs();
      toast(`Saved ${saved.name}`, "success");
      setDbEditor(null);
    } catch (e) {
      error(e);
    }
  };

  const test = async () => {
    setTesting(true);
    try {
      const version = await api.dbTest(await withPassword());
      toast(`Connected: ${version}`, "success");
    } catch (e) {
      const msg = errMsg(e);
      toast(`Connection failed: ${msg}`, "error");
      if (/unlock the vault/i.test(msg)) setUnlockPrompt(true);
    } finally {
      setTesting(false);
    }
  };

  const del = async () => {
    if (
      useApp.getState().dbTx[d.id] &&
      !(await ask("A tab on this connection has an open transaction. Deleting closes it and rolls the changes back.", {
        title: "Delete connection?",
        kind: "warning",
        okLabel: "Delete",
      }))
    )
      return;
    try {
      await api.dbDelete(d.id);
      await refreshDbs();
      setDbEditor(null);
    } catch (e) {
      error(e);
    }
  };

  const needsVault = !vault.unlocked;
  const via = hosts.find((h) => h.id === d.viaHostId);
  const missing = d.host.trim() ? null : "Enter the database host first";
  const passwords = secrets.filter((s) => s.kind === "password");

  return (
    <Modal
      title={isNew ? "New database connection" : `Edit ${dbEditor.name}`}
      onClose={() => setDbEditor(null)}
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
          {missing && <span className="text-xs text-fg-subtle">{missing}</span>}
          <Button onClick={test} disabled={testing || !!missing}>
            <Play size={14} /> {testing ? "Testing…" : "Test connection"}
          </Button>
          <Button variant="primary" onClick={save} disabled={!!missing}>
            Save
          </Button>
        </>
      }
    >
      <div className="space-y-6">
        <Section title="General">
          <div className="grid grid-cols-2 gap-4">
            <Field label="Name">
              <Input value={d.name} onChange={(e) => set("name", e.target.value)} placeholder="orders-prod" autoFocus />
            </Field>
            <Field label="Engine">
              <Select
                value={d.engine}
                onChange={(e) => {
                  const next = DB_ENGINES[e.target.value as DbEngine];
                  setD((p) => ({ ...p, engine: next.id, port: next.defaultPort, sslMode: next.sslModes.includes(p.sslMode) ? p.sslMode : (next.sslModes[0] ?? "disable"), options: {} }));
                }}
              >
                {Object.values(DB_ENGINES).map((x) => (
                  <option key={x.id} value={x.id}>
                    {x.label}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Group">
              <Input value={d.group} onChange={(e) => set("group", e.target.value)} placeholder="Production" list="db-groups" />
              <datalist id="db-groups">
                {[...new Set(dbs.map((x) => x.group).filter(Boolean))].map((g) => (
                  <option key={g} value={g} />
                ))}
              </datalist>
            </Field>
            <Field label="Color">
              <div className="flex h-9 items-center gap-2">
                {COLORS.map((c) => {
                  const selected = (d.color ?? "") === c;
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
          <Field
            label="Connect through"
            className="mb-4"
            hint={
              via
                ? `Reached from ${via.name} over a private SSH tunnel; nothing is opened on your network.`
                : hosts.length
                  ? "Pick an SSH host to reach a database that only that server can see."
                  : "Add an SSH host to reach databases on private networks."
            }
          >
            <Select value={d.viaHostId ?? ""} onChange={(e) => set("viaHostId", e.target.value || null)}>
              <option value="">Direct connection</option>
              {hosts.map((h) => (
                <option key={h.id} value={h.id}>
                  SSH tunnel via {h.name} ({h.username}@{h.hostname})
                </option>
              ))}
            </Select>
          </Field>
          <div className="grid grid-cols-[1fr_7rem] gap-4">
            <Field label="Host (required)" hint={via ? `As ${via.name} sees it, e.g. 127.0.0.1 or db.internal` : undefined}>
              <Input
                value={d.host}
                onChange={(e) => set("host", e.target.value)}
                placeholder={via ? "127.0.0.1" : "db.example.com"}
                aria-required
                aria-invalid={!d.host.trim() && touched}
                onBlur={() => setTouched(true)}
                className={!d.host.trim() && touched ? "border-danger" : undefined}
              />
            </Field>
            <Field label="Port">
              <Input type="number" value={d.port} onChange={(e) => set("port", Number(e.target.value) || engine.defaultPort)} />
            </Field>
          </div>
          <div className="mt-4 grid grid-cols-2 gap-4">
            <Field label="Database">
              <Input value={d.database} onChange={(e) => set("database", e.target.value)} placeholder={engine.databasePlaceholder} />
            </Field>
            <Field label="Username">
              <Input value={d.username} onChange={(e) => set("username", e.target.value)} placeholder={engine.usernamePlaceholder} />
            </Field>
          </div>
          <div className="mt-4">
            {needsVault ? (
              <div className="flex items-center gap-3 rounded-lg border border-warn/30 bg-warn/10 px-3 py-2.5 text-sm text-warn">
                <Lock size={15} /> <span className="flex-1">The vault is locked. Unlock it to choose or store a password.</span>
                <Button size="sm" onClick={() => setUnlockPrompt(true)}>
                  Unlock
                </Button>
              </div>
            ) : (
              <div className="grid grid-cols-2 gap-4">
                <Field label="Stored password">
                  <Select value={d.passwordSecretId ?? ""} onChange={(e) => set("passwordSecretId", e.target.value || null)}>
                    <option value="">— none —</option>
                    {passwords.map((s) => (
                      <option key={s.id} value={s.id}>
                        {s.name}
                      </option>
                    ))}
                  </Select>
                </Field>
                <Field label="Or enter a new password" hint="Saved to the vault on save or test">
                  <Input type="password" value={inlinePw} onChange={(e) => setInlinePw(e.target.value)} />
                </Field>
              </div>
            )}
          </div>
        </Section>

        <Section title="Safety">
          <div role="radiogroup" aria-label="Safety level" className="grid grid-cols-3 gap-2">
            {SAFETY.map((o) => (
              <button
                key={o.value}
                type="button"
                role="radio"
                aria-checked={d.safety === o.value}
                onClick={() => set("safety", o.value)}
                className={
                  "rounded-lg border px-3 py-2.5 text-left transition-colors cursor-pointer " +
                  (d.safety === o.value ? "border-accent bg-accent/10" : "border-border hover:bg-surface-3")
                }
              >
                <div className="text-sm font-medium">{o.label}</div>
                <div className="mt-0.5 text-xs text-fg-muted">{o.help}</div>
              </button>
            ))}
          </div>
          {d.safety === "read-only" && (
            <p className="mt-2 text-xs text-fg-subtle">
              Shellport asks the server to refuse writes for this session. For a hard guarantee, also connect with a database role that only has read access.
            </p>
          )}
        </Section>

        {(engine.sslModes.length > 0 || engine.extraFields.length > 0) && (
          <Section title="Advanced">
            <div className="grid grid-cols-2 gap-4">
              {engine.sslModes.length > 0 && (
                <Field label="TLS" hint={d.sslMode === "verify-full" ? "Checks the certificate against public CAs and the host name" : undefined}>
                  <Select value={d.sslMode} onChange={(e) => set("sslMode", e.target.value as SslMode)}>
                    {engine.sslModes.map((m) => (
                      <option key={m} value={m}>
                        {SSL_LABELS[m]}
                      </option>
                    ))}
                  </Select>
                </Field>
              )}
              {engine.extraFields.map((f) =>
                f.type === "checkbox" ? (
                  <div key={f.key} className="col-span-2 flex flex-col gap-1.5">
                    <Checkbox label={f.label} checked={d.options[f.key] === "1"} onChange={(v) => setOption(f.key, v ? "1" : "")} />
                    {f.hint && <span className="pl-6 text-xs text-fg-subtle">{f.hint}</span>}
                  </div>
                ) : (
                  <Field key={f.key} label={f.label} hint={f.hint}>
                    <Input value={d.options[f.key] ?? ""} onChange={(e) => setOption(f.key, e.target.value)} placeholder={f.placeholder} />
                  </Field>
                ),
              )}
            </div>
          </Section>
        )}
      </div>
    </Modal>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <div className="mb-3 border-b border-border pb-2">
        <h3 className="text-xs font-semibold uppercase tracking-wider text-fg-subtle">{title}</h3>
      </div>
      {children}
    </section>
  );
}
