<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Mic, Palette, Sparkles } from "@lucide/svelte";
  import { dictation, TALK_KEYS, VOICE_COMMANDS } from "$lib/dictation.svelte";
  import { theme, THEMES } from "$lib/theme.svelte";
  import Button from "./Button.svelte";
  import Toggle from "./Toggle.svelte";

  type LlmProvider = "apple" | "ollama";
  type LlmSettings = { provider: LlmProvider; url: string; model: string };

  let llm = $state<LlmSettings>({
    provider: "apple",
    url: "http://127.0.0.1:11434",
    model: "",
  });
  let models = $state<string[]>([]);
  let llmError = $state("");
  let loadingModels = $state(false);
  let savingLlm = $state(false);
  const PROVIDERS: { value: LlmProvider; label: string; hint: string }[] = [
    { value: "apple", label: "Apple Intelligence", hint: "Runs on this Mac" },
    { value: "ollama", label: "Ollama", hint: "Your own models" },
  ];

  async function loadLlm() {
    try {
      llm = await invoke<LlmSettings>("llm");
      if (llm.provider === "ollama") await refreshModels();
    } catch (e) {
      llmError = String(e);
    }
  }

  async function saveLlm(change: Partial<LlmSettings>) {
    const next = { ...llm, ...change };
    savingLlm = true;
    llmError = "";
    try {
      await invoke("set_llm", { settings: next });
      llm = next;
    } catch (e) {
      llmError = String(e);
    }
    savingLlm = false;
  }

  async function refreshModels() {
    loadingModels = true;
    llmError = "";
    try {
      models = await invoke<string[]>("ollama_models", { url: llm.url });
    } catch (e) {
      models = [];
      llmError = String(e);
    }
    loadingModels = false;
  }

  async function choose(provider: LlmProvider) {
    await saveLlm({ provider });
    if (llm.provider === "ollama") await refreshModels();
  }

  onMount(() => {
    dictation.init();
    loadLlm();
  });

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
      <Button class="shrink-0" onclick={action.run}>{action.label}</Button>
    {/if}
  </div>
{/snippet}

<h2 class="mb-6 text-xl font-semibold">Settings</h2>

<section aria-labelledby="appearance-heading" class="mb-8 space-y-4">
  <div>
    <h3 id="appearance-heading" class="flex items-center gap-2 font-semibold">
      <Palette class="size-4 text-blue-600" aria-hidden="true" />Appearance
    </h3>
    <p class="mt-1 text-sm text-zinc-500">Pick how Aube looks.</p>
  </div>
  <div class="grid grid-cols-3 gap-3" role="radiogroup" aria-label="Theme">
    {#each THEMES as option (option.id)}
      {@const selected = theme.current === option.id}
      <button
        role="radio"
        aria-checked={selected}
        class={[
          "overflow-hidden rounded-lg border text-left transition focus-visible:outline-2 focus-visible:outline-blue-600",
          selected ? "border-blue-600 ring-1 ring-blue-600" : "border-zinc-700 hover:border-zinc-500",
        ]}
        onclick={() => theme.set(option.id)}
      >
        <span class="flex h-20 items-end gap-2 p-3" style:background={option.bg} style:color={option.fg}>
          <span class="text-2xl leading-none" style:font-family={option.font} style:font-weight={option.weight}>Aa</span>
          <span class="ml-auto h-5 w-12" style:background={option.accent} style:border-radius={option.radius}></span>
        </span>
        <span class="block px-3 py-2">
          <span class="block text-sm font-medium">{option.name}</span>
          <span class="block text-xs text-zinc-500">{option.note}</span>
        </span>
      </button>
    {/each}
  </div>
</section>

<section aria-labelledby="intelligence-heading" class="mb-8 space-y-4">
  <div>
    <h3 id="intelligence-heading" class="flex items-center gap-2 font-semibold">
      <Sparkles class="size-4 text-blue-600" aria-hidden="true" />Intelligence
    </h3>
    <p class="mt-1 text-sm text-zinc-500">
      Choose the model that writes meeting summaries and tidies push-to-talk dictation.
    </p>
  </div>

  <fieldset>
    <legend class="mb-2 text-sm font-medium">Model source</legend>
    <div class="grid grid-cols-2 gap-2">
      {#each PROVIDERS as option (option.value)}
        {@const selected = llm.provider === option.value}
        <label
          class={[
            "flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2 text-sm focus-within:outline-2 focus-within:outline-blue-600",
            selected
              ? "border-zinc-900 bg-zinc-900 text-white dark:border-white dark:bg-white dark:text-zinc-900"
              : "border-zinc-300 hover:bg-zinc-100 dark:border-zinc-700 dark:hover:bg-zinc-800",
          ]}
        >
          <input
            type="radio"
            name="llm-provider"
            class="sr-only"
            value={option.value}
            checked={selected}
            disabled={savingLlm}
            onchange={() => choose(option.value)}
          />
          <span>
            <span class="font-medium">{option.label}</span>
            <span class:opacity-60={selected} class="block text-xs">{option.hint}</span>
          </span>
        </label>
      {/each}
    </div>
  </fieldset>

  {#if llm.provider === "ollama"}
    <div class="space-y-3 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
      <label class="block text-sm">
        <span class="mb-1 block font-medium">Ollama address</span>
        <input
          class="w-full rounded-md border border-zinc-300 bg-transparent px-2 py-1.5 focus-visible:outline-2 focus-visible:outline-blue-600 dark:border-zinc-700"
          bind:value={llm.url}
          onchange={() => saveLlm({ url: llm.url }).then(refreshModels)}
          placeholder="http://127.0.0.1:11434"
          spellcheck="false"
        />
      </label>

      <label class="block text-sm">
        <span class="mb-1 block font-medium">Model</span>
        <div class="flex gap-2">
          <input
            class="min-w-0 flex-1 rounded-md border border-zinc-300 bg-transparent px-2 py-1.5 focus-visible:outline-2 focus-visible:outline-blue-600 dark:border-zinc-700"
            bind:value={llm.model}
            onchange={() => saveLlm({ model: llm.model })}
            list="ollama-models"
            placeholder="llama3.2"
            spellcheck="false"
          />
          <datalist id="ollama-models">
            {#each models as model (model)}
              <option value={model}></option>
            {/each}
          </datalist>
          <Button class="shrink-0" onclick={refreshModels} disabled={loadingModels}>
            {loadingModels ? "Checking…" : "Refresh"}
          </Button>
        </div>
        {#if !llm.model}
          <span class="mt-1 block text-xs text-amber-700 dark:text-amber-300">
            Choose a chat model from the list, or type its name.
          </span>
        {/if}
      </label>

      <p class="text-xs text-zinc-500">
        Summaries and dictation cleanup use this model. Transcripts and typed notes are sent to the
        Ollama server; audio is not.
      </p>
    </div>
  {:else}
    <p class="text-xs text-zinc-500">
      Summaries and dictation cleanup run on this Mac with Apple Intelligence.
    </p>
  {/if}

  {#if llmError}
    {@render notice(llmError, "error")}
  {/if}
</section>

<section aria-labelledby="talk-heading" class="space-y-5">
  <div>
    <h3 id="talk-heading" class="flex items-center gap-2 font-semibold">
      <Mic class="size-4 text-blue-600" aria-hidden="true" />Push to talk
    </h3>
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
    <Toggle checked={dictation.settings.cleanup} disabled={!on} onchange={(cleanup) => dictation.update({ cleanup })} />
    <span>
      <span class="font-medium">
        Tidy up with {llm.provider === "ollama" ? "Ollama" : "Apple Intelligence"}
      </span>
      {#if llm.provider === "ollama"}
        <span class="block text-zinc-500">
          Uses {llm.model || "your chosen Ollama model"} to remove fillers, apply corrections, and fix
          punctuation before pasting.
        </span>
      {:else}
        <span class="block text-zinc-500">
          Removes "um"s and repeated words, applies corrections like "no, Wednesday", and fixes punctuation. Runs on this
          Mac.
        </span>
      {/if}
    </span>
  </label>

  {#each VOICE_COMMANDS as command (command.setting)}
    <label class="flex items-start gap-3 text-sm">
      <Toggle
        checked={dictation.settings[command.setting]}
        disabled={!on}
        onchange={(checked) => dictation.update({ [command.setting]: checked })}
      />
      <span>
        <span class="font-medium">{command.title}</span>
        <span class="block text-zinc-500">{command.detail}</span>
      </span>
    </label>
  {/each}

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
