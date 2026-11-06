import { ArrowRight, ExternalLink, Loader2, Pencil, Play, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../lib/ipc";
import { useApp } from "../store";
import { Badge, Button, Empty, IconButton, Modal } from "./ui";

/** Start/stop the saved local port forwards of one host. */
export function ForwardsDialog() {
  const { forwardsHost, setForwardsHost, hosts, forwards, refreshForwards, setHostEditor, error, toast } = useApp();
  const [busy, setBusy] = useState<string | null>(null);
  useEffect(() => {
    if (forwardsHost) refreshForwards();
  }, [forwardsHost, refreshForwards]);
  const host = hosts.find((h) => h.id === forwardsHost);
  if (!forwardsHost || !host) return null;
  const isActive = (fid: string) => forwards.some((f) => f.hostId === host.id && f.forwardId === fid);

  const toggle = async (fid: string, port: number) => {
    setBusy(fid);
    try {
      if (isActive(fid)) {
        await api.forwardStop(host.id, fid);
        toast(`Stopped forward on port ${port}`);
      } else {
        await api.forwardStart(host.id, fid);
        toast(`Forwarding 127.0.0.1:${port}`, "success");
      }
      await refreshForwards();
    } catch (e) {
      error(e);
    } finally {
      setBusy(null);
    }
  };

  return (
    <Modal
      title={`Port forwards · ${host.name}`}
      onClose={() => setForwardsHost(null)}
      width="max-w-2xl"
      footer={
        <>
          <Button
            variant="ghost"
            onClick={() => {
              setForwardsHost(null);
              setHostEditor(host);
            }}
          >
            <Pencil size={14} /> Edit forwards
          </Button>
          <div className="flex-1" />
          <Button variant="primary" onClick={() => setForwardsHost(null)}>
            Done
          </Button>
        </>
      }
    >
      {host.forwards.length === 0 ? (
        <Empty title="No forwards configured">Add one in the host editor under “Port forwards”.</Empty>
      ) : (
        <div className="divide-y divide-border overflow-hidden rounded-xl border border-border">
          {host.forwards.map((f) => {
            const on = isActive(f.id);
            return (
              <div key={f.id} className="flex items-center gap-4 bg-surface-1 px-4 py-3">
                <span className={on ? "h-2 w-2 rounded-full bg-ok shadow-[0_0_0_3px] shadow-ok/20" : "h-2 w-2 rounded-full bg-fg-subtle"} />
                <div className="flex min-w-0 flex-1 items-center gap-2 font-mono text-sm">
                  <span>127.0.0.1:{f.localPort}</span>
                  <ArrowRight size={14} className="shrink-0 text-fg-subtle" />
                  <span className="truncate text-fg-muted">
                    {f.remoteHost}:{f.remotePort}
                  </span>
                  {f.autoStart && <Badge tone="info">auto</Badge>}
                </div>
                {on && (
                  <IconButton title={`Copy http://localhost:${f.localPort}`} onClick={() => navigator.clipboard.writeText(`http://localhost:${f.localPort}`).then(() => toast("URL copied"))}>
                    <ExternalLink size={14} />
                  </IconButton>
                )}
                <Button size="sm" variant={on ? "subtle" : "primary"} disabled={busy === f.id} onClick={() => toggle(f.id, f.localPort)}>
                  {busy === f.id ? <Loader2 size={13} className="animate-spin" /> : on ? <Square size={13} /> : <Play size={13} />}
                  {on ? "Stop" : "Start"}
                </Button>
              </div>
            );
          })}
        </div>
      )}
      <p className="mt-3 text-xs text-fg-subtle">
        Forwards listen on 127.0.0.1 only and reconnect the SSH session automatically if it drops. “Auto” forwards start when the host first connects.
      </p>
    </Modal>
  );
}
