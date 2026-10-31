import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TalkKey = "off" | "fn" | "right-option" | "right-command" | "right-control" | "right-shift";

export type DictationSettings = { key: TalkKey; cleanup: boolean; spokenEdits: boolean; voiceFixes: boolean };

export type DictationStatus = {
  ready: boolean;
  accessibility: boolean;
  fnConflict: boolean;
  cleanup: string | null;
  error: string | null;
};

export const TALK_KEYS: { key: TalkKey; label: string; symbol: string }[] = [
  { key: "off", label: "Off", symbol: "" },
  { key: "fn", label: "Fn", symbol: "🌐" },
  { key: "right-option", label: "Right Option", symbol: "⌥" },
  { key: "right-command", label: "Right Command", symbol: "⌘" },
  { key: "right-control", label: "Right Control", symbol: "⌃" },
  { key: "right-shift", label: "Right Shift", symbol: "⇧" },
];

export const VOICE_COMMANDS: { setting: "spokenEdits" | "voiceFixes"; title: string; detail: string }[] = [
  {
    setting: "spokenEdits",
    title: "Edit while you speak",
    detail:
      'While you talk, say "delete the last line", "delete" and any words you said, "scratch that", "start over" or "new line". The notch shows the result live and the commands are never typed.',
  },
  {
    setting: "voiceFixes",
    title: "Fix what was typed",
    detail:
      'Hold the key and say only a command, like "delete that", "delete the last word" or "delete" and some words, to change what Aube typed since you last typed or clicked.',
  },
];

class Dictation {
  settings = $state<DictationSettings>({ key: "off", cleanup: true, spokenEdits: true, voiceFixes: true });
  status = $state<DictationStatus | null>(null);
  error = $state("");

  async init() {
    listen<DictationStatus | null>("dictation-status", ({ payload }) => (this.status = payload));
    const overview = await invoke<{ settings: DictationSettings; status: DictationStatus | null }>("dictation");
    this.settings = overview.settings;
    this.status = overview.status;
  }

  async update(change: Partial<DictationSettings>) {
    this.error = "";
    const next = { ...this.settings, ...change };
    try {
      await invoke("set_dictation", { settings: next });
      this.settings = next;
    } catch (e) {
      this.error = String(e);
    }
  }

  openSystemSettings(pane: "accessibility" | "keyboard") {
    invoke("open_system_settings", { pane }).catch((e) => (this.error = String(e)));
  }
}

export const dictation = new Dictation();
