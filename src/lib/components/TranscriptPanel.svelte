<script lang="ts">
  import { ChevronDown } from "@lucide/svelte";
  import type { Snippet } from "svelte";
  import type { Segment } from "$lib/transcript";
  import Conversation from "./Conversation.svelte";

  let {
    lines,
    label = "",
    empty,
    follow = false,
    highlight,
    actions,
  }: {
    lines: Pick<Segment, "start" | "speaker" | "text">[];
    label?: string;
    empty: string;
    follow?: boolean;
    highlight?: number;
    actions?: Snippet;
  } = $props();

  let open = $state(true);
  let scroller = $state<HTMLElement>();

  $effect(() => {
    if (!follow || !open || !scroller || !lines.length) return;
    if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 160) {
      scroller.scrollTop = scroller.scrollHeight;
    }
  });
</script>

<section class="flex max-h-[40vh] shrink-0 flex-col border-t border-zinc-200 dark:border-zinc-800" aria-label="Transcript">
  <div class="flex items-center gap-3 px-6 py-2">
    <button
      class="flex items-center gap-1.5 rounded text-xs font-medium tracking-wide text-zinc-500 uppercase hover:text-zinc-900 focus-visible:outline-2 focus-visible:outline-blue-600 dark:hover:text-zinc-100"
      aria-expanded={open}
      onclick={() => (open = !open)}
    >
      <ChevronDown class={["size-3.5 transition-transform", !open && "-rotate-90"]} aria-hidden="true" />
      Transcript
    </button>
    <span class="min-w-0 flex-1 truncate text-xs text-zinc-500">{label}</span>
    {@render actions?.()}
  </div>
  {#if open}
    <div bind:this={scroller} class="min-h-0 overflow-y-auto px-6 pb-4" aria-live={follow ? "polite" : undefined}>
      {#if lines.length}
        <Conversation {lines} {highlight} />
      {:else}
        <p class="py-6 text-center text-sm text-zinc-500">{empty}</p>
      {/if}
    </div>
  {/if}
</section>
