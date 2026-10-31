<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ArrowLeft, Check, Copy, Download, NotebookPen, PencilLine, Sparkles, Trash2 } from "@lucide/svelte";
  import { formatDuration, recorder, type Recording } from "$lib/recorder.svelte";
  import { toMarkdown } from "$lib/markdown";
  import type { Summary } from "$lib/summary";
  import type { Transcript } from "$lib/transcript";
  import { NeoTab, NeoTabs } from "@dvcol/neo-svelte/nav";
  import { theme } from "$lib/theme.svelte";
  import Button from "./Button.svelte";
  import NoteEditor from "./NoteEditor.svelte";
  import SummarySkeleton from "./SummarySkeleton.svelte";
  import SummaryView from "./SummaryView.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";

  let { recording, at, onBack }: { recording: Recording; at?: number; onBack: () => void } = $props();

  let transcript = $state<Transcript | null>(null);
  let error = $state("");
  let copied = $state(false);
  let shared = $state<"copy" | "save" | null>(null);
  let renaming = $state(false);
  let title = $state("");
  let confirmDelete = $state(false);
  let busy = $state(false);
  let actionError = $state("");
  let summary = $state<Summary | null>(null);
  let tab = $state<"summary" | "notes">("notes");

  const segments = $derived(transcript?.segments ?? []);
  const empty = $derived(error || (transcript ? "This recording doesn't have a transcript." : "Loading transcript…"));
  const identifying = $derived(recorder.identifyingSpeakers.includes(recording.id));
  const writingSummary = $derived(recorder.writingSummary.includes(recording.id));
  const summaryError = $derived(recorder.summaryErrors[recording.id]);

  async function load() {
    try {
      transcript = await invoke<Transcript>("transcript", { id: recording.id });
    } catch (e) {
      error = String(e);
    }
  }

  async function loadSummary() {
    summary = await invoke<Summary | null>("summary", { id: recording.id }).catch(() => null);
    if (summary || writingSummary) tab = "summary";
  }

  function writeSummary() {
    recorder.summaryErrors = { ...recorder.summaryErrors, [recording.id]: null };
    recorder.writingSummary = [...recorder.writingSummary, recording.id];
    tab = "summary";
    invoke("summarize", { id: recording.id });
  }

  onMount(async () => {
    await Promise.all([load(), loadSummary()]);
    await tick();
    document.querySelector(`[data-start="${at}"]`)?.scrollIntoView({ block: "center" });
  });

  $effect(() => {
    if (recorder.speakersIdentified[recording.id]) load();
  });

  $effect(() => {
    if (recorder.summaryWritten[recording.id]) loadSummary();
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

  async function share(save: boolean) {
    const notes = await invoke<string | null>("note_markdown", { recordingId: recording.id }).catch(() => null);
    const markdown = toMarkdown(recording, summary, notes);
    if (save) await run(() => invoke("save_markdown", { title: recording.title, markdown }));
    else await navigator.clipboard.writeText(markdown);
    if (actionError) return;
    shared = save ? "save" : "copy";
    setTimeout(() => (shared = null), 1500);
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
    <Button onclick={copy}>
      {#if copied}<Check class="size-3.5" aria-hidden="true" />{:else}<Copy class="size-3.5" aria-hidden="true" />{/if}
      {copied ? "Copied" : "Copy"}
    </Button>
  {/if}
{/snippet}

<div class="flex min-h-0 flex-1 flex-col">
  <div class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto w-full max-w-3xl px-8 py-8">
      <div class="mb-6 flex items-start gap-4">
        <Button onclick={onBack} aria-label="Back to recordings"><ArrowLeft class="size-4" aria-hidden="true" />Back</Button>
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
              <Button primary disabled={busy} onclick={rename}>Save</Button>
            </form>
          {:else}
            <h2 class="truncate text-xl font-semibold">{recording.title}</h2>
          {/if}
          <p class="text-sm text-zinc-500 tabular-nums">
            {new Date(recording.startedAt).toLocaleString()} · {formatDuration(recording.durationSecs)}
          </p>
        </div>
        {#if !renaming && !confirmDelete}
          <Button onclick={() => share(false)} disabled={busy}>
            {#if shared === "copy"}<Check class="size-4" aria-hidden="true" />{:else}<Copy class="size-4" aria-hidden="true" />{/if}
            {shared === "copy" ? "Copied" : "Copy .md"}
          </Button>
          <Button onclick={() => share(true)} disabled={busy}>
            <Download class="size-4" aria-hidden="true" />{shared === "save" ? "Saved to Downloads" : "Save .md"}
          </Button>
          <Button onclick={() => ((title = recording.title), (renaming = true))} aria-label="Rename" title="Rename">
            <PencilLine class="size-4" aria-hidden="true" />
          </Button>
          <Button danger onclick={() => (confirmDelete = true)} aria-label="Delete…" title="Delete">
            <Trash2 class="size-4" aria-hidden="true" />
          </Button>
        {/if}
      </div>
      {#if confirmDelete}
        <div class="mb-6 flex flex-wrap items-center gap-2">
          <span class="flex-1 text-sm text-zinc-600 dark:text-zinc-400">
            Delete this recording's notes and transcript from this Mac?
          </span>
          <Button danger onclick={remove} disabled={busy}>{busy ? "Deleting…" : "Delete"}</Button>
          <Button onclick={() => (confirmDelete = false)}>Cancel</Button>
        </div>
      {/if}
      {#if actionError}
        <p class="mb-6 text-sm text-red-700 dark:text-red-400" role="alert">{actionError}</p>
      {/if}
      <div class="mb-5 flex items-center gap-3">
        <NeoTabs bind:active={tab} pill glass={theme.glass} rounded={theme.current !== "terminal"} aria-label="View">
          {#each [["summary", "Summary", Sparkles], ["notes", "My notes", NotebookPen]] as const as [id, label, Icon] (id)}
            <NeoTab tabId={id}>
              <span class="flex items-center gap-1.5"><Icon class="size-3.5" aria-hidden="true" />{label}</span>
            </NeoTab>
          {/each}
        </NeoTabs>
        {#if tab === "summary" && !writingSummary && transcript?.segments.length}
          <Button class="ml-auto" onclick={writeSummary}>
            {summary ? "Rewrite summary" : "Write summary"}
          </Button>
        {/if}
      </div>
      {#if tab === "notes"}
        <NoteEditor recordingId={recording.id} />
      {:else if writingSummary}
        <SummarySkeleton />
      {:else if summaryError}
        <p class="py-6 text-sm text-red-700 dark:text-red-400" role="alert">{summaryError}</p>
      {:else if summary}
        <SummaryView {summary} />
      {:else}
        <p class="py-6 text-sm text-zinc-500">
          No summary yet. Aube writes one from the transcript and your notes using the model in Settings.
        </p>
      {/if}
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
