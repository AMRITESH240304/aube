<script lang="ts">
  import { formatDuration, recorder } from "$lib/recorder.svelte";

  let { limit, onOpen, onViewAll }: { limit?: number; onOpen: (id: string) => void; onViewAll?: () => void } =
    $props();

  const title = $derived(limit ? "Recent recordings" : "All recordings");
  const all = $derived(recorder.recordings);
  const shown = $derived(limit ? all.slice(0, limit) : all);

  const startedAt = (ms: number) =>
    new Date(ms).toLocaleString(undefined, {
      weekday: "short",
      day: "numeric",
      month: "short",
      hour: "numeric",
      minute: "2-digit",
    });
</script>

<section aria-labelledby="recordings-heading">
  <div class="mb-6 flex items-center justify-between">
    <h2 id="recordings-heading" class="text-xl font-semibold">
      {title}
      {#if !limit && all.length}<span class="ml-1 text-base font-normal text-zinc-500">{all.length}</span>{/if}
    </h2>
    {#if onViewAll && all.length > shown.length}
      <button class="btn" onclick={onViewAll}>View all {all.length}</button>
    {/if}
  </div>

  {#if !recorder.loaded}
    <div class="h-14 animate-pulse rounded-lg bg-zinc-100 dark:bg-zinc-900" aria-busy="true"></div>
  {:else if shown.length === 0}
    <div class="rounded-lg border border-dashed border-zinc-300 px-6 py-10 text-center dark:border-zinc-700">
      <p class="text-3xl" aria-hidden="true">👻</p>
      <h3 class="mt-2 font-semibold">Nothing haunts these halls yet</h3>
      <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Press Start recording when your meeting starts, and every word will be kept.
      </p>
    </div>
  {:else}
    <ul class="divide-y divide-zinc-200 rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
      {#each shown as recording (recording.id)}
        <li class="flex items-center gap-3 pr-4">
          <button
            class="min-w-0 flex-1 px-4 py-3 text-left hover:bg-zinc-50 dark:hover:bg-zinc-900"
            onclick={() => onOpen(recording.id)}
          >
            <p class="truncate font-medium">{recording.title}</p>
            <p class="text-xs text-zinc-500 tabular-nums">
              {startedAt(recording.startedAt)} · {formatDuration(recording.durationSecs)}
            </p>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
