<script lang="ts">
  import {
    APP_NAV_ITEMS,
    NAV_ITEMS,
    type Destination,
  } from "../../../lib/app/navigation";
  import NarrowNavigation from "./NarrowNavigation.svelte";
  import WideNavigation from "./WideNavigation.svelte";

  let {
    active,
    hrefFor,
    libraryName,
    onlibraries,
  }: {
    active: Destination;
    /** Resolved by the shell, so a destination resumes where it was left. */
    hrefFor: (destination: Destination) => string;
    libraryName: string;
    onlibraries: () => void;
  } = $props();

  const storeItem = APP_NAV_ITEMS.filter((item) => item.id === "store");
  const accountItem = APP_NAV_ITEMS.find((item) => item.id === "account")!;
  const settingsItem = APP_NAV_ITEMS.find((item) => item.id === "settings")!;
  const sidebarItems = [...NAV_ITEMS, ...storeItem];
  const bottomItems = NAV_ITEMS.filter(
    (item) => item.id === "home" || item.id === "folders" || item.id === "subjects",
  );
  const moreItems = [
    ...NAV_ITEMS.filter((item) => item.id === "authors" || item.id === "trash"),
    ...storeItem,
  ];
</script>

<WideNavigation
  {active}
  {hrefFor}
  {libraryName}
  {onlibraries}
  {sidebarItems}
  {accountItem}
  {settingsItem}
/>
<NarrowNavigation
  {active}
  {hrefFor}
  {onlibraries}
  {bottomItems}
  {moreItems}
  {accountItem}
  {settingsItem}
/>
