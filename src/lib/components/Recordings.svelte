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
      <h3 class="font-semibold">No recordings yet</h3>
      <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Press Start recording when your meeting starts.
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
          {#if recording.recovered}
            <span
              class="rounded-full bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800 dark:bg-amber-950 dark:text-amber-300"
              title="The app closed during this recording. Everything up to that moment was saved."
            >
              Recovered
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>
