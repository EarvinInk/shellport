import { ask } from "@tauri-apps/plugin-dialog";
import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal, type ITheme } from "@xterm/xterm";
import { RotateCcw, Search, WifiOff, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, b64ToBytes, binaryStringToB64, errMsg, type Opener } from "../lib/ipc";
import type { TermEvent } from "../lib/types";
import { useApp } from "../store";
import { Button, IconButton } from "./ui";

export const THEMES: Record<string, ITheme> = {
  "shellport-dark": {
    background: "#0b0e13",
    foreground: "#dfe4ec",
    cursor: "#a78bfa",
    cursorAccent: "#0b0e13",
    selectionBackground: "#8b5cf655",
    black: "#1b2029",
    red: "#f7768e",
    green: "#9ece6a",
    yellow: "#e0af68",
    blue: "#7aa2f7",
    magenta: "#bb9af7",
    cyan: "#2ac3de",
    white: "#c0caf5",
    brightBlack: "#414868",
    brightRed: "#ff8fa3",
    brightGreen: "#b9f27c",
    brightYellow: "#ffc777",
    brightBlue: "#8db0ff",
    brightMagenta: "#c7a9ff",
    brightCyan: "#5fd7ef",
    brightWhite: "#ffffff",
  },
  "solarized-dark": {
    background: "#002b36",
    foreground: "#93a1a1",
    cursor: "#93a1a1",
    selectionBackground: "#073642",
    black: "#073642",
    red: "#dc322f",
    green: "#859900",
    yellow: "#b58900",
    blue: "#268bd2",
    magenta: "#d33682",
    cyan: "#2aa198",
    white: "#eee8d5",
    brightBlack: "#586e75",
    brightRed: "#cb4b16",
    brightGreen: "#586e75",
    brightYellow: "#657b83",
    brightBlue: "#839496",
    brightMagenta: "#6c71c4",
    brightCyan: "#93a1a1",
    brightWhite: "#fdf6e3",
  },
  dracula: {
    background: "#1e1f29",
    foreground: "#f8f8f2",
    cursor: "#f8f8f2",
    selectionBackground: "#44475a",
    black: "#21222c",
    red: "#ff5555",
    green: "#50fa7b",
    yellow: "#f1fa8c",
    blue: "#bd93f9",
    magenta: "#ff79c6",
    cyan: "#8be9fd",
    white: "#f8f8f2",
    brightBlack: "#6272a4",
    brightRed: "#ff6e6e",
    brightGreen: "#69ff94",
    brightYellow: "#ffffa5",
    brightBlue: "#d6acff",
    brightMagenta: "#ff92df",
    brightCyan: "#a4ffff",
    brightWhite: "#ffffff",
  },
  "light": {
    background: "#fafafa",
    foreground: "#383a42",
    cursor: "#7c3aed",
    selectionBackground: "#7c3aed33",
    black: "#383a42",
    red: "#e45649",
    green: "#50a14f",
    yellow: "#c18401",
    blue: "#4078f2",
    magenta: "#a626a4",
    cyan: "#0184bc",
    white: "#a0a1a7",
    brightBlack: "#4f525e",
    brightRed: "#e06c75",
    brightGreen: "#98c379",
    brightYellow: "#e5c07b",
    brightBlue: "#61afef",
    brightMagenta: "#c678dd",
    brightCyan: "#56b6c2",
    brightWhite: "#ffffff",
  },
};

interface Props {
  /** Opens the backend session; receives initial size and the output channel. */
  open: Opener;
  active: boolean;
  /** Read-only output (log viewer): keyboard input is not forwarded. */
  readOnly?: boolean;
  onTitle?: (t: string) => void;
  /** Called with the backend id once opened (e.g. to cancel log tasks). */
  onOpened?: (id: string) => void;
  onClosed?: (id: string | null) => void;
  /** Bump to tear down and re-open the session. */
  generation?: number;
  toolbar?: React.ReactNode;
  /** What to do when the connection drops (SSH): reconnect with backoff, ask, or close the tab. */
  onLost?: "reconnect" | "ask" | "close";
  /** SSH host to ping after sleep or a network change, so a dead link is noticed quickly. */
  probeHostId?: string;
  /** Close the tab (used by the "close" disconnect policy). */
  onCloseTab?: () => void;
}

type Status = "connecting" | "open" | "closed" | "error" | "lost";

/** Seconds to wait before each automatic reconnect attempt. */
const BACKOFF = [1, 2, 5, 10, 30];
/** Errors that retrying won't fix: stop and show them. */
const FATAL = /unlock the vault|authentication|permission denied|host key|no .* secret selected|cancelled/i;
/** Output flow control: pause the backend above HIGH bytes waiting to be drawn, resume below LOW. */
const FLOW_HIGH = 2 * 1024 * 1024;
const FLOW_LOW = 512 * 1024;

export function TerminalView({ open, active, readOnly, onTitle, onOpened, onClosed, generation = 0, toolbar, onLost, probeHostId, onCloseTab }: Props) {
  const settings = useApp((s) => s.settings);
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const search = useRef<SearchAddon | null>(null);
  const sessionId = useRef<string | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  // Automatic reconnect: attempt number, seconds left, waiting for the network.
  const attempt = useRef(0);
  const auto = useRef(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [offline, setOffline] = useState(false);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);
  // Bytes handed to xterm but not drawn yet (flow control).
  const pending = useRef(0);
  const flowPaused = useRef(false);

  const openRef = useRef(open);
  openRef.current = open;
  const cbRef = useRef({ onTitle, onOpened, onClosed, onLost, onCloseTab });
  cbRef.current = { onTitle, onOpened, onClosed, onLost, onCloseTab };

  const stopTimer = () => {
    if (timer.current) clearInterval(timer.current);
    timer.current = null;
  };

  /** Wait (backoff), then reopen; if offline, wait for the network first. */
  const scheduleReconnect = useCallback(() => {
    stopTimer();
    auto.current = true;
    if (!navigator.onLine) {
      setOffline(true);
      setCountdown(null);
      return;
    }
    setOffline(false);
    let left = BACKOFF[Math.min(attempt.current, BACKOFF.length - 1)];
    attempt.current += 1;
    setCountdown(left);
    timer.current = setInterval(() => {
      left -= 1;
      if (left <= 0) {
        stopTimer();
        setCountdown(null);
        setRetry((r) => r + 1);
      } else setCountdown(left);
    }, 1000);
  }, []);

  const stopReconnect = useCallback(() => {
    stopTimer();
    auto.current = false;
    attempt.current = 0;
    setCountdown(null);
    setOffline(false);
    setStatus("closed");
    setError("Reconnect stopped");
  }, []);

  useEffect(() => stopTimer, []);

  // Back online: reconnect now instead of waiting.
  useEffect(() => {
    if (!offline) return;
    const h = () => {
      setOffline(false);
      setRetry((r) => r + 1);
    };
    window.addEventListener("online", h);
    return () => window.removeEventListener("online", h);
  }, [offline]);

  // After sleep or a network change, ping the server so a dead connection is
  // noticed in seconds instead of after the keepalive timeout.
  useEffect(() => {
    if (!probeHostId || status !== "open") return;
    let last = Date.now();
    const probe = () => api.sshProbe(probeHostId).catch(() => {});
    const iv = setInterval(() => {
      const now = Date.now();
      if (now - last > 20_000) probe();
      last = now;
    }, 5000);
    window.addEventListener("online", probe);
    return () => {
      clearInterval(iv);
      window.removeEventListener("online", probe);
    };
  }, [probeHostId, status]);

  // Create xterm once.
  useEffect(() => {
    const t = new Terminal({
      allowProposedApi: true,
      fontFamily: settings?.fontFamily,
      fontSize: settings?.fontSize ?? 14,
      // A little extra height keeps underscores from being clipped (DejaVu Sans Mono).
      lineHeight: 1.15,
      scrollback: settings?.scrollback ?? 10000,
      cursorStyle: settings?.cursorStyle ?? "block",
      cursorBlink: settings?.cursorBlink ?? true,
      theme: THEMES[settings?.theme ?? "shellport-dark"] ?? THEMES["shellport-dark"],
      disableStdin: readOnly,
      convertEol: readOnly,
      // OSC 8 hyperlinks printed by remote programs.
      linkHandler: { activate: (_e, uri) => openExternal(uri), allowNonHttpProtocols: false },
    });
    const f = new FitAddon();
    const s = new SearchAddon();
    t.loadAddon(f);
    t.loadAddon(s);
    t.loadAddon(new WebLinksAddon((_e, uri) => openExternal(uri)));
    const u = new Unicode11Addon();
    t.loadAddon(u);
    t.unicode.activeVersion = "11";
    t.open(host.current!);
    term.current = t;
    fit.current = f;
    search.current = s;

    t.attachCustomKeyEventHandler((e) => {
      if (e.type !== "keydown") return true;
      // Ctrl+Shift+C / V copy & paste, Ctrl+Shift+F search.
      if (e.ctrlKey && e.shiftKey && e.code === "KeyC") {
        const sel = t.getSelection();
        if (sel) navigator.clipboard.writeText(sel);
        return false;
      }
      if (e.ctrlKey && e.shiftKey && e.code === "KeyV") {
        navigator.clipboard.readText().then((txt) => {
          // paste() applies bracketed paste, so the shell doesn't run pasted newlines.
          if (sessionId.current && !readOnly) t.paste(txt);
        });
        return false;
      }
      if (e.ctrlKey && e.shiftKey && e.code === "KeyF") {
        setSearchOpen(true);
        return false;
      }
      return true;
    });
    t.onTitleChange((title) => cbRef.current.onTitle?.(title));
    t.onSelectionChange(() => {
      if (useApp.getState().settings?.copyOnSelect) {
        const sel = t.getSelection();
        if (sel) navigator.clipboard.writeText(sel);
      }
    });
    if (!readOnly) {
      t.onData((d) => sessionId.current && api.termWrite(sessionId.current, d).catch(() => {}));
      t.onBinary((d) => sessionId.current && api.termWriteB64(sessionId.current, binaryStringToB64(d)).catch(() => {}));
    }
    t.onResize(({ cols, rows }) => {
      if (sessionId.current && !readOnly) api.termResize(sessionId.current, cols, rows).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      if (host.current && host.current.offsetParent !== null) {
        try {
          f.fit();
        } catch {
          /* not visible */
        }
      }
    });
    ro.observe(host.current!);
    return () => {
      ro.disconnect();
      t.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Apply settings changes live.
  useEffect(() => {
    const t = term.current;
    if (!t || !settings) return;
    t.options.fontFamily = settings.fontFamily;
    t.options.fontSize = settings.fontSize;
    t.options.cursorStyle = settings.cursorStyle;
    t.options.cursorBlink = settings.cursorBlink;
    t.options.theme = THEMES[settings.theme] ?? THEMES["shellport-dark"];
    try {
      fit.current?.fit();
    } catch {
      /* ignore */
    }
  }, [settings]);

  // (Re)open the backend session.
  useEffect(() => {
    const t = term.current!;
    let cancelled = false;
    let myId: string | null = null;
    setStatus("connecting");
    setError(null);
    try {
      fit.current?.fit();
    } catch {
      /* ignore */
    }
    pending.current = 0;
    flowPaused.current = false;
    const ch = new Channel<TermEvent>();
    ch.onmessage = (ev) => {
      if (cancelled) return;
      if (ev.type === "data") {
        const bytes = b64ToBytes(ev.data);
        pending.current += bytes.length;
        t.write(bytes, () => {
          pending.current -= bytes.length;
          const id = sessionId.current ?? myId;
          if (flowPaused.current && pending.current < FLOW_LOW && id) {
            flowPaused.current = false;
            api.termFlow(id, false).catch(() => {});
          }
        });
        const id = sessionId.current ?? myId;
        if (!flowPaused.current && pending.current > FLOW_HIGH && id) {
          flowPaused.current = true;
          api.termFlow(id, true).catch(() => {});
        }
      } else if (ev.type === "lost") {
        sessionId.current = null;
        cbRef.current.onClosed?.(myId);
        t.write(`\r\n\x1b[33m[connection lost: ${ev.reason}]\x1b[0m\r\n`);
        setStatus("lost");
        const policy = cbRef.current.onLost ?? "ask";
        setError(policy === "ask" ? `Connection lost: ${ev.reason}` : null);
        if (policy === "reconnect") scheduleReconnect();
        else if (policy === "close") cbRef.current.onCloseTab?.();
      } else if (ev.type === "error") {
        t.write(`\r\n\x1b[31m${ev.message}\x1b[0m\r\n`);
      } else if (ev.type === "exit") {
        setStatus("closed");
        t.write(`\r\n\x1b[90m[session ended${ev.code != null ? ` with code ${ev.code}` : ""}]\x1b[0m\r\n`);
        cbRef.current.onClosed?.(myId);
        sessionId.current = null;
      }
    };
    openRef
      .current(t.cols || 80, t.rows || 24, ch)
      .then((id) => {
        if (cancelled) {
          api.termClose(id).catch(() => {});
          api.taskCancel(id).catch(() => {});
          return;
        }
        myId = id;
        sessionId.current = id;
        if (auto.current) t.write("\x1b[32m[reconnected]\x1b[0m\r\n");
        auto.current = false;
        attempt.current = 0;
        setStatus((s) => (s === "connecting" ? "open" : s));
        cbRef.current.onOpened?.(id);
        if (!readOnly) api.termResize(id, t.cols, t.rows).catch(() => {});
        if (active && !readOnly) t.focus();
      })
      .catch((e) => {
        if (cancelled) return;
        const msg = errMsg(e);
        // While reconnecting on our own, keep trying unless retrying can't help.
        if (auto.current && !FATAL.test(msg)) {
          t.write(`\x1b[90m[reconnect failed: ${msg}]\x1b[0m\r\n`);
          setStatus("lost");
          scheduleReconnect();
          return;
        }
        auto.current = false;
        setStatus("error");
        setError(msg);
        useApp.getState().error(e);
      });
    return () => {
      cancelled = true;
      if (myId) {
        api.termClose(myId).catch(() => {});
        api.taskCancel(myId).catch(() => {});
      }
      sessionId.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [retry, generation]);

  useEffect(() => {
    if (active) {
      requestAnimationFrame(() => {
        try {
          fit.current?.fit();
        } catch {
          /* ignore */
        }
        if (!readOnly) term.current?.focus();
      });
    }
  }, [active, readOnly]);

  const reconnect = useCallback(() => {
    stopTimer();
    setCountdown(null);
    setOffline(false);
    term.current?.write("\r\n\x1b[90m[reconnecting…]\x1b[0m\r\n");
    setRetry((r) => r + 1);
  }, []);

  const doSearch = (next: boolean) => {
    if (!query) return;
    const opts = { decorations: { matchOverviewRuler: "#8b5cf6", activeMatchColorOverviewRuler: "#f59e0b", matchBackground: "#8b5cf655", activeMatchBackground: "#f59e0b88" } };
    if (next) search.current?.findNext(query, opts);
    else search.current?.findPrevious(query, opts);
  };

  return (
    <div className="relative flex h-full flex-col">
      {toolbar}
      <div className="relative min-h-0 flex-1 bg-[var(--term-bg)]" style={{ ["--term-bg" as string]: THEMES[settings?.theme ?? "shellport-dark"]?.background }}>
        <div ref={host} className="absolute inset-0" />
        {status === "connecting" && (
          <div className="pointer-events-none absolute right-3 top-2 rounded bg-surface-3/90 px-2 py-1 text-xs text-fg-muted">Connecting…</div>
        )}
        {status === "lost" && !readOnly && (countdown !== null || offline) && (
          <div className="absolute bottom-4 right-4 flex items-center gap-2 rounded-lg border border-warn/40 bg-surface-2/95 px-3 py-2 shadow-lg" role="status">
            {offline ? <WifiOff size={14} className="text-warn" /> : <RotateCcw size={14} className="animate-spin text-warn [animation-direction:reverse]" />}
            <span className="text-xs text-fg-muted">
              {offline
                ? "Connection lost. Waiting for the network…"
                : `Connection lost. Reconnecting in ${countdown}s${attempt.current > 1 ? ` (attempt ${attempt.current})` : ""}`}
            </span>
            <Button size="sm" variant="primary" onClick={reconnect}>
              Reconnect now
            </Button>
            <Button size="sm" variant="ghost" onClick={stopReconnect}>
              Stop
            </Button>
          </div>
        )}
        {(status === "closed" || status === "error" || (status === "lost" && countdown === null && !offline && !auto.current)) && !readOnly && (
          <div className="absolute bottom-4 right-4 flex items-center gap-2 rounded-lg border border-border bg-surface-2/95 px-3 py-2 shadow-lg">
            <span className="max-w-md truncate text-xs text-fg-muted">{error ?? "Session closed"}</span>
            <Button size="sm" variant="primary" onClick={reconnect}>
              <RotateCcw size={13} /> Reconnect
            </Button>
          </div>
        )}
        {searchOpen && (
          <div className="absolute right-4 top-2 z-10 flex items-center gap-1 rounded-lg border border-border bg-surface-2 p-1 shadow-lg">
            <Search size={14} className="ml-1 text-fg-subtle" />
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") doSearch(!e.shiftKey);
                if (e.key === "Escape") {
                  setSearchOpen(false);
                  search.current?.clearDecorations();
                  term.current?.focus();
                }
              }}
              placeholder="Find (Enter / Shift+Enter)"
              className="h-7 w-56 bg-transparent px-1 text-sm outline-none"
            />
            <IconButton
              title="Close search"
              onClick={() => {
                setSearchOpen(false);
                search.current?.clearDecorations();
              }}
            >
              <X size={14} />
            </IconButton>
          </div>
        )}
      </div>
    </div>
  );
}

/** Open a link from terminal output in the system browser, after showing the full URL. */
async function openExternal(uri: string) {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return;
  const ok = await ask(`Open this link in your browser?\n\n${url.href}`, { title: "Open link", kind: "info", okLabel: "Open", cancelLabel: "Cancel" });
  if (ok) api.openUrl(url.href).catch(() => {});
}
