<script lang="ts">
  import { AUTO_LANGUAGE, formatDuration, recorder } from "$lib/recorder.svelte";
  import NoteEditor from "./NoteEditor.svelte";
  import TranscriptPanel from "./TranscriptPanel.svelte";

  const status = $derived(recorder.status!);
  const detection = $derived(
    recorder.language === AUTO_LANGUAGE
      ? Object.entries(recorder.detected)
          .map(([speaker, language]) => `${speaker}: ${language ?? "Mac's language"}`)
          .join(" · ") || "Detecting language…"
      : "",
  );
</script>

{#snippet notice(text: string)}
  <p
    class="mb-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-200"
    role="alert"
  >
    {text}
  </p>
{/snippet}

<div class="flex min-h-0 flex-1 flex-col">
  <div class="min-h-0 flex-1 overflow-y-auto">
    <div class="mx-auto w-full max-w-3xl px-8 py-8">
      <h2 class="truncate text-xl font-semibold">{status.title}</h2>
      <p class="mb-6 text-sm text-zinc-500 tabular-nums">
        {status.paused ? "Paused" : "Recording"} · {formatDuration(status.elapsedSecs)}
      </p>
      {#if status.systemError}
        {@render notice(`Recording your microphone only. ${status.systemError}`)}
      {/if}
      {#if recorder.silent}
        {@render notice(
          "We can't hear your microphone. Check it isn't muted, and that Aube is allowed in System Settings → Privacy & Security → Microphone.",
        )}
      {/if}
      <NoteEditor recordingId={status.id} />
    </div>
  </div>
  <TranscriptPanel
    lines={recorder.live}
    label={detection}
    empty={status.transcriptionError ?? "Listening… what you and the call say will appear here."}
    follow
  />
  <div class="h-20 shrink-0"></div>
</div>
