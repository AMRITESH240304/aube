<script lang="ts">
  import { NeoSwitchButton } from "@dvcol/neo-svelte/buttons";
  import { theme } from "$lib/theme.svelte";

  let {
    checked,
    disabled = false,
    onchange,
  }: { checked: boolean; disabled?: boolean; onchange: (checked: boolean) => void } = $props();
</script>

<NeoSwitchButton
  role="switch"
  aria-checked={checked}
  {checked}
  {disabled}
  glass={theme.glass}
  rounded={theme.current !== "terminal"}
  onclick={() => onchange(!checked)}
/>
