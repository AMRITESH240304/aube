<script lang="ts">
  const OVERVIEW = ["w-full", "w-11/12", "w-2/3"];
  const SECTIONS = [
    { heading: "w-1/3", lines: ["w-5/6", "w-3/4", "w-11/12"] },
    { heading: "w-1/4", lines: ["w-4/5", "w-2/3", "w-5/6", "w-1/2"] },
    { heading: "w-2/5", lines: ["w-3/4", "w-5/6"] },
  ];

  const rows = [
    ...OVERVIEW.map((width) => ({ width, kind: "overview" })),
    ...SECTIONS.flatMap((section) => [
      { width: section.heading, kind: "heading" },
      ...section.lines.map((width) => ({ width, kind: "line" })),
    ]),
  ];
</script>

<div role="status" aria-label="Writing the summary on this Mac" class="py-1">
  <span class="sr-only">Writing the summary on this Mac…</span>
  <div class="space-y-2.5" aria-hidden="true">
    {#each rows as row, i (i)}
      <div
        class={[
          "shimmer",
          row.width,
          row.kind === "heading" && "mt-6 h-5",
          row.kind === "line" && "ml-6",
          i === OVERVIEW.length - 1 && "mb-2",
        ]}
        style:--row={i}
      ></div>
    {/each}
  </div>
</div>
