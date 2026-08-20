<script lang="ts">
  import asterisk from "@icons/phosphor/asterisk.svg";
  import bookOpen from "@icons/phosphor/book-open.svg";
  import buildings from "@icons/phosphor/buildings.svg";
  import dotsThree from "@icons/phosphor/dots-three.svg";
  import folder from "@icons/phosphor/folder.svg";
  import gear from "@icons/phosphor/gear.svg";
  import house from "@icons/phosphor/house.svg";
  import tag from "@icons/phosphor/tag.svg";
  import trash from "@icons/phosphor/trash.svg";
  import user from "@icons/phosphor/user.svg";
  import userCircle from "@icons/phosphor/user-circle.svg";
  import type { NavigationIcon as NavigationIconName } from "../../lib/bindings.ts";

  let { icon, size = "1em" }: { icon: NavigationIconName | "dots_three"; size?: string } =
    $props();

  const iconSources = {
    house,
    book_open: bookOpen,
    folder,
    asterisk,
    tags: tag,
    user,
    trash2: trash,
    building2: buildings,
    circle_user: userCircle,
    settings: gear,
    dots_three: dotsThree,
  } satisfies Record<NavigationIconName | "dots_three", string>;

  const source = $derived(iconSources[icon]);
</script>

<span
  class="navigation-icon"
  style:width={size}
  style:height={size}
  style:--icon-source={`url("${source}")`}
  aria-hidden="true"
></span>

<style>
  .navigation-icon {
    display: block;
    width: 1em;
    height: 1em;
    flex: none;
    background: currentColor;
    mask: var(--icon-source) center / contain no-repeat;
    line-height: 0;
  }
</style>
