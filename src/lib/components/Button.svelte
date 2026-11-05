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
  text={!primary}
  elevation={0}
  hover={primary ? 0 : -1}
  rounded={theme.current !== "terminal"}
  color={danger ? "var(--color-red-500)" : undefined}
  class={[primary && "aube-primary", className]}
  {...rest}
/>
