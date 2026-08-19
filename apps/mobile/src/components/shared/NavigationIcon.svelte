<script lang="ts">
  import asterisk from "../../../../ui/design-tokens/assets/phosphor-light/asterisk.svg";
  import bookOpen from "../../../../ui/design-tokens/assets/phosphor-light/book-open.svg";
  import buildings from "../../../../ui/design-tokens/assets/phosphor-light/buildings.svg";
  import dotsThree from "../../../../ui/design-tokens/assets/phosphor-light/dots-three.svg";
  import folder from "../../../../ui/design-tokens/assets/phosphor-light/folder.svg";
  import gear from "../../../../ui/design-tokens/assets/phosphor-light/gear.svg";
  import house from "../../../../ui/design-tokens/assets/phosphor-light/house.svg";
  import tag from "../../../../ui/design-tokens/assets/phosphor-light/tag.svg";
  import trash from "../../../../ui/design-tokens/assets/phosphor-light/trash.svg";
  import user from "../../../../ui/design-tokens/assets/phosphor-light/user.svg";
  import userCircle from "../../../../ui/design-tokens/assets/phosphor-light/user-circle.svg";
  import type { NavigationIcon as NavigationIconName } from "../../lib/bindings.ts";

  let { icon, size = "1em" }: { icon: NavigationIconName | "dots_three"; size?: string } = $props();

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

<span class="navigation-icon" style:width={size} style:height={size} style:--icon-source={`url("${source}")`} aria-hidden="true"></span>

<style>
  .navigation-icon { display: block; width: 1em; height: 1em; flex: none; background: currentColor; mask: var(--icon-source) center / contain no-repeat; line-height: 0; }
</style>
