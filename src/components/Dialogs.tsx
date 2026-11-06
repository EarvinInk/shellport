import { listen } from "@tauri-apps/api/event";
import { KeyRound, ShieldAlert, ShieldQuestion } from "lucide-react";
import { useEffect, useState } from "react";
import { api, errMsg } from "../lib/ipc";
import type { HostKeyPrompt } from "../lib/types";
import { useApp } from "../store";
import { Button, Checkbox, Input, Modal } from "./ui";

/** Host key verification: shown when the server key is unknown or changed. */
export function HostKeyDialog() {
  const [queue, setQueue] = useState<HostKeyPrompt[]>([]);
  useEffect(() => {
    const un = listen<HostKeyPrompt>("hostkey-prompt", (e) => setQueue((q) => [...q, e.payload]));
    return () => {
      un.then((f) => f());
    };
  }, []);
  const p = queue[0];
  if (!p) return null;
  const answer = (accept: boolean) => {
    api.hostkeyRespond(p.requestId, accept).catch(() => {});
    setQueue((q) => q.slice(1));
  };
  return (
    <Modal
      title={p.changed ? "HOST KEY CHANGED" : "Unknown host key"}
      onClose={() => answer(false)}
      footer={
        <>
          <Button variant="ghost" onClick={() => answer(false)}>
            Reject
          </Button>
          {p.changed ? (
            <Button variant="danger" onClick={() => answer(true)}>
              Connect anyway (once)
            </Button>
          ) : (
            <Button variant="primary" onClick={() => answer(true)}>
              Trust & connect
            </Button>
          )}
        </>
      }
    >
      <div className="flex gap-3">
        {p.changed ? <ShieldAlert className="shrink-0 text-danger" size={28} /> : <ShieldQuestion className="shrink-0 text-warn" size={28} />}
        <div className="space-y-2 text-sm">
          {p.changed ? (
            <p className="text-danger">
              The key for <b>{p.host}:{p.port}</b> does not match the one in ~/.ssh/known_hosts (line {p.changedLine}). Someone could be
              intercepting this connection, or the server was reinstalled. The stored key will <b>not</b> be replaced; edit known_hosts if the change
              is expected.
            </p>
          ) : (
            <p>
              The authenticity of <b>{p.host}:{p.port}</b> can't be established. Trusting it adds the key to ~/.ssh/known_hosts.
            </p>
          )}
          <div className="rounded-md bg-surface-1 p-2 font-mono text-xs">
            <div className="text-fg-subtle">{p.algorithm}</div>
            <div className="break-all">{p.fingerprint}</div>
          </div>
        </div>
      </div>
    </Modal>
  );
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
