<script module lang="ts">
  export type View = { kind: "home" } | { kind: "all" };
</script>

<script lang="ts">
  import { recorder } from "$lib/recorder.svelte";

  let {
    view,
    query = $bindable(),
    onNavigate,
  }: { view: View; query: string; onNavigate: (view: View) => void } = $props();

  let search: HTMLInputElement;

  const isActive = (next: View) => !query.trim() && next.kind === view.kind;

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "f" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      search.focus();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet navItem(label: string, target: View, count?: number)}
  <button
    class={[
      "flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-sm hover:bg-zinc-200/70 dark:hover:bg-zinc-800",
      isActive(target) && "bg-zinc-200 font-medium dark:bg-zinc-800",
    ]}
    aria-current={isActive(target) ? "page" : undefined}
    onclick={() => onNavigate(target)}
  >
    <span class="truncate">{label}</span>
    {#if count}
      <span class="text-xs text-zinc-500 tabular-nums">{count}</span>
    {/if}
  </button>
{/snippet}

<nav
  class="flex w-60 shrink-0 flex-col gap-4 border-r border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-900/60"
  aria-label="Sidebar"
>
  <p class="px-2 pt-1 text-sm font-semibold">Ease Rec</p>

  <div class="relative">
    <label for="search" class="sr-only">Search transcripts</label>
    <input
      id="search"
      type="search"
      bind:this={search}
      bind:value={query}
      placeholder="Search transcripts"
      class="w-full rounded-md border border-zinc-300 bg-white px-3 py-1.5 pr-9 text-sm focus-visible:outline-2 focus-visible:outline-blue-600 dark:border-zinc-700 dark:bg-zinc-950"
    />
    {#if !query}
      <kbd class="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-xs text-zinc-400">⌘F</kbd>
    {/if}
  </div>

  <div class="space-y-0.5">
    {@render navItem("Home", { kind: "home" })}
    {@render navItem("All recordings", { kind: "all" }, recorder.recordings.length)}
  </div>

</nav>
