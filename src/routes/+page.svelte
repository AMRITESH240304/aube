<script lang="ts">
  import { onMount } from "svelte";
  import LiveView from "$lib/components/LiveView.svelte";
  import RecordBar from "$lib/components/RecordBar.svelte";
  import RecordingView from "$lib/components/RecordingView.svelte";
  import Recordings from "$lib/components/Recordings.svelte";
  import SearchResults from "$lib/components/SearchResults.svelte";
  import Sidebar, { type View } from "$lib/components/Sidebar.svelte";
  import { recorder } from "$lib/recorder.svelte";

  let view = $state<View>({ kind: "home" });
  let query = $state("");
  let openId = $state<string | null>(null);
  let openAt = $state<number | undefined>();

  const open = $derived(recorder.recordings.find((r) => r.id === openId));

  onMount(() => recorder.init());

  function navigate(next: View) {
    view = next;
    query = "";
    openId = null;
  }

  function openRecording(id: string, at?: number) {
    openId = id;
    openAt = at;
  }
</script>

<div class="flex h-screen">
  <Sidebar {view} bind:query onNavigate={navigate} />

  <div class="relative flex min-w-0 flex-1 flex-col">
    <main class="flex-1 overflow-y-auto pb-24">
      {#if open}
        {#key open.id}
          <RecordingView recording={open} at={openAt} onBack={() => (openId = null)} />
        {/key}
      {:else}
        <div class="mx-auto w-full max-w-3xl px-8 py-8">
          {#if query.trim()}
            <SearchResults {query} onOpen={openRecording} />
          {:else if view.kind === "home" && recorder.status}
            <LiveView />
          {:else if view.kind === "home"}
            <Recordings limit={5} onOpen={openRecording} onViewAll={() => navigate({ kind: "all" })} />
          {:else}
            <Recordings onOpen={openRecording} />
          {/if}
        </div>
      {/if}
    </main>
    <RecordBar continuing={open} onStarted={() => navigate({ kind: "home" })} onSaved={openRecording} />
  </div>
</div>
