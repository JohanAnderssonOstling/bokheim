<script lang="ts">
  import type { Snippet } from "svelte";
  import BaseButton from "./BaseButton.svelte";

  /**
   * The neutral action: outlined rather than filled, so it can sit next to a
   * `PrimaryButton` without competing with it. This is the default choice for
   * anything that is neither the affirmative action nor destructive.
   */
  let {
    children,
    disabled = false,
    active = false,
    ariaLabel,
    ariaPressed,
    onclick,
  }: {
    children: Snippet;
    disabled?: boolean;
    active?: boolean;
    ariaLabel?: string;
    ariaPressed?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<BaseButton
  variant="secondary"
  {disabled}
  {active}
  {ariaLabel}
  {ariaPressed}
  {onclick}
>
  {@render children()}
</BaseButton>
