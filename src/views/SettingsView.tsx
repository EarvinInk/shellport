import { useEffect, useState } from "react";
import { BackupSection } from "../components/BackupSection";
import { THEMES } from "../components/TerminalView";
import { Button, Checkbox, Field, Input, Select } from "../components/ui";
import type { Settings } from "../lib/types";
import { useApp } from "../store";

export function SettingsView() {
  const { settings, saveSettings } = useApp();
  const [s, setS] = useState<Settings | null>(settings);
  useEffect(() => setS(settings), [settings]);
  if (!s) return null;
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS({ ...s, [k]: v });
  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl space-y-8 p-6">
        <section className="space-y-3">
          <h2 className="text-sm font-semibold">Terminal</h2>
          <div className="grid grid-cols-2 gap-3">
            <Field label="Theme">
              <Select value={s.theme} onChange={(e) => set("theme", e.target.value)}>
                {Object.keys(THEMES).map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label="Font size">
              <Input type="number" min={8} max={32} value={s.fontSize} onChange={(e) => set("fontSize", Number(e.target.value) || 14)} />
            </Field>
            <Field label="Font family" className="col-span-2">
              <Input value={s.fontFamily} onChange={(e) => set("fontFamily", e.target.value)} />
            </Field>
            <Field label="Cursor">
              <Select value={s.cursorStyle} onChange={(e) => set("cursorStyle", e.target.value as Settings["cursorStyle"])}>
                <option value="block">Block</option>
                <option value="bar">Bar</option>
                <option value="underline">Underline</option>
              </Select>
            </Field>
            <Field label="Scrollback lines">
              <Input type="number" value={s.scrollback} onChange={(e) => set("scrollback", Number(e.target.value) || 1000)} />
            </Field>
            <Field label="Local shell" className="col-span-2">
              <Input value={s.localShell} onChange={(e) => set("localShell", e.target.value)} />
            </Field>
          </div>
          <div className="flex gap-6">
            <Checkbox label="Blinking cursor" checked={s.cursorBlink} onChange={(v) => set("cursorBlink", v)} />
            <Checkbox label="Copy on select" checked={s.copyOnSelect} onChange={(v) => set("copyOnSelect", v)} />
            <Checkbox label="Restore tabs on startup" checked={s.restoreTabs} onChange={(v) => set("restoreTabs", v)} />
          </div>
          <div className="flex flex-col gap-1">
            <Checkbox label="Show whether hosts are up" checked={s.hostStatusChecks} onChange={(v) => set("hostStatusChecks", v)} />
            <span className="pl-6 text-xs text-fg-subtle">
              Briefly connects to each host's SSH port every few minutes while Shellport is visible (no login). Some intrusion-prevention tools log these short connections; turn this off if yours does.
            </span>
          </div>
          <p className="text-xs text-fg-subtle">Shortcuts: Ctrl+Shift+C copy · Ctrl+Shift+V paste · Ctrl+Shift+F search · Ctrl+Shift+T new local tab · Ctrl+Shift+W close tab</p>
        </section>
        <section className="space-y-3">
          <h2 className="text-sm font-semibold">Security</h2>
          <Field label="Auto-lock vault after (minutes, 0 = never)">
            <Input type="number" min={0} value={s.autoLockMinutes} onChange={(e) => set("autoLockMinutes", Math.max(0, Number(e.target.value) || 0))} />
          </Field>
        </section>
        <section className="space-y-3">
          <h2 className="text-sm font-semibold">Docker</h2>
          <Field label="Default log tail (lines)">
            <Input type="number" value={s.dockerLogTail} onChange={(e) => set("dockerLogTail", Number(e.target.value) || 100)} />
          </Field>
        </section>
        <Button variant="primary" onClick={() => saveSettings(s)}>
          Save settings
        </Button>
        <div className="border-t border-border pt-8">
          <BackupSection />
        </div>
      </div>
    </div>
  );
}
