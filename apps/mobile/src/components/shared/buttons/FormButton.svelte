<script lang="ts">
  import type { Snippet } from "svelte";
  import BaseButton from "./BaseButton.svelte";

  /**
   * The submit button of a `<form>`. Looks like `PrimaryButton` — a form has
   * one affirmative action and this is it — but renders `type="submit"`, so
   * the form's `onsubmit` runs for a click and for Enter in a field alike.
   *
   * Keeping this separate is what lets every other button hardcode
   * `type="button"`: submitting is a distinct job, not a flag.
   */
  let {
    children,
    disabled = false,
  }: {
    children: Snippet;
    disabled?: boolean;
  } = $props();
</script>

<BaseButton
  variant="primary"
  type="submit"
  {disabled}
>
  {@render children()}
</BaseButton>
