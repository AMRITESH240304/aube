<script lang="ts">
  import { Pause, Play, Plus, Square } from "@lucide/svelte";
  import { formatDuration, recorder, type Recording } from "$lib/recorder.svelte";
  import Button from "./Button.svelte";
  import LanguagePicker from "./LanguagePicker.svelte";

  let {
    continuing,
    onStarted,
    onNew,
    onSaved,
  }: {
    continuing?: Recording;
    onStarted: () => void;
    onNew: () => void;
    onSaved: (id: string) => void;
  } = $props();

  const status = $derived(recorder.status);
  const SHAPE = [0.4, 0.75, 1, 0.6, 0.9, 0.55, 0.3];

  async function stop() {
    const id = await recorder.stop();
    if (id) onSaved(id);
  }

  async function start(from?: Recording) {
    if (await recorder.start(from)) onStarted();
  }
</script>

{#snippet wave(label: string, level: number, off: boolean, color: string)}
  <span class="flex h-5 items-center gap-0.5" title={label}>
    <span class="sr-only">{label}</span>
    {#each SHAPE as k}
      <span
        class={["w-[3px] rounded-full transition-[height] duration-150", off && "bg-zinc-600"]}
        style:background-color={off ? undefined : color}
        style:height={`${Math.max(3, 20 * k * Math.min(1, Math.sqrt(off ? 0 : level)))}px`}
        aria-hidden="true"
      ></span>
    {/each}
  </span>
{/snippet}

{#if status}
  <div class="flex shrink-0 items-center gap-3 border-t border-white/[0.06] px-5 py-3" role="status">
    <span class="mr-auto flex items-center gap-3.5">
      {@render wave("Your mic", status.micLevel, status.paused, "var(--wave-you)")}
      {@render wave("Call audio", status.systemLevel, status.paused || !!status.systemError, "var(--wave-them)")}
    </span>
    <span class="text-sm text-zinc-400 tabular-nums">{formatDuration(status.elapsedSecs)}</span>
    <button
      class="grid size-8 place-items-center rounded-full bg-zinc-800 hover:bg-zinc-700 focus-visible:outline-2 focus-visible:outline-blue-600 disabled:opacity-50"
      aria-label={status.paused ? "Resume" : "Pause"}
      title={status.paused ? "Resume" : "Pause"}
      disabled={recorder.saving}
      onclick={() => recorder.setPaused(!status.paused)}
    >
      {#if status.paused}
        <Play class="size-3.5" fill="currentColor" aria-hidden="true" />
      {:else}
        <Pause class="size-3.5" fill="currentColor" aria-hidden="true" />
      {/if}
    </button>
    <button
      class="stop flex items-center gap-2 rounded-[var(--radius-btn)] px-3.5 py-1.5 text-sm font-semibold focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:opacity-60"
      disabled={recorder.saving}
      onclick={stop}
    >
      <Square class="size-3" fill="currentColor" aria-hidden="true" />
      {recorder.saving ? "Saving…" : "Stop"}
    </button>
  </div>
{:else}
  <div class="pointer-events-none absolute inset-x-0 bottom-5 flex flex-col items-center gap-2 px-4">
    {#if recorder.error}
      <p
        class="pointer-events-auto flex items-center gap-3 rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-800 shadow-sm dark:border-red-900 dark:bg-red-950 dark:text-red-200"
        role="alert"
      >
        Couldn't record: {recorder.error}
        <Button onclick={() => (recorder.error = "")}>Dismiss</Button>
      </p>
    {/if}

    <div
      class="pointer-events-auto flex items-center gap-2 rounded-[var(--radius-btn)] border border-zinc-200 bg-white/90 p-1.5 shadow-lg dark:border-zinc-800 dark:bg-zinc-900"
    >
      {#if recorder.languages.length}
        <LanguagePicker />
      {/if}
      <Button primary disabled={recorder.installing} onclick={() => start(continuing)}>
        <span class="ember size-2.5 rounded-full" aria-hidden="true"></span>
        {recorder.installing ? "Downloading…" : continuing ? "Continue" : "Record"}
      </Button>
      {#if continuing}
        <Button disabled={recorder.installing} onclick={onNew}>
          <Plus class="size-4" aria-hidden="true" />New
        </Button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .stop {
    background: var(--stop-bg);
    color: var(--stop-fg);
  }

  .stop:hover:not(:disabled) {
    filter: brightness(1.08);
  }
</style>
