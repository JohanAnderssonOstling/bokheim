// Bokheim extension to NickelMenu v0.6.0. Included after NickelMenu's config,
// error, and action declarations. Leave the usual menu available on any error
// or when the configured main-menu label is missing or ambiguous.
static bool nm_try_direct_launch(nm_menu_item_t **items, size_t count) {
    const char *label = nm_global_config_experimental("menu_main_15505_direct");
    if (!items || !label || !*label || nm_err_peek())
        return false;

    nm_menu_item_t *target = NULL;
    for (size_t i = 0; i < count; ++i) {
        nm_menu_item_t *item = items[i];
        if (item->loc != NM_MENU_LOCATION(main) || strcmp(item->lbl, label) != 0)
            continue;
        if (target)
            return false;
        target = item;
    }
    if (!target)
        return false;

    nm_menu_item_do(target, NULL, NULL);
    return true;
}
