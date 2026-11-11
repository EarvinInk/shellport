// The parts of the File System Access API that trzsz.js uses
// (`showOpenFilePicker`, `showDirectoryPicker` and their handles), built on
// native dialogs and backend commands. WebKitGTK, Tauri's engine on Linux,
// doesn't have them. Only picked files and folders are reachable (the backend
// enforces that).
import { api } from "./ipc";

const abort = () => new DOMException("The user cancelled the dialog", "AbortError");
const notFound = (name: string) => new DOMException(`${name} not found`, "NotFoundError");
const base = (p: string) => p.split("/").filter(Boolean).pop() ?? p;


/** A File-like object read in chunks on demand (big files never load into memory). */
function lazyFile(path: string, name: string, size: number) {
  return {
    name,
    size,
    type: "",
    slice(start = 0, end = size) {
      const from = Math.max(0, start);
      const to = Math.min(size, end);
      return { size: to - from, arrayBuffer: () => (to > from ? api.localFsRead(path, from, to - from) : Promise.resolve(new ArrayBuffer(0))) };
    },
  };
}

function fileHandle(path: string, name: string, size: number) {
  return {
    kind: "file" as const,
    name,
    getFile: async () => lazyFile(path, name, size),
    createWritable: async () => ({
      write: async (data: ArrayBuffer | Uint8Array) => api.localFsAppendBytes(path, data instanceof Uint8Array ? data : new Uint8Array(data)),
      close: async () => {},
      abort: async () => {},
    }),
    queryPermission: async () => "granted",
    requestPermission: async () => "granted",
  };
}

type FileHandle = ReturnType<typeof fileHandle>;
interface DirHandle {
  kind: "directory";
  name: string;
  values(): AsyncGenerator<DirHandle | FileHandle>;
  getFileHandle(child: string, opts?: { create?: boolean }): Promise<FileHandle>;
  getDirectoryHandle(child: string, opts?: { create?: boolean }): Promise<DirHandle>;
  queryPermission(): Promise<string>;
  requestPermission(): Promise<string>;
}

function dirHandle(path: string, name: string): DirHandle {
  const find = async (child: string) => (await api.localFsList(path)).find((e) => e.name === child);
  return {
    kind: "directory" as const,
    name,
    async *values() {
      for (const e of await api.localFsList(path)) yield e.isDir ? dirHandle(e.path, e.name) : fileHandle(e.path, e.name, e.size);
    },
    async getFileHandle(child: string, opts?: { create?: boolean }) {
      if (opts?.create) return fileHandle(await api.localFsCreate(path, child, false), child, 0);
      const e = await find(child);
      if (!e || e.isDir) throw notFound(child);
      return fileHandle(e.path, e.name, e.size);
    },
    async getDirectoryHandle(child: string, opts?: { create?: boolean }): Promise<DirHandle> {
      if (opts?.create) return dirHandle(await api.localFsCreate(path, child, true), child);
      const e = await find(child);
      if (!e || !e.isDir) throw notFound(child);
      return dirHandle(e.path, e.name);
    },
    queryPermission: async () => "granted",
    requestPermission: async () => "granted",
  };
}

/** Pick files to send; resolves to file handles. */
export async function pickFiles(title = "Choose files to upload") {
  const paths = await api.localFsPick(false, true, false, title);
  if (!paths.length) throw abort();
  return Promise.all(
    paths.map(async (p) => {
      const s = await api.localFsStat(p);
      return fileHandle(s.path, s.name, s.size);
    }),
  );
}

/** Pick a folder (to save into when `write`, or to send). */
export async function pickFolder(write: boolean, title?: string) {
  const [p] = await api.localFsPick(true, false, write, title ?? (write ? "Choose where to save the files" : "Choose a folder to upload"));
  if (!p) throw abort();
  return dirHandle(p, base(p));
}

/** Install the pickers where the browser engine lacks them. */
export function installFsAccess() {
  const w = window as unknown as Record<string, unknown>;
  if (typeof w.showOpenFilePicker !== "function") w.showOpenFilePicker = () => pickFiles();
  if (typeof w.showDirectoryPicker !== "function") w.showDirectoryPicker = (opts?: { mode?: string }) => pickFolder(opts?.mode === "readwrite");
}
