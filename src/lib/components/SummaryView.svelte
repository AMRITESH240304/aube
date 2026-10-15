<script lang="ts">
  import type { Origin, Summary } from "$lib/summary";

  let { summary }: { summary: Summary } = $props();

  const tone = (origin: Origin) => (origin === "ai" ? "text-zinc-500 dark:text-zinc-400" : "text-zinc-900 dark:text-zinc-100");
</script>

<article class="leading-relaxed" aria-label="Summary">
  {#if summary.skipped?.length}
    <div
      class="mb-5 rounded-lg border border-amber-200 bg-amber-50 px-4 py-2.5 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-200"
      role="note"
    >
      {#each summary.skipped as line, i (i)}
        <p>{line}</p>
      {/each}
      <p class="mt-1 text-amber-800/80 dark:text-amber-200/70">
        Apple Intelligence declined these parts. The transcript still has everything that was said.
      </p>
    </div>
  {/if}
  {#if summary.overview}
    <p class={["mb-6", tone("ai")]}>{summary.overview}</p>
  {/if}
  {#each summary.sections as section, i (i)}
    <section class="mb-5">
      <h3 class={["mb-1.5 text-lg font-semibold", tone(section.heading.origin)]}>{section.heading.text}</h3>
      <ul class="list-disc space-y-1 pl-6 marker:text-zinc-400">
        {#each section.lines as line, j (j)}
          <li class={["select-text", tone(line.origin)]}>{line.text}</li>
        {/each}
      </ul>
    </section>
  {/each}
</article>
