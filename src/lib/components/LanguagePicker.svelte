<script lang="ts">
  import { tick } from "svelte";
  import { AUTO_LANGUAGE, recorder } from "$lib/recorder.svelte";

  let open = $state(false);
  let query = $state("");
  let root = $state<HTMLElement>();
  let search = $state<HTMLInputElement>();

  const label = $derived(
    recorder.language === AUTO_LANGUAGE
      ? "Auto-detect"
      : (recorder.languages.find((l) => l.id === recorder.language)?.name ?? "Mac's language"),
  );
  const matches = $derived(
    recorder.languages.filter((l) => l.name.toLowerCase().includes(query.trim().toLowerCase())),
  );

  async function toggle() {
    open = !open;
    query = "";
    if (open) {
      await tick();
      search?.focus();
    }
  }

  function choose(id: string) {
    open = false;
    recorder.setLanguage(id);
  }

  function onWindowClick(e: MouseEvent) {
    if (open && !root?.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && (open = false)} />

{#snippet item(id: string, name: string, installed: boolean)}
  {@const selected = id === recorder.language}
  <li>
    <button
      class={[
        "flex w-full items-center justify-between gap-3 rounded-lg px-2.5 py-1.5 text-left text-sm hover:bg-zinc-100 focus-visible:bg-zinc-100 focus-visible:outline-none dark:hover:bg-zinc-800 dark:focus-visible:bg-zinc-800",
        selected && "font-medium",
      ]}
      aria-current={selected || undefined}
      onclick={() => choose(id)}
    >
      {name}
      {#if selected}
        <span aria-hidden="true">✓</span>
      {:else if !installed}
        <span class="text-zinc-400" title="Downloads when you pick it">↓</span>
      {/if}
    </button>
  </li>
{/snippet}

<div class="relative" bind:this={root}>
  <button
    class="flex items-center gap-1.5 rounded-full py-2 pr-2.5 pl-3 text-sm hover:bg-zinc-100 focus-visible:outline-2 focus-visible:outline-blue-600 disabled:opacity-50 dark:hover:bg-zinc-800"
    aria-haspopup="true"
    aria-expanded={open}
    aria-label={`Transcription language: ${label}`}
    disabled={recorder.installing}
    onclick={toggle}
  >
    {label}
    <svg viewBox="0 0 16 16" class="size-3 fill-current opacity-60" aria-hidden="true"><path d="M4 6l4 4 4-4z" /></svg>
  </button>

  {#if open}
    <div
      class="absolute bottom-full left-0 mb-2 w-60 overflow-hidden rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900"
    >
      <label for="language-search" class="sr-only">Search languages</label>
      <input
        id="language-search"
        bind:this={search}
        bind:value={query}
        placeholder="Search languages"
        class="w-full border-b border-zinc-200 bg-transparent px-3 py-2.5 text-sm outline-none placeholder:text-zinc-400 dark:border-zinc-800"
        onkeydown={(e) => e.key === "Enter" && matches[0] && choose(matches[0].id)}
      />
      <ul class="max-h-64 overflow-y-auto p-1" aria-label="Languages">
        {#if !query.trim()}
          {@render item(AUTO_LANGUAGE, "Auto-detect", true)}
          {@render item("", "Mac's language", true)}
        {/if}
        {#each matches as language (language.id)}
          {@render item(language.id, language.name, language.installed)}
        {:else}
          <li class="px-2.5 py-1.5 text-sm text-zinc-500">No language matches “{query.trim()}”</li>
        {/each}
      </ul>
    </div>
  {/if}
</div>
