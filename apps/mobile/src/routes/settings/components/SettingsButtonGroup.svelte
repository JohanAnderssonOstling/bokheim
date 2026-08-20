<script lang="ts">
  import ButtonGroup from "../../../components/shared/ButtonGroup.svelte";

  type Option = {
    value: string;
    label: string;
    disabled?: boolean;
  };

  let {
    label,
    options,
    value,
    onselect,
    buttonWidth = "5.625rem",
  }: {
    label: string;
    options: readonly Option[];
    value: string;
    onselect: (value: string) => void;
    buttonWidth?: string;
  } = $props();
</script>

<div
  class="setting-button-group"
  role="group"
  aria-label={label}
>
  <span>{label}</span>
  <ButtonGroup
    ariaLabel={label}
    {options}
    {value}
    onSelect={onselect}
    {buttonWidth}
  />
</div>

<style>
  .setting-button-group {
    display: grid;
    grid-template-columns: minmax(6rem, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
  }

  span {
    color: var(--color-subtle-text);
    font-size: 0.75rem;
    font-weight: 750;
  }
</style>
