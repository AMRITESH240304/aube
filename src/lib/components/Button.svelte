<script lang="ts">
  import { NeoButton, type NeoButtonProps } from "@dvcol/neo-svelte/buttons";
  import { theme } from "$lib/theme.svelte";

  let {
    primary = false,
    danger = false,
    class: className,
    ...rest
  }: NeoButtonProps & { primary?: boolean; danger?: boolean } = $props();
</script>

<NeoButton
  glass={theme.glass}
  rounded={theme.current !== "terminal"}
  color={danger ? "var(--color-red-500)" : undefined}
  class={[primary && "aube-primary", className]}
  {...rest}
/>
