<script lang="ts">
  import { recorder, type NoteDraft } from "$lib/recorder.svelte";
  import NoteEditor from "./NoteEditor.svelte";

  function remember(draft: NoteDraft) {
    if (!recorder.status) recorder.draftNote = draft;
  }
</script>

<div>
  <h1 class="text-xl font-semibold">New recording</h1>
  <p class="mb-6 text-sm text-zinc-500">Write your notes now, then press Record when you're ready.</p>
  <NoteEditor draft={recorder.draftNote ?? undefined} onChange={remember} />
</div>
