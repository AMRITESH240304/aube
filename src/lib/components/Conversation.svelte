<script lang="ts">
  import { formatDuration } from "$lib/recorder.svelte";

  type Line = { start: number; speaker: string | null; text: string };

  let { lines, highlight }: { lines: Line[]; highlight?: number } = $props();

  // Older transcripts have no speaker, so every line gets its own timestamp.
  const startsTurn = (i: number) => lines[i].speaker === null || lines[i - 1]?.speaker !== lines[i].speaker;
</script>

<ol class="space-y-1.5">
  {#each lines as line, i (i)}
    {@const mine = line.speaker === "You"}
    <li class={["flex flex-col", mine ? "items-end" : "items-start", startsTurn(i) && i > 0 && "pt-3"]}>
      {#if startsTurn(i)}
        <span class="mb-1 px-1 text-xs text-zinc-500 tabular-nums">
          {[line.speaker, formatDuration(line.start)].filter(Boolean).join(" · ")}
        </span>
      {/if}
      <p
        data-start={line.start}
        class={[
          "max-w-[75%] rounded-2xl px-3.5 py-2 leading-relaxed select-text",
          mine ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900" : "bg-zinc-100 dark:bg-zinc-800",
          line.start === highlight && "ring-2 ring-amber-400",
        ]}
      >
        {line.text}
      </p>
    </li>
  {/each}
</ol>
