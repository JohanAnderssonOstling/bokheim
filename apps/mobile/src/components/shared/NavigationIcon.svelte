<script lang="ts">
  import { Asterisk, BookOpen, Building2, CircleUser, Folder, House, Settings, Tags, Trash2, User } from "@lucide/svelte";
  import type { NavigationIcon as NavigationIconName } from "../../lib/bindings.ts";

  let { icon, size = "1em" }: { icon: NavigationIconName; size?: string } = $props();
</script>

<span class="navigation-icon" style:width={size} style:height={size} aria-hidden="true">
  {#if icon === "house"}
    <House size="100%" />
  {:else if icon === "book_open"}
    <BookOpen size="100%" />
  {:else if icon === "folder"}
    <Folder size="100%" />
  {:else if icon === "asterisk"}
    <Asterisk size="100%" />
  {:else if icon === "tags"}
    <Tags size="100%" />
  {:else if icon === "user"}
    <User size="100%" />
  {:else if icon === "trash2"}
    <Trash2 size="100%" />
  {:else if icon === "building2"}
    <Building2 size="100%" />
  {:else if icon === "circle_user"}
    <CircleUser size="100%" />
  {:else}
    <Settings size="100%" />
  {/if}
</span>

<style>
  .navigation-icon { display: block; width: 1em; height: 1em; flex: none; line-height: 0; }
  .navigation-icon :global(svg) { display: block; width: 100%; height: 100%; }
</style>
