import { listen } from "@tauri-apps/api/event";
import { KeyRound, ShieldAlert, ShieldQuestion } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errMsg } from "../lib/ipc";
import type { AuthPrompt, HostKeyPrompt } from "../lib/types";
import { useApp } from "../store";
import { KeyringNote } from "../views/VaultView";
import { Button, Checkbox, Field, Input, Modal } from "./ui";

/** Host key verification: shown when the server key is unknown or changed. */
export function HostKeyDialog() {
  const [queue, setQueue] = useState<HostKeyPrompt[]>([]);
  const [confirmReplace, setConfirmReplace] = useState(false);
  useEffect(() => {
    const un = listen<HostKeyPrompt>("hostkey-prompt", (e) => setQueue((q) => [...q, e.payload]));
    // The backend gave up on a prompt (timeout, cancelled connection): drop its dialog.
    const un2 = listen<string>("hostkey-prompt-done", (e) => setQueue((q) => q.filter((x) => x.requestId !== e.payload)));
    return () => {
      un.then((f) => f());
      un2.then((f) => f());
    };
  }, []);
  const p = queue[0];
  if (!p) return null;
  const answer = (decision: "reject" | "once" | "trust") => {
    api.hostkeyRespond(p.requestId, decision).catch(() => {});
    setConfirmReplace(false);
    setQueue((q) => q.slice(1));
  };
  return (
    <Modal
      title={p.changed ? "Host key changed" : "Unknown host key"}
      onClose={() => answer("reject")}
      footer={
        p.changed ? (
          <>
            <Button variant="ghost" onClick={() => answer("reject")}>
              Reject
            </Button>
            <div className="flex-1" />
            {confirmReplace ? (
              <Button variant="danger" onClick={() => answer("trust")}>
                Yes, replace saved key
              </Button>
            ) : (
              <Button variant="danger" onClick={() => setConfirmReplace(true)}>
                Replace key & connect
              </Button>
            )}
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={() => answer("reject")}>
              Reject
            </Button>
            <div className="flex-1" />
            <Button onClick={() => answer("once")}>Connect once</Button>
            <Button variant="primary" onClick={() => answer("trust")}>
              Trust & connect
            </Button>
          </>
        )
      }
    >
      <div className="flex gap-4">
        {p.changed ? <ShieldAlert className="shrink-0 text-danger" size={30} /> : <ShieldQuestion className="shrink-0 text-warn" size={30} />}
        <div className="space-y-3 text-sm leading-relaxed">
          {p.changed ? (
            <>
              <p className="text-danger">
                {p.changedLine
                  ? <>The key presented by <b>{p.host}:{p.port}</b> does not match the one saved in ~/.ssh/known_hosts (line {p.changedLine}).</>
                  : <>~/.ssh/known_hosts could not be read, so the key presented by <b>{p.host}:{p.port}</b> can't be checked.</>}{" "}
                Someone could be intercepting this connection, or the server was reinstalled. No password or key is sent unless you replace the
                saved key.
              </p>
              <p className="text-fg-muted">
                Only replace the saved key if you know the server changed. The old entry is removed with <code>ssh-keygen -R</code>, which keeps a
                backup in <code>known_hosts.old</code>.
              </p>
            </>
          ) : (
            <p>
              The authenticity of <b>{p.host}:{p.port}</b> can't be established. <b>Trust</b> saves the key to ~/.ssh/known_hosts;{" "}
              <b>Connect once</b> doesn't save it.
            </p>
          )}
          <div className="rounded-lg bg-surface-1 p-3 font-mono text-xs">
            <div className="text-fg-subtle">{p.algorithm}</div>
            <div className="break-all">{p.fingerprint}</div>
          </div>
        </div>
      </div>
    </Modal>
  );
}

/** Keyboard-interactive prompts (2FA / OTP codes) from the server. */
export function AuthPromptDialog() {
  const [queue, setQueue] = useState<AuthPrompt[]>([]);
  const [values, setValues] = useState<string[]>([]);
  useEffect(() => {
    const un = listen<AuthPrompt>("auth-prompt", (e) => setQueue((q) => [...q, e.payload]));
    return () => {
      un.then((f) => f());
    };
  }, []);
  const p = queue[0];
  useEffect(() => setValues(p ? p.prompts.map(() => "") : []), [p]);
  if (!p) return null;
  const respond = (answers: string[] | null) => {
    api.authPromptRespond(p.requestId, answers).catch(() => {});
    setQueue((q) => q.slice(1));
  };
  return (
    <Modal
      title={`Verification for ${p.host}`}
      onClose={() => respond(null)}
      width="max-w-md"
      footer={
        <>
          <Button variant="ghost" onClick={() => respond(null)}>
            Cancel
          </Button>
          <Button variant="primary" onClick={() => respond(values)}>
            Continue
          </Button>
        </>
      }
    >
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          respond(values);
        }}
      >
        <div className="flex items-start gap-3 text-sm text-fg-muted">
          <KeyRound size={18} className="mt-0.5 shrink-0 text-accent" />
          <span>
            <b className="text-fg">{p.host}</b> is asking for additional verification. The text below comes from that server, not from Shellport;
            never enter your vault master password here.
          </span>
        </div>
        {(p.name || p.instructions) && (
          <div className="whitespace-pre-wrap rounded-md border border-border bg-surface-2 px-3 py-2 text-sm text-fg-muted">
            {[p.name, p.instructions].filter(Boolean).join("\n")}
          </div>
        )}
        {p.prompts.map((f, i) => (
          <Field key={i} label={f.prompt.trim().replace(/:$/, "") || "Response"}>
            <Input
              autoFocus={i === 0}
              type={f.echo ? "text" : "password"}
              autoComplete="one-time-code"
              value={values[i] ?? ""}
              onChange={(e) => setValues((v) => v.map((x, j) => (j === i ? e.target.value : x)))}
            />
          </Field>
        ))}
        <button type="submit" className="hidden" />
      </form>
    </Modal>
  );
}

/** Backend errors raised outside a command (e.g. auto-started forwards). */
export function useAppErrors() {
  useEffect(() => {
    const un = listen<string>("app-error", (e) => useApp.getState().toast(e.payload, "error"));
    const un2 = listen("forwards-changed", () => useApp.getState().refreshForwards());
    return () => {
      un.then((f) => f());
      un2.then((f) => f());
    };
  }, []);
}

/** Small unlock prompt used when an action needs vault secrets. */
export function UnlockDialog() {
  const { unlockPrompt, setUnlockPrompt, vault, refreshVault, openTab } = useApp();
  const [pw, setPw] = useState("");
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    setPw("");
    setErr(null);
  }, [unlockPrompt]);
  if (!unlockPrompt) return null;
  const close = () => setUnlockPrompt(false);
  if (!vault.exists) {
    return (
      <Modal title="No vault yet" onClose={close}>
        <p className="mb-3 text-sm text-fg-muted">Create a vault to store passwords and keys securely.</p>
        <Button
          variant="primary"
          onClick={() => {
            close();
            openTab({ kind: "vault", title: "Vault" }, { unique: true });
          }}
        >
          Open Vault
        </Button>
      </Modal>
    );
  }
  const submit = async () => {
    setBusy(true);
    setErr(null);
    try {
      await api.vaultUnlock(pw, remember);
      await refreshVault();
      close();
    } catch (e) {
      setErr(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title="Unlock vault"
      onClose={close}
      width="max-w-sm"
      footer={
        <>
          {vault.remembered && (
            <Button
              onClick={async () => {
                try {
                  await api.vaultUnlockKeyring();
                  await refreshVault();
                  close();
                } catch (e) {
                  setErr(errMsg(e));
                }
              }}
            >
              <KeyRound size={14} /> Use keyring
            </Button>
          )}
          <Button variant="primary" disabled={!pw || busy} onClick={submit}>
            {busy ? "Unlocking…" : "Unlock"}
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
        className="space-y-3"
      >
        <Input type="password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} placeholder="Master password" />
        <Checkbox label="Remember on this device (system keyring)" checked={remember} onChange={setRemember} />
        {remember && <KeyringNote />}
        {err && <div className="text-sm text-danger">{err}</div>}
      </form>
    </Modal>
  );
}

export function Toasts() {
  const { toasts, dismissToast } = useApp();
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-96 flex-col gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          onClick={() => dismissToast(t.id)}
          className={
            "pointer-events-auto cursor-pointer rounded-lg border px-3 py-2 text-sm shadow-xl " +
            (t.kind === "error"
              ? "border-danger/40 bg-[#2a1219] text-red-200"
              : t.kind === "success"
                ? "border-ok/40 bg-[#10241a] text-green-200"
                : "border-border bg-surface-2 text-fg")
          }
        >
          {t.text}
        </div>
      ))}
    </div>
  );
}
