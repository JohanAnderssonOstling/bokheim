<script lang="ts">
  let {
    source,
    size = "1rem",
    label,
  }: {
    source: string;
    size?: string;
    label?: string;
  } = $props();
</script>

<span
  class="file-icon"
  class:decorative={!label}
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
  style:width={size}
  style:height={size}
  style:--icon-source={`url("${source}")`}
></span>

<style>
  .file-icon {
    display: inline-block;
    flex: none;
    background: currentColor;
    mask: var(--icon-source) center / contain no-repeat;
    line-height: 0;
  }
  .decorative {
    pointer-events: none;
  }
</style>
