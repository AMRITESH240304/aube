<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { formatDuration, recorder, type Recording } from "$lib/recorder.svelte";
  import type { Transcript } from "$lib/transcript";
  import NoteEditor from "./NoteEditor.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";

  let { recording, at, onBack }: { recording: Recording; at?: number; onBack: () => void } = $props();

  let transcript = $state<Transcript | null>(null);
  let error = $state("");
  let copied = $state(false);
  let renaming = $state(false);
  let title = $state("");
  let confirmDelete = $state(false);
  let busy = $state(false);
  let actionError = $state("");

  const segments = $derived(transcript?.segments ?? []);
  const empty = $derived(error || (transcript ? "This recording doesn't have a transcript." : "Loading transcript…"));
  const identifying = $derived(recorder.identifyingSpeakers.includes(recording.id));

  async function load() {
    try {
      transcript = await invoke<Transcript>("transcript", { id: recording.id });
    } catch (e) {
      error = String(e);
    }
  }

  onMount(async () => {
    await load();
    await tick();
    document.querySelector(`[data-start="${at}"]`)?.scrollIntoView({ block: "center" });
  });

  $effect(() => {
    if (recorder.speakersIdentified[recording.id]) load();
  });

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
    const text = segments.map((s) => (s.speaker ? `${s.speaker}: ${s.text}` : s.text)).join("\n\n");
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "Escape" || (e.target as HTMLElement).isContentEditable) return;
    if (renaming) renaming = false;
    else if (confirmDelete) confirmDelete = false;
    else onBack();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet copyButton()}
  {#if segments.length}
    <button class="btn py-1 text-xs" onclick={copy}>{copied ? "Copied" : "Copy"}</button>
  {/if}
{/snippet}

<div class="flex min-h-0 flex-1 flex-col">
  <div class="min-h-0 flex-1 overflow-y-auto">
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
            <h2 class="truncate text-xl font-semibold">{recording.title}</h2>
          {/if}
          <p class="text-sm text-zinc-500 tabular-nums">
            {new Date(recording.startedAt).toLocaleString()} · {formatDuration(recording.durationSecs)}
          </p>
        </div>
        {#if !renaming && !confirmDelete}
          <button class="btn" onclick={() => ((title = recording.title), (renaming = true))}>Rename</button>
          <button class="btn text-red-700 dark:text-red-400" onclick={() => (confirmDelete = true)}>Delete…</button>
        {/if}
      </div>
      {#if confirmDelete}
        <div class="mb-6 flex flex-wrap items-center gap-2">
          <span class="flex-1 text-sm text-zinc-600 dark:text-zinc-400">
            Delete this recording's notes and transcript from this Mac?
          </span>
          <button class="btn border-red-600 text-red-700 dark:text-red-400" onclick={remove} disabled={busy}>
            {busy ? "Deleting…" : "Delete"}
          </button>
          <button class="btn" onclick={() => (confirmDelete = false)}>Cancel</button>
        </div>
      {/if}
      {#if actionError}
        <p class="mb-6 text-sm text-red-700 dark:text-red-400" role="alert">{actionError}</p>
      {/if}
      <NoteEditor recordingId={recording.id} />
    </div>
  </div>
  <TranscriptPanel
    lines={segments}
    label={identifying ? "Working out who said what…" : ""}
    highlight={at}
    {empty}
    actions={copyButton}
  />
  <div class="h-20 shrink-0"></div>
</div>
