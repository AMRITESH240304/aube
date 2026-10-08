<script lang="ts">
  import { tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { revealItemInDir } from "@tauri-apps/plugin-opener";
  import { formatDuration, recorder, type Recording } from "$lib/recorder.svelte";
  import type { Transcript } from "$lib/transcript";
  import Conversation from "./Conversation.svelte";

  let { recording, at, onBack }: { recording: Recording; at?: number; onBack: () => void } = $props();

  let transcript = $state<Transcript | null>(null);
  let error = $state("");
  let copied = $state(false);
  let renaming = $state(false);
  let title = $state("");
  let confirmDelete = $state(false);
  let busy = $state(false);
  let actionError = $state("");

  $effect(() => {
    if (recording.transcribed && !transcript) load();
  });

  async function load() {
    error = "";
    try {
      transcript = await invoke<Transcript>("transcript", { id: recording.id });
      await tick();
      document.querySelector(`[data-start="${at}"]`)?.scrollIntoView({ block: "center" });
    } catch (e) {
      error = String(e);
    }
  }

  async function run(action: () => Promise<unknown>) {
    busy = true;
    actionError = "";
    try {
      await action();
    } catch (e) {
      actionError = String(e);
    }
    busy = false;
  }

  const rename = () =>
    run(async () => {
      await invoke("rename_recording", { id: recording.id, title });
      await recorder.refresh();
      renaming = false;
    });

  const remove = () =>
    run(async () => {
      await invoke("delete_recording", { id: recording.id });
      await recorder.refresh();
    });

  async function copy() {
    await navigator.clipboard.writeText(transcript?.text ?? "");
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (renaming) renaming = false;
    else if (confirmDelete) confirmDelete = false;
    else onBack();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="mx-auto w-full max-w-3xl px-8 py-8">
  <div class="mb-6 flex items-start gap-4">
    <button class="btn" onclick={onBack} aria-label="Back to recordings">← Back</button>
    <div class="min-w-0 flex-1">
      {#if renaming}
        <form class="flex gap-2" onsubmit={(e) => (e.preventDefault(), rename())}>
          <label for="title" class="sr-only">Recording name</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            id="title"
            bind:value={title}
            autofocus
            class="min-w-0 flex-1 rounded-md border border-zinc-300 bg-transparent px-2 py-1 font-semibold focus-visible:outline-2 focus-visible:outline-blue-600 dark:border-zinc-700"
          />
          <button class="btn-primary" disabled={busy}>Save</button>
        </form>
      {:else}
        <h2 class="truncate text-lg font-semibold">{recording.title}</h2>
      {/if}
      <p class="text-sm text-zinc-500 tabular-nums">
        {new Date(recording.startedAt).toLocaleString()} · {formatDuration(recording.durationSecs)}
      </p>
    </div>
  </div>

  <div class="mb-6 flex flex-wrap items-center gap-2">
    {#if !renaming}
      <button class="btn" onclick={() => ((title = recording.title), (renaming = true))}>Rename</button>
    {/if}
    <button class="btn" onclick={() => revealItemInDir(recording.path)}>Show in Finder</button>
    {#if transcript?.text}
      <button class="btn" onclick={copy}>{copied ? "Copied" : "Copy text"}</button>
    {/if}
    <span class="flex-1"></span>
    {#if confirmDelete}
      <span class="text-sm text-zinc-600 dark:text-zinc-400">Delete the audio and transcript from this Mac?</span>
      <button class="btn border-red-600 text-red-700 dark:text-red-400" onclick={remove} disabled={busy}>
        {busy ? "Deleting…" : "Delete"}
      </button>
      <button class="btn" onclick={() => (confirmDelete = false)}>Cancel</button>
    {:else}
      <button class="btn text-red-700 dark:text-red-400" onclick={() => (confirmDelete = true)}>Delete…</button>
    {/if}
  </div>
  {#if actionError}
    <p class="-mt-3 mb-6 text-sm text-red-700 dark:text-red-400" role="alert">{actionError}</p>
  {/if}

  {#if !recording.transcribed}
    <p class="rounded-lg border border-dashed border-zinc-300 px-6 py-10 text-center text-sm text-zinc-500 dark:border-zinc-700 dark:text-zinc-400">
      This recording doesn't have a transcript.
    </p>
  {:else if error}
    <div class="rounded-lg border border-zinc-200 px-6 py-10 text-center dark:border-zinc-800" role="alert">
      <h3 class="font-semibold">Couldn't open the transcript</h3>
      <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">{error}</p>
      <button class="btn mt-4" onclick={load}>Try again</button>
    </div>
  {:else if !transcript}
    <div class="space-y-3" aria-busy="true" aria-label="Loading transcript">
      {#each { length: 5 } as _}
        <div class="h-5 animate-pulse rounded bg-zinc-100 dark:bg-zinc-900"></div>
      {/each}
    </div>
  {:else if transcript.speech_detected === false || transcript.segments.length === 0}
    <div class="rounded-lg border border-dashed border-zinc-300 px-6 py-10 text-center dark:border-zinc-700">
      <h3 class="font-semibold">No speech in this recording</h3>
      <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        {transcript.suggestion ?? "Nothing was said that could be transcribed."}
      </p>
    </div>
  {:else}
    <Conversation lines={transcript.segments} highlight={at} />
  {/if}
</div>
