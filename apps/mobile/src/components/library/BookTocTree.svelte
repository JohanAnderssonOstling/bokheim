<script lang="ts">
  import type { BookTocEntry } from "../../lib/bindings";
  import BookTocTree from "./BookTocTree.svelte";

  let { entries, onopen }: { entries: BookTocEntry[]; onopen: (target: string) => void | Promise<void> } = $props();
  function key(entry: BookTocEntry, index: number) { return `${index}:${entry.target}:${entry.title}`; }
  let expanded = $state(new Set<string>());
  function toggle(entryKey: string) {
    const next = new Set(expanded);
    if (next.has(entryKey)) next.delete(entryKey); else next.add(entryKey);
    expanded = next;
  }
</script>

<ol class="toc-level">
  {#each entries as entry, index (key(entry, index))}
    {@const entryKey = key(entry, index)}
    <li>
      <div class="toc-row">
        {#if entry.children.length}
          <button class="disclosure" type="button" aria-label={`${expanded.has(entryKey) ? "Collapse" : "Expand"} ${entry.title}`} aria-expanded={expanded.has(entryKey)} onclick={() => toggle(entryKey)}>⌄</button>
        {:else}<span class="disclosure-spacer"></span>{/if}
        <button class="toc-link" type="button" onclick={() => onopen(entry.target)}>{entry.title || "Untitled section"}</button>
      </div>
      {#if entry.children.length && expanded.has(entryKey)}
        <BookTocTree entries={entry.children} {onopen} />
      {/if}
    </li>
  {/each}
</ol>

<style>
  .toc-level { display: grid; gap: .1rem; margin: 0; padding: 0; list-style: none; }
  .toc-level :global(.toc-level) { margin-left: 1.15rem; padding-left: .75rem; border-left: 1px solid var(--color-border); }
  .toc-row { display: grid; grid-template-columns: 2.75rem minmax(0, 1fr); align-items: start; min-height: 2.75rem; }
  button { font: inherit; }
  .disclosure { display: grid; place-items: center; width: 2.75rem; min-height: 2.75rem; padding: 0; border: 0; color: var(--color-subtle-text); background: transparent; cursor: pointer; font-size: 1rem; }
  .disclosure[aria-expanded="false"] { transform: rotate(-90deg); }
  .disclosure-spacer { width: 2.75rem; }
  .toc-link { min-height: 2.75rem; padding: .62rem .25rem; border: 0; color: var(--color-ink-text); background: transparent; text-align: left; cursor: pointer; line-height: 1.35; }
  .toc-link:hover { color: var(--color-accent); text-decoration: underline; text-underline-offset: .18em; }
  @media (prefers-reduced-motion: no-preference) { .disclosure { transition: transform .12s ease; } }
</style>
