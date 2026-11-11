// In-terminal file transfers (M3.1): trzsz (`trz`/`tsz`) through trzsz.js, and
// ZModem (`rz`/`sz`, opt-in per host) through zmodem.js. They sit between the
// SSH session and xterm.js; files go through the native pickers in fsAccess.ts.
import { TrzszFilter } from "trzsz";
import Zmodem from "zmodem.js";
import { installFsAccess, pickFiles } from "./fsAccess";
import { api } from "./ipc";

export interface TermTransfers {
  /** Server output; returns nothing (it reaches the screen through `write`). */
  output(bytes: Uint8Array): void;
  /** Keyboard input. */
  input(data: string): void;
  resize(cols: number): void;
}

interface Options {
  /** Show on screen. */
  write(data: string | Uint8Array): void;
  /** Send to the server. */
  send(data: string | Uint8Array): Promise<void>;
  cols: number;
  zmodem: boolean;
  notify(text: string, kind?: "info" | "error"): void;
}

const enc = new TextEncoder();
const toBytes = (o: string | ArrayBuffer | Uint8Array | Blob): Uint8Array | null =>
  typeof o === "string" ? enc.encode(o) : o instanceof Uint8Array ? o : o instanceof ArrayBuffer ? new Uint8Array(o) : null;


/** "name", or "name.1", "name.2"… if taken (never replace a local file). */
async function freeName(dir: string, name: string): Promise<string> {
  const taken = new Set((await api.localFsList(dir)).map((e) => e.name));
  if (!taken.has(name)) return name;
  for (let i = 1; i < 1000; i++) if (!taken.has(`${name}.${i}`)) return `${name}.${i}`;
  throw new Error(`too many files named ${name}`);
}

export function createTermTransfers(o: Options): TermTransfers {
  installFsAccess();
  // Bytes we send are queued, so big ZModem uploads don't flood the connection.
  let sendQ: Promise<void> = Promise.resolve();
  const queueSend = (d: string | Uint8Array) => {
    sendQ = sendQ.then(() => o.send(d)).catch(() => {});
    return sendQ;
  };

  // ZModem (only when the host allows it).
  let zsession: { abort(): void; has_ended(): boolean } | null = null;
  const sentry = o.zmodem
    ? new Zmodem.Sentry({
        to_terminal: (octets: number[]) => o.write(Uint8Array.from(octets)),
        sender: (octets: number[]) => void queueSend(Uint8Array.from(octets)),
        on_retract: () => {},
        on_detect: (det: ZDetection) => {
          const z = det.confirm();
          zsession = z;
          z.on("session_end", () => (zsession = null));
          (z.type === "send" ? zmodemSend(z) : zmodemReceive(z)).catch((e) => {
            o.notify(`ZModem: ${e instanceof Error ? e.message : String(e)}`, "error");
            if (!z.has_ended()) {
              try {
                z.abort();
              } catch {
                /* already over */
              }
            }
          });
        },
      })
    : null;

  /** The server runs `rz`: send files we pick. */
  async function zmodemSend(z: ZSession) {
    let handles: Awaited<ReturnType<typeof pickFiles>>;
    try {
      handles = await pickFiles("Choose files to send with ZModem (rz)");
    } catch {
      await z.close();
      return;
    }
    const files = await Promise.all(handles.map((h) => h.getFile()));
    let bytesLeft = files.reduce((a, f) => a + f.size, 0);
    for (const [i, f] of files.entries()) {
      const xfer = await z.send_offer({ name: f.name, size: f.size, mtime: new Date(), files_remaining: files.length - i, bytes_remaining: bytesLeft });
      bytesLeft -= f.size;
      if (!xfer) continue; // the server skipped it
      for (let off = 0; off < f.size; off += 64 * 1024) {
        const buf = new Uint8Array(await f.slice(off, Math.min(f.size, off + 64 * 1024)).arrayBuffer());
        xfer.send(buf);
        await sendQ;
      }
      await xfer.end();
    }
    await z.close();
    o.notify(`ZModem: sent ${files.length} file${files.length === 1 ? "" : "s"}`);
  }

  /** The server runs `sz`: save what it offers into a folder we pick. */
  async function zmodemReceive(z: ZSession) {
    const [dir] = await api.localFsPick(true, false, true, "Choose where to save the files from sz");
    if (!dir) {
      z.abort();
      return;
    }
    const ended = new Promise<void>((r) => z.on("session_end", () => r()));
    const saving: Promise<void>[] = [];
    let saved = 0;
    z.on("offer", (x) => {
      const name = (x.get_details().name.split("/").pop() ?? "file").replace(/\0/g, "") || "file";
      let path = "";
      let failed: unknown = null;
      // Writes are queued in order; the protocol hands us data synchronously.
      let chain: Promise<unknown> = freeName(dir, name)
        .then((n) => api.localFsCreate(dir, n, false))
        .then((p) => (path = p))
        .catch((e) => (failed = e));
      const done = x
        .accept({
          on_input: (payload) => {
            const b = Uint8Array.from(payload);
            chain = chain.then(() => (failed ? undefined : api.localFsAppendBytes(path, b))).catch((e) => (failed = e));
          },
        })
        .then(() => chain)
        .then(() => {
          if (failed) throw failed;
          saved++;
        })
        .catch((e) => o.notify(`ZModem: couldn't save ${name}: ${e instanceof Error ? e.message : String(e)}`, "error"));
      saving.push(done);
    });
    z.start();
    await ended;
    await Promise.all(saving);
    o.notify(`ZModem: saved ${saved} file${saved === 1 ? "" : "s"} to ${dir}`);
  }

  const trzsz = new TrzszFilter({
    writeToTerminal: (out) => {
      if (sentry) {
        const b = toBytes(out as string | ArrayBuffer | Uint8Array);
        if (b) sentry.consume(b);
      } else o.write(typeof out === "string" ? out : (toBytes(out as ArrayBuffer | Uint8Array) ?? new Uint8Array()));
    },
    sendToServer: (input) => void queueSend(input),
    terminalColumns: o.cols,
  });

  return {
    output: (bytes) => trzsz.processServerOutput(bytes),
    input: (data) => {
      if (zsession && !zsession.has_ended()) {
        // Ctrl+C stops a ZModem transfer; other keys wait until it's over.
        if (data === "\x03") zsession.abort();
        return;
      }
      trzsz.processTerminalInput(data);
    },
    resize: (cols) => trzsz.setTerminalColumns(cols),
  };
}

// Just the parts of zmodem.js used here.
interface ZOffer {
  get_details(): { name: string; size?: number };
  accept(opts: { on_input: (payload: number[]) => void }): Promise<void>;
  skip(): void;
}
interface ZTransfer {
  send(bytes: Uint8Array): void;
  end(bytes?: Uint8Array): Promise<void>;
}
interface ZSession {
  type: "send" | "receive";
  on(ev: "offer", f: (x: ZOffer) => void): void;
  on(ev: "session_end", f: () => void): void;
  start(): void;
  close(): Promise<void>;
  abort(): void;
  has_ended(): boolean;
  send_offer(p: { name: string; size: number; mtime: Date; files_remaining: number; bytes_remaining: number }): Promise<ZTransfer | undefined>;
}
interface ZDetection {
  confirm(): ZSession;
  deny(): void;
}
