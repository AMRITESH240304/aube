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

  const HEIGHT_KEY = "transcript-height";

  let open = $state(true);
  let scroller = $state<HTMLElement>();
  let panel = $state<HTMLElement>();
  let height = $state(savedHeight());

  function savedHeight() {
    try {
      return Number(localStorage.getItem(HEIGHT_KEY)) || null;
    } catch {
      return null;
    }
  }

  function setHeight(next: number) {
    height = Math.round(Math.min(Math.max(next, 96), window.innerHeight * 0.8));
    try {
      localStorage.setItem(HEIGHT_KEY, String(height));
    } catch {}
  }

  function startResize(e: PointerEvent) {
    if (!panel) return;
    const handle = e.currentTarget as HTMLElement;
    const startY = e.clientY;
    const startHeight = panel.offsetHeight;
    const move = (m: PointerEvent) => setHeight(startHeight + startY - m.clientY);
    handle.setPointerCapture(e.pointerId);
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", () => handle.removeEventListener("pointermove", move), { once: true });
  }

  function resizeWithKeys(e: KeyboardEvent) {
    const step = { ArrowUp: 32, ArrowDown: -32 }[e.key];
    if (!step || !panel) return;
    e.preventDefault();
    setHeight(panel.offsetHeight + step);
  }

  $effect(() => {
    if (!follow || !open || !scroller || !lines.length) return;
    if (scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 160) {
      scroller.scrollTop = scroller.scrollHeight;
    }
  });
</script>

<section
  bind:this={panel}
  class={[
    "relative flex shrink-0 flex-col border-t border-zinc-200 dark:border-zinc-800",
    !(open && height) && "max-h-[40vh]",
  ]}
  style:height={open && height ? `${height}px` : undefined}
  aria-label="Transcript"
>
  {#if open}
    <button
      type="button"
      class="group absolute inset-x-0 -top-1.5 z-10 flex h-3 cursor-row-resize justify-center focus-visible:outline-none"
      aria-label="Resize transcript, use the up and down arrow keys"
      title="Drag to resize"
      onpointerdown={startResize}
      onkeydown={resizeWithKeys}
    >
      <span
        class="mt-1 h-1 w-10 rounded-full bg-zinc-700 opacity-0 transition group-hover:opacity-100 group-focus-visible:bg-blue-600 group-focus-visible:opacity-100"
      ></span>
    </button>
  {/if}
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
