<script lang="ts">
  import type { Snippet } from "svelte";
  import BaseButton from "./BaseButton.svelte";

  /**
   * A destructive action. Deliberately not a red fill: the outline matches
   * `SecondaryButton` and only the label turns red, so deleting never looks
   * like the button the view wants you to press.
   */
  let {
    children,
    disabled = false,
    onclick,
  }: {
    children: Snippet;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<BaseButton
  variant="danger"
  {disabled}
  {onclick}
>
  {@render children()}
</BaseButton>
