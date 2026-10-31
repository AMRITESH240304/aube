<script lang="ts">
  import { Pause, Play, Plus, Square } from "@lucide/svelte";
  import { formatDuration, recorder, type Recording } from "$lib/recorder.svelte";
  import Button from "./Button.svelte";
  import LanguagePicker from "./LanguagePicker.svelte";

  let {
    continuing,
    onStarted,
    onSaved,
  }: { continuing?: Recording; onStarted: () => void; onSaved: (id: string) => void } = $props();

  const status = $derived(recorder.status);
  const SHAPE = [0.45, 0.8, 1, 0.65, 0.35];

  async function stop() {
    const id = await recorder.stop();
    if (id) onSaved(id);
  }

  async function start(from?: Recording) {
    if (await recorder.start(from)) onStarted();
  }
</script>

{#snippet wave(label: string, level: number, off: boolean)}
  <span class="flex h-5 items-center gap-0.5" title={label}>
    <span class="sr-only">{label}</span>
    {#each SHAPE as k}
      <span
        class={["w-[3px] rounded-full transition-[height] duration-150", off ? "bg-zinc-300 dark:bg-zinc-600" : "bg-emerald-500"]}
        style:height={`${Math.max(3, 20 * k * Math.min(1, Math.sqrt(off ? 0 : level)))}px`}
        aria-hidden="true"
      ></span>
    {/each}
  </span>
{/snippet}

<div class="pointer-events-none absolute inset-x-0 bottom-5 flex flex-col items-center gap-2 px-4">
  {#if recorder.error && !status}
    <p
      class="pointer-events-auto flex items-center gap-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800 shadow-sm dark:border-red-900 dark:bg-red-950 dark:text-red-200"
      role="alert"
    >
      Couldn't record: {recorder.error}
      <Button onclick={() => (recorder.error = "")}>Dismiss</Button>
    </p>
  {/if}

  <div
    class={[
      "surface pointer-events-auto flex items-center gap-2 rounded-[var(--radius-btn)] border border-zinc-200 bg-white/90 p-1.5 shadow-lg backdrop-blur-xl dark:border-zinc-800 dark:bg-zinc-900/80",
      status && !status.paused && "recording-glow",
    ]}
    role={status ? "status" : undefined}
  >
    {#if status}
      <span class="flex items-center gap-2.5 pl-3">
        {@render wave("Your mic", status.micLevel, status.paused)}
        {@render wave("Call audio", status.systemLevel, status.paused || !!status.systemError)}
      </span>
      <span class="w-12 text-sm text-zinc-600 tabular-nums dark:text-zinc-300">
        {formatDuration(status.elapsedSecs)}
      </span>
      <button
        class="grid size-8 place-items-center rounded-full hover:bg-zinc-100 focus-visible:outline-2 focus-visible:outline-blue-600 disabled:opacity-50 dark:hover:bg-zinc-800"
        aria-label={status.paused ? "Resume" : "Pause"}
        title={status.paused ? "Resume" : "Pause"}
        disabled={recorder.saving}
        onclick={() => recorder.setPaused(!status.paused)}
      >
        {#if status.paused}
          <Play class="size-4" fill="currentColor" aria-hidden="true" />
        {:else}
          <Pause class="size-4" fill="currentColor" aria-hidden="true" />
        {/if}
      </button>
      <button
        class="flex items-center gap-2 rounded-full bg-red-600 px-3.5 py-1.5 text-sm font-medium text-white hover:bg-red-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:opacity-60"
        disabled={recorder.saving}
        onclick={stop}
      >
        <Square class="size-3" fill="currentColor" aria-hidden="true" />
        {recorder.saving ? "Saving…" : "Stop"}
      </button>
    {:else}
      {#if recorder.languages.length}
        <LanguagePicker />
      {/if}
      <Button primary disabled={recorder.installing} onclick={() => start(continuing)}>
        <span class="ember size-2.5 rounded-full" aria-hidden="true"></span>
        {recorder.installing ? "Downloading…" : continuing ? "Continue recording" : "Start recording"}
      </Button>
      {#if continuing}
        <Button disabled={recorder.installing} onclick={() => start()}>
          <Plus class="size-4" aria-hidden="true" />New note
        </Button>
      {/if}
    {/if}
  </div>
</div>
