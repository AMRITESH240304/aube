<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { formatDuration, recorder } from "$lib/recorder.svelte";

  type Match = { id: string; start: number; text: string };

  let { query, onOpen }: { query: string; onOpen: (id: string, at: number) => void } = $props();

  let matches = $state<Match[] | null>(null);

  const titles = $derived(new Map(recorder.recordings.map((r) => [r.id, r.title])));
  const term = $derived(query.trim());

  $effect(() => {
    const q = term;
    const timer = setTimeout(async () => {
      matches = await invoke<Match[]>("search_transcripts", { query: q });
    }, 150);
    return () => clearTimeout(timer);
  });

  function highlight(text: string) {
    const i = text.toLowerCase().indexOf(term.toLowerCase());
    return i < 0 ? [text, "", ""] : [text.slice(0, i), text.slice(i, i + term.length), text.slice(i + term.length)];
  }
</script>

<section aria-labelledby="results-heading">
  <h2 id="results-heading" class="mb-6 text-xl font-semibold">
    Results for “{term}”
    {#if matches?.length}<span class="ml-1 text-base font-normal text-zinc-500">{matches.length}</span>{/if}
  </h2>
  {#if matches === null}
    <div class="h-14 animate-pulse rounded-lg bg-zinc-100 dark:bg-zinc-900" aria-busy="true"></div>
  {:else if matches.length === 0}
    <p class="rounded-lg border border-dashed border-zinc-300 px-6 py-8 text-center text-sm text-zinc-500 dark:border-zinc-700">
      Nobody said “{term}” in your transcripts.
    </p>
  {:else}
    <ul class="divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
      {#each matches as match, i (i)}
        {@const [before, hit, after] = highlight(match.text)}
        <li>
          <button
            class="w-full px-4 py-3 text-left hover:bg-zinc-50 dark:hover:bg-zinc-900"
            onclick={() => onOpen(match.id, match.start)}
          >
            <p class="text-xs text-zinc-500 tabular-nums">{titles.get(match.id)} · {formatDuration(match.start)}</p>
            <p class="mt-0.5 text-sm">
              {before}<mark class="rounded bg-amber-200 px-0.5 dark:bg-amber-700/60 dark:text-white">{hit}</mark>{after}
            </p>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
