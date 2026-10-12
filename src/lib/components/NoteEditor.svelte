<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Editor, type JSONContent } from "@tiptap/core";
  import type { Node } from "@tiptap/pm/model";
  import StarterKit from "@tiptap/starter-kit";
  import UniqueID from "@tiptap/extension-unique-id";
  import { Placeholder } from "@tiptap/extensions";

  const SAVE_AFTER_MS = 500;
  const LINE_TYPES = ["heading", "paragraph", "codeBlock"];
  const CONTAINER_MARKERS: Record<string, string> = { bulletList: "- ", orderedList: "1. ", blockquote: "> " };

  let { recordingId }: { recordingId: string } = $props();

  let element = $state<HTMLElement>();
  let editor: Editor | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let dirty = false;
  let destroyed = false;

  function marker(doc: Node, pos: number, node: Node) {
    if (node.type.name === "heading") return "#".repeat(node.attrs.level) + " ";
    const resolved = doc.resolve(pos);
    for (let depth = resolved.depth; depth > 0; depth--) {
      const found = CONTAINER_MARKERS[resolved.node(depth).type.name];
      if (found) return found;
    }
    return "";
  }

  function lines(doc: Node) {
    const found: { id: string; text: string }[] = [];
    doc.descendants((node, pos) => {
      if (!node.isTextblock) return true;
      if (node.attrs.id && node.textContent.trim()) {
        found.push({ id: node.attrs.id, text: marker(doc, pos, node) + node.textContent });
      }
      return false;
    });
    return found;
  }

  function save() {
    clearTimeout(timer);
    if (!editor || !dirty) return;
    dirty = false;
    const found = lines(editor.state.doc);
    invoke("save_note", {
      recordingId,
      doc: JSON.stringify(editor.getJSON()),
      markdown: found.map((line) => line.text).join("\n"),
      lines: found,
    }).catch(console.error);
  }

  onMount(async () => {
    const saved = await invoke<string | null>("note", { recordingId });
    if (destroyed) return;
    editor = new Editor({
      element,
      extensions: [
        StarterKit,
        UniqueID.configure({ types: LINE_TYPES }),
        Placeholder.configure({ placeholder: "Write notes…" }),
      ],
      content: saved ? (JSON.parse(saved) as JSONContent) : "",
      editorProps: {
        attributes: { class: "note", "aria-label": "Notes" },
        handleKeyDown: (_view, event) => {
          if (event.key === "Enter") queueMicrotask(save);
          return false;
        },
      },
      onUpdate: () => {
        dirty = true;
        clearTimeout(timer);
        timer = setTimeout(save, SAVE_AFTER_MS);
      },
      onBlur: save,
    });
  });

  onDestroy(() => {
    destroyed = true;
    save();
    editor?.destroy();
  });
</script>

<div bind:this={element}></div>
