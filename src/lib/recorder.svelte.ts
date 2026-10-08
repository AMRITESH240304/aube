import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type Status = {
  id: string;
  title: string;
  elapsedSecs: number;
  paused: boolean;
  micLevel: number;
  systemLevel: number;
  systemError: string | null;
  transcriptionError: string | null;
};

export type Recording = {
  id: string;
  title: string;
  startedAt: number;
  durationSecs: number;
  recovered: boolean;
  transcribed: boolean;
  path: string;
};

export type LiveTurn = { index: number; speaker: string; start: number; text: string };
type LiveText = LiveTurn & { recordingId: string };

const SILENCE_HINT_SECS = 5;

class Recorder {
  status = $state<Status | null>(null);
  recordings = $state<Recording[]>([]);
  loaded = $state(false);
  error = $state("");
  heardSound = $state(false);
  saving = $state(false);
  // Rough preview while recording; each turn's newer text replaces its older guess.
  live = $state<LiveTurn[]>([]);

  #poll?: ReturnType<typeof setInterval>;

  get silent() {
    return (
      !!this.status && !this.status.paused && !this.heardSound && this.status.elapsedSecs > SILENCE_HINT_SECS
    );
  }

  async init() {
    listen<LiveText>("live-text", ({ payload: { recordingId, ...turn } }) => {
      if (recordingId !== this.status?.id) return;
      const others = this.live.filter((t) => t.index !== turn.index);
      this.live = (turn.text ? [...others, turn] : others).sort((a, b) => a.start - b.start);
    });
    await this.refresh();
    this.status = await invoke<Status | null>("recording_status");
    if (this.status) this.#startPolling();
  }

  async refresh() {
    this.recordings = await invoke<Recording[]>("list_recordings");
    this.loaded = true;
  }

  async start(title = defaultTitle()) {
    this.error = "";
    try {
      await invoke("start_recording", { title });
      this.heardSound = false;
      this.live = [];
      this.#startPolling();
    } catch (e) {
      this.error = String(e);
    }
  }

  async setPaused(paused: boolean) {
    await invoke("set_paused", { paused });
    if (this.status) this.status.paused = paused;
  }

  async stop() {
    clearInterval(this.#poll);
    this.saving = true;
    const id = await invoke<string>("stop_recording").catch((e) => {
      this.error = String(e);
      return null;
    });
    this.status = null;
    this.saving = false;
    this.live = [];
    await this.refresh();
    return id;
  }

  #startPolling() {
    clearInterval(this.#poll);
    this.#poll = setInterval(async () => {
      this.status = await invoke<Status | null>("recording_status");
      if (this.status && this.status.micLevel > 0.01) this.heardSound = true;
    }, 200);
  }
}

export const recorder = new Recorder();

const defaultTitle = () =>
  `Recording, ${new Date().toLocaleString(undefined, { day: "numeric", month: "short", hour: "numeric", minute: "2-digit" })}`;

export function formatDuration(totalSecs: number) {
  const secs = Math.floor(totalSecs);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = String(secs % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
