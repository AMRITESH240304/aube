<script lang="ts">
  import { formatDuration, recorder } from "$lib/recorder.svelte";
  import Conversation from "./Conversation.svelte";

  let end = $state<HTMLElement>();

  const status = $derived(recorder.status!);

  // Follows new text, unless you've scrolled up to read.
  $effect(() => {
    if (!recorder.live.length || !end) return;
    const main = end.closest("main");
    if (main && main.scrollHeight - main.scrollTop - main.clientHeight < 240) end.scrollIntoView({ block: "end" });
  });
</script>

{#snippet notice(text: string)}
  <p
    class="mb-4 rounded-lg border border-amber-200 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-200"
    role="alert"
  >
    {text}
  </p>
{/snippet}

<section aria-labelledby="live-heading" aria-live="polite">
  <div class="mb-6">
    <h2 id="live-heading" class="truncate text-xl font-semibold">{status.title}</h2>
    <p class="text-sm text-zinc-500 tabular-nums">
      {status.paused ? "Paused" : "Live transcript"} · {formatDuration(status.elapsedSecs)}
    </p>
  </div>

  {#if status.systemError}
    {@render notice(`Recording your microphone only. ${status.systemError}`)}
  {/if}
  {#if recorder.silent}
    {@render notice(
      "We can't hear your microphone. Check it isn't muted, and that Ease Rec is allowed in System Settings → Privacy & Security → Microphone.",
    )}
  {/if}

  {#if recorder.live.length}
    <Conversation lines={recorder.live} />
  {:else}
    <p class="py-16 text-center text-sm text-zinc-500">
      {status.transcriptionError ?? "Listening… what you and the call say will appear here."}
    </p>
  {/if}
  <div bind:this={end}></div>
</section>
