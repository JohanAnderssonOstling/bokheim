<script lang="ts">
  import type { Snippet } from "svelte";
  import BaseButton from "./BaseButton.svelte";

  /**
   * The affirmative action of a view: filled with the accent so exactly one
   * button in a group draws the eye. Pair it with `SecondaryButton` for the
   * dismissing half of a dialog footer. Inside a `<form>`, use `FormButton`.
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
  variant="primary"
  {disabled}
  {onclick}
>
  {@render children()}
</BaseButton>
