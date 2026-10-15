<script lang="ts">
  import { onMount } from "svelte";
  import { dictation, TALK_KEYS } from "$lib/dictation.svelte";

  onMount(() => dictation.init());

  const chosen = $derived(TALK_KEYS.find((k) => k.key === dictation.settings.key) ?? TALK_KEYS[0]);
  const status = $derived(dictation.status);
  const on = $derived(dictation.settings.key !== "off");
  const ready = $derived(!!status?.ready && status.accessibility && !status.error);
</script>

{#snippet notice(text: string, tone: "warn" | "error" | "ok", action?: { label: string; run: () => void })}
  <div
    class={[
      "flex items-start gap-3 rounded-lg border px-4 py-3 text-sm",
      tone === "warn" && "border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-200",
      tone === "error" && "border-red-200 bg-red-50 text-red-800 dark:border-red-900 dark:bg-red-950/40 dark:text-red-300",
      tone === "ok" && "border-emerald-200 bg-emerald-50 text-emerald-900 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-200",
    ]}
    role={tone === "ok" ? "status" : "alert"}
  >
    <p class="flex-1">{text}</p>
    {#if action}
      <button class="btn shrink-0 bg-white py-1 text-xs dark:bg-zinc-900" onclick={action.run}>{action.label}</button>
    {/if}
  </div>
{/snippet}

<h2 class="mb-6 text-xl font-semibold">Settings</h2>

<section aria-labelledby="talk-heading" class="space-y-5">
  <div>
    <h3 id="talk-heading" class="font-semibold">Push to talk</h3>
    <p class="mt-1 text-sm text-zinc-500">
      Hold a key anywhere on your Mac and talk. What you say appears under the notch, and when you let go it's typed
      into the app you're using.
    </p>
  </div>

  <fieldset>
    <legend class="mb-2 text-sm font-medium">Key to hold</legend>
    <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
      {#each TALK_KEYS as option (option.key)}
        {@const selected = option.key === dictation.settings.key}
        <label
          class={[
            "flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-sm focus-within:outline-2 focus-within:outline-blue-600",
            selected
              ? "border-zinc-900 bg-zinc-900 text-white dark:border-white dark:bg-white dark:text-zinc-900"
              : "border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800",
          ]}
        >
          <input
            type="radio"
            name="talk-key"
            class="sr-only"
            value={option.key}
            checked={selected}
            onchange={() => dictation.update({ key: option.key })}
          />
          {#if option.symbol}
            <kbd class="w-5 text-center font-sans">{option.symbol}</kbd>
          {/if}
          {option.label}
        </label>
      {/each}
    </div>
  </fieldset>

  <label class="flex items-start gap-3 text-sm">
    <input
      type="checkbox"
      class="mt-0.5 size-4 accent-zinc-900 dark:accent-white"
      checked={dictation.settings.cleanup}
      disabled={!on}
      onchange={(e) => dictation.update({ cleanup: e.currentTarget.checked })}
    />
    <span>
      <span class="font-medium">Tidy up with Apple Intelligence</span>
      <span class="block text-zinc-500">
        Removes "um"s and repeated words, applies corrections like "no, Wednesday", and fixes punctuation. Runs on this
        Mac.
      </span>
    </span>
  </label>

  {#if on}
    <div class="space-y-2">
      {#if dictation.error}
        {@render notice(dictation.error, "error")}
      {/if}
      {#if !status}
        {@render notice("Starting push to talk…", "warn")}
      {:else if status.error}
        {@render notice(status.error, "error")}
      {:else}
        {#if !status.accessibility}
          {@render notice(
            "Aube needs Accessibility access to notice the key and type your words into other apps. Turn on Aube in the list, then come back.",
            "warn",
            { label: "Open Accessibility", run: () => dictation.openSystemSettings("accessibility") },
          )}
        {/if}
        {#if status.fnConflict}
          {@render notice(
            'macOS also uses the Fn key. In Keyboard settings, set "Press 🌐 key to" to "Do Nothing", or pick another key here.',
            "warn",
            { label: "Open Keyboard", run: () => dictation.openSystemSettings("keyboard") },
          )}
        {/if}
        {#if status.cleanup && dictation.settings.cleanup}
          {@render notice(`${status.cleanup} Until then, what you say is typed as heard.`, "warn")}
        {/if}
        {#if ready}
          {@render notice(`Ready. Hold ${chosen.label} ${chosen.symbol} and talk; let go to type it.`, "ok")}
        {:else if status.accessibility && !status.ready}
          {@render notice("Getting the speech model ready…", "warn")}
        {/if}
      {/if}
    </div>
  {/if}
</section>
