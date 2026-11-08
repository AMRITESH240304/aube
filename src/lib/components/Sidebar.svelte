<script module lang="ts">
  export type View = { kind: "home" } | { kind: "all" } | { kind: "settings" } | { kind: "new" };
</script>

<script lang="ts">
  import { AudioLines, House, Search, Settings } from "@lucide/svelte";
  import { recorder } from "$lib/recorder.svelte";

  let {
    view,
    query = $bindable(),
    openId,
    onNavigate,
    onOpen,
  }: {
    view: View;
    query: string;
    openId: string | null;
    onNavigate: (view: View) => void;
    onOpen: (id: string) => void;
  } = $props();

  let search: HTMLInputElement;

  const isActive = (next: View) => !openId && !query.trim() && next.kind === view.kind;
  const live = $derived(recorder.status);
  const past = $derived(recorder.recordings.filter((r) => r.id !== live?.id));
  const watchingLive = $derived(!!live && !openId && !query.trim() && view.kind === "home");

  const DAY = 86_400_000;

  function when(ms: number) {
    const date = new Date(ms);
    const days = Math.round((new Date().setHours(0, 0, 0, 0) - new Date(ms).setHours(0, 0, 0, 0)) / DAY);
    if (days === 0) return date.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
    if (days === 1) return "Yesterday";
    if (days < 7) return date.toLocaleDateString(undefined, { weekday: "short" });
    return date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "f" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      search.focus();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#snippet navItem(label: string, target: View, Icon: typeof House, count?: number)}
  <button
    class={[
      "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-sm hover:bg-zinc-200/70 dark:hover:bg-zinc-800",
      isActive(target) && "bg-zinc-200 font-medium dark:bg-zinc-800",
    ]}
    aria-current={isActive(target) ? "page" : undefined}
    onclick={() => onNavigate(target)}
  >
    <Icon class="size-4 shrink-0 text-zinc-400" aria-hidden="true" />
    <span class="flex-1 truncate">{label}</span>
    {#if count}
      <span class="text-xs text-zinc-500 tabular-nums">{count}</span>
    {/if}
  </button>
{/snippet}

{#snippet recordingItem(title: string, active: boolean, onclick: () => void, subtitle: string, isLive = false)}
  <li>
    <button
      class={[
        "flex w-full min-w-0 flex-col rounded-md px-2 py-1.5 text-left hover:bg-zinc-800/70",
        active && "bg-[rgb(var(--glow)/0.12)] hover:bg-[rgb(var(--glow)/0.16)]",
      ]}
      aria-current={active ? "page" : undefined}
      {onclick}
    >
      <span class="w-full truncate text-sm">{title}</span>
      <span class="flex items-center gap-1.5 text-xs text-zinc-500 tabular-nums">
        {#if isLive}<span class="ember size-1.5 rounded-full" aria-hidden="true"></span>{/if}{subtitle}
      </span>
    </button>
  </li>
{/snippet}

<nav
  class="flex w-60 shrink-0 flex-col gap-4 border-r border-zinc-200 bg-zinc-50 p-3 pt-11 dark:border-white/[0.07] dark:bg-white/[0.025]"
  aria-label="Sidebar"
>
  <p class="flex items-center gap-2 px-2 font-[family-name:var(--display-font)] text-base font-semibold tracking-wide">
    <img src="/favicon.png" alt="" class="size-5 rounded-[5px] drop-shadow-[0_0_8px_rgb(var(--glow)/0.6)]" />Aube
  </p>

  <div class="relative">
    <label for="search" class="sr-only">Search transcripts</label>
    <Search class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-zinc-500" aria-hidden="true" />
    <input
      id="search"
      type="search"
      bind:this={search}
      bind:value={query}
      placeholder="Search transcripts"
      class="w-full rounded-md border border-zinc-300 bg-white py-1.5 pr-9 pl-8 text-sm focus-visible:outline-2 focus-visible:outline-blue-600 dark:border-zinc-700 dark:bg-zinc-950"
    />
    {#if !query}
      <kbd class="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-xs text-zinc-400">⌘F</kbd>
    {/if}
  </div>

  <div class="space-y-0.5">
    {@render navItem("Home", { kind: "home" }, House)}
    {@render navItem("All recordings", { kind: "all" }, AudioLines, recorder.recordings.length)}
    {@render navItem("Settings", { kind: "settings" }, Settings)}
  </div>

  {#if live || past.length}
    <section class="-mx-1 flex min-h-0 flex-1 flex-col" aria-labelledby="sidebar-recordings">
      <p id="sidebar-recordings" class="mb-1.5 px-3 text-[11px] tracking-[0.08em] text-zinc-500 uppercase">Recordings</p>
      <ul class="min-h-0 space-y-0.5 overflow-y-auto px-1">
        {#if live}
          {@render recordingItem(live.title, watchingLive, () => onNavigate({ kind: "home" }), "Now", true)}
        {/if}
        {#each past as recording (recording.id)}
          {@render recordingItem(recording.title, recording.id === openId, () => onOpen(recording.id), when(recording.startedAt))}
        {/each}
      </ul>
    </section>
  {/if}

</nav>
