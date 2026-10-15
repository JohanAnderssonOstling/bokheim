#include <cassert>
#include <cstddef>
#include <cstring>

// Stand-ins for NickelMenu's APIs so the actual extension can run on the host.
#define NM_MENU_LOCATION(name) NM_MENU_LOCATION_##name
enum { NM_MENU_LOCATION_main, NM_MENU_LOCATION_reader };
struct nm_menu_item_t { int loc; const char *lbl; };
static const char *configured_label;
static const char *config_error;
static nm_menu_item_t *launched;
static int launch_count;
static const char *nm_global_config_experimental(const char *key) {
    assert(std::strcmp(key, "menu_main_15505_direct") == 0);
    return configured_label;
}
static const char *nm_err_peek() { return config_error; }
static void nm_menu_item_do(nm_menu_item_t *item, void *, void *) {
    launched = item;
    ++launch_count;
}

#include "../../kobo/nickelmenu-direct/direct-launch.h"

int main() {
    nm_menu_item_t other = { NM_MENU_LOCATION_main, "Other app" };
    nm_menu_item_t target = { NM_MENU_LOCATION_main, "Bokheim Kobo" };
    nm_menu_item_t reader = { NM_MENU_LOCATION_reader, "Bokheim Kobo" };
    nm_menu_item_t duplicate = { NM_MENU_LOCATION_main, "Bokheim Kobo" };
    nm_menu_item_t *items[] = { &other, &reader, &target, &duplicate };
    configured_label = "Bokheim Kobo";

    // The named main-menu action runs immediately, even with other apps present.
    assert(nm_try_direct_launch(items, 3));
    assert(launched == &target && launch_count == 1);
    launched = NULL;

    // Missing or ambiguous targets and parser errors leave the normal menu.
    assert(!nm_try_direct_launch(items, 2));
    assert(!nm_try_direct_launch(items, 4));
    config_error = "Invalid configuration";
    assert(!nm_try_direct_launch(items, 3));
    config_error = NULL;
    configured_label = "Missing app";
    assert(!nm_try_direct_launch(items, 3));
    configured_label = "";
    assert(!nm_try_direct_launch(items, 3));
    configured_label = NULL;
    assert(!nm_try_direct_launch(items, 3));
    configured_label = "Bokheim Kobo";
    assert(!nm_try_direct_launch(NULL, 0));
    assert(!nm_try_direct_launch(items, 0));
    assert(!launched && launch_count == 1);
}
