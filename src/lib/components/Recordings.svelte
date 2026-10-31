<script lang="ts">
  import { ArrowRight, AudioLines, ChevronRight, Ghost } from "@lucide/svelte";
  import { formatDuration, recorder } from "$lib/recorder.svelte";
  import { theme } from "$lib/theme.svelte";
  import Button from "./Button.svelte";

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
      <Button onclick={onViewAll}>View all {all.length}<ArrowRight class="size-3.5" aria-hidden="true" /></Button>
    {/if}
  </div>

  {#if !recorder.loaded}
    <div class="h-14 animate-pulse rounded-lg bg-zinc-100 dark:bg-zinc-900" aria-busy="true"></div>
  {:else if shown.length === 0}
    <div class="surface rounded-lg border border-dashed border-zinc-300 px-6 py-10 text-center dark:border-zinc-700">
      <span class="mx-auto grid size-12 place-items-center rounded-xl bg-zinc-800 text-blue-600" aria-hidden="true">
        {#if theme.current === "haunted"}<Ghost class="size-6" />{:else}<AudioLines class="size-6" />{/if}
      </span>
      <h3 class="mt-3 font-semibold">
        {theme.current === "haunted" ? "Nothing haunts these halls yet" : "No recordings yet"}
      </h3>
      <p class="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
        Press Start recording when your meeting starts, and every word will be kept.
      </p>
    </div>
  {:else}
    <ul class="surface divide-y divide-zinc-200 overflow-hidden rounded-lg border border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
      {#each shown as recording (recording.id)}
        <li>
          <button
            class="group flex w-full min-w-0 items-center gap-3 px-4 py-3 text-left hover:bg-zinc-50 dark:hover:bg-zinc-900/70"
            onclick={() => onOpen(recording.id)}
          >
            <span class="grid size-9 shrink-0 place-items-center rounded-lg bg-zinc-800 text-zinc-300" aria-hidden="true">
              <AudioLines class="size-4" />
            </span>
            <span class="min-w-0 flex-1">
              <span class="block truncate font-medium">{recording.title}</span>
              <span class="block text-xs text-zinc-500 tabular-nums">
                {startedAt(recording.startedAt)} · {formatDuration(recording.durationSecs)}
              </span>
            </span>
            <ChevronRight
              class="size-4 shrink-0 text-zinc-600 transition group-hover:translate-x-0.5 group-hover:text-zinc-300"
              aria-hidden="true"
            />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>
