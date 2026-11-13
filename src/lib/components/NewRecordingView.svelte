<script lang="ts">
  import { recorder, type NoteDraft } from "$lib/recorder.svelte";
  import NoteEditor from "./NoteEditor.svelte";

  function remember(draft: NoteDraft) {
    if (!recorder.status) recorder.draftNote = draft;
  }
</script>

<div>
  <h1 class="text-xl font-semibold">New note</h1>
  <p class="mb-6 text-sm text-zinc-400">Write an agenda or questions now. Press Record when the meeting starts and they're kept with it.</p>
  <NoteEditor draft={recorder.draftNote ?? undefined} onChange={remember} />
</div>
