import { formatDuration, type Recording } from "$lib/recorder.svelte";
import type { Summary } from "$lib/summary";

export function toMarkdown(recording: Recording, summary: Summary | null, notes: string | null) {
  const parts = [
    `# ${recording.title}`,
    `${new Date(recording.startedAt).toLocaleString()} · ${formatDuration(recording.durationSecs)}`,
  ];
  if (summary) {
    parts.push("## Summary", summary.overview);
    for (const section of summary.sections) {
      parts.push(`### ${section.heading.text}`, section.lines.map((line) => `- ${line.text}`).join("\n"));
    }
    if (summary.skipped?.length) parts.push(`_Left out: ${summary.skipped.join(", ")}_`);
  }
  if (notes?.trim()) parts.push("## My notes", notes.trim());
  return parts.filter(Boolean).join("\n\n") + "\n";
}
