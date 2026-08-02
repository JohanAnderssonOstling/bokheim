use crate::components::{books_scroll, inline_message, library_item_card, page_surface, secondary_button};
use crate::page_navigation::Signals;
use crate::style::palette;
use epub_svelte_lib::app_core::AppCore;
use epub_svelte_lib::opds::{OpdsCatalog, OpdsCredentials, OpdsEndpoint, OpdsEndpointAuthentication, OpdsEndpointId, OpdsEpubAcquisition, OpdsPublication, OpdsSearch};
use floem::event::EventPropagation;
use floem::ext_event::create_ext_action;
use floem::prelude::{create_rw_signal, dyn_view, h_stack, label, text_input, v_stack, Decorators, RwSignal, SignalGet, SignalUpdate, View};
use floem::reactive::Scope;
use floem::style::FlexWrap;
use floem::views::stack_from_iter;
use std::collections::HashMap;

#[derive(Clone)]
enum CatalogTransition {
    Root,
    Push(String),
    Replace(usize),
    Search(String),
}

pub fn store_tab_view(signals: Signals) -> impl View {
    let selected = create_rw_signal(None::<OpdsEndpoint>);
    let catalog = create_rw_signal(None::<OpdsCatalog>);
    let search = create_rw_signal(None::<OpdsSearch>);
    let levels = create_rw_signal(Vec::<(String, String)>::new());
    let username = create_rw_signal(String::new());
    let password = create_rw_signal(String::new());
    let query = create_rw_signal(String::new());
    let loading = create_rw_signal(false);
    let error = create_rw_signal(None::<String>);
    let imports = create_rw_signal(HashMap::<String, String>::new());

    page_surface(dyn_view(move || -> Box<dyn View> {
        match selected.get() {
            None => Box::new(source_picker(signals.clone(), selected, catalog, search, levels, username, password, query, loading, error)),
            Some(endpoint) => Box::new(catalog_workspace(signals.clone(), endpoint, selected, catalog, search, levels, username, password, query, loading, error, imports)),
        }
    }))
}

fn source_picker(
    signals: Signals, selected: RwSignal<Option<OpdsEndpoint>>, catalog: RwSignal<Option<OpdsCatalog>>, search: RwSignal<Option<OpdsSearch>>, levels: RwSignal<Vec<(String, String)>>, username: RwSignal<String>, password: RwSignal<String>,
    query: RwSignal<String>, loading: RwSignal<bool>, error: RwSignal<Option<String>>,
) -> impl View {
    let endpoints = signals.app_core.opds_endpoints();
    let source_count = endpoints.len();
    let cards = stack_from_iter(endpoints.into_iter().map(move |endpoint| {
        let details = format!(
            "{} · {}",
            endpoint.languages.join(" · "),
            match endpoint.authentication {
                OpdsEndpointAuthentication::None => "Open access",
                OpdsEndpointAuthentication::Basic { .. } => "Membership",
            }
        );
        let endpoint_for_click = endpoint.clone();
        library_item_card(endpoint.title.clone(), format!("{}\n{details}", endpoint.description)).on_click({
            let core = signals.app_core.clone();
            move |_| {
                selected.set(Some(endpoint_for_click.clone()));
                catalog.set(None);
                search.set(None);
                levels.set(Vec::new());
                username.set(String::new());
                password.set(String::new());
                query.set(String::new());
                error.set(None);
                if matches!(endpoint_for_click.authentication, OpdsEndpointAuthentication::None) {
                    request_catalog(core.clone(), endpoint_for_click.id, None, None, CatalogTransition::Root, catalog, search, levels, loading, error);
                }
                EventPropagation::Stop
            }
        })
    }))
    .style(|style| style.width_full().gap(14).flex_row().flex_wrap(FlexWrap::Wrap));
    let heading = v_stack((
        label(|| "STORE".to_owned()).style(|style| style.font_size(12).font_bold().color(palette().accent)),
        label(|| "Online catalogs".to_owned()).style(|style| style.font_size(28).font_bold()),
        label(move || format!("Browse free and open books from {source_count} sources.")).style(|style| style.color(palette().subtle_text)),
    ))
    .style(|style| style.gap(6));
    let error_view = store_error(error);
    books_scroll(v_stack((heading, error_view, cards)).style(|style| style.width_full().padding(24).gap(20)))
}

fn catalog_workspace(
    signals: Signals, endpoint: OpdsEndpoint, selected: RwSignal<Option<OpdsEndpoint>>, catalog: RwSignal<Option<OpdsCatalog>>, search: RwSignal<Option<OpdsSearch>>, levels: RwSignal<Vec<(String, String)>>, username: RwSignal<String>,
    password: RwSignal<String>, query: RwSignal<String>, loading: RwSignal<bool>, error: RwSignal<Option<String>>, imports: RwSignal<HashMap<String, String>>,
) -> impl View {
    let back = secondary_button("← Sources").on_click(move |_| {
        selected.set(None);
        catalog.set(None);
        error.set(None);
        EventPropagation::Stop
    });
    let title = endpoint.title.clone();
    let languages = endpoint.languages.join(" · ");
    let heading = v_stack((label(move || title.clone()).style(|style| style.font_size(24).font_bold()), label(move || languages.clone()).style(|style| style.color(palette().subtle_text))));
    let header = h_stack((back, heading)).style(|style| style.items_center().gap(16));

    let body: Box<dyn View> = if matches!(endpoint.authentication, OpdsEndpointAuthentication::Basic { .. }) && catalog.get().is_none() {
        Box::new(credentials_panel(signals.app_core.clone(), endpoint, catalog, search, levels, username, password, loading, error))
    } else {
        match catalog.get() {
            Some(current) => Box::new(catalog_content(signals, endpoint, current, catalog, search, levels, username, password, query, loading, error, imports)),
            None if loading.get() => Box::new(inline_message("Opening catalog…")),
            None => Box::new(
                v_stack((
                    store_error(error),
                    secondary_button("Try again").on_click({
                        let core = signals.app_core.clone();
                        move |_| {
                            request_catalog(core.clone(), endpoint.id, None, credentials_from_signals(endpoint.id, username, password).ok().flatten(), CatalogTransition::Root, catalog, search, levels, loading, error);
                            EventPropagation::Stop
                        }
                    }),
                ))
                .style(|style| style.gap(12)),
            ),
        }
    };
    v_stack((header, body)).style(|style| style.size_full().min_height(0).padding(24).gap(18))
}

fn credentials_panel(
    core: AppCore, endpoint: OpdsEndpoint, catalog: RwSignal<Option<OpdsCatalog>>, search: RwSignal<Option<OpdsSearch>>, levels: RwSignal<Vec<(String, String)>>, username: RwSignal<String>, password: RwSignal<String>,
    loading: RwSignal<bool>, error: RwSignal<Option<String>>,
) -> impl View {
    let username_label = match &endpoint.authentication {
        OpdsEndpointAuthentication::Basic { username_label, .. } => username_label.clone(),
        OpdsEndpointAuthentication::None => "Username".to_owned(),
    };
    let connect = secondary_button("Open catalog").on_click(move |_| {
        match credentials_from_signals(endpoint.id, username, password) {
            Ok(Some(credentials)) => request_catalog(core.clone(), endpoint.id, None, Some(credentials), CatalogTransition::Root, catalog, search, levels, loading, error),
            Ok(None) => error.set(Some("Enter the catalog username".to_owned())),
            Err(message) => error.set(Some(message)),
        }
        EventPropagation::Stop
    });
    v_stack((
        label(|| "Sign in to this catalog".to_owned()).style(|style| style.font_size(18).font_bold()),
        label(move || endpoint.description.clone()).style(|style| style.color(palette().subtle_text)),
        label(move || username_label.clone()),
        text_input(username),
        label(|| "Password is intentionally empty for Standard Ebooks.".to_owned()).style(|style| style.font_size(12).color(palette().subtle_text)),
        store_error(error),
        connect,
    ))
    .style(|style| style.width(480).padding(18).gap(10).border(1).border_color(palette().border_color).border_radius(10).background(palette().white_surface))
}

fn catalog_content(
    signals: Signals, endpoint: OpdsEndpoint, current: OpdsCatalog, catalog: RwSignal<Option<OpdsCatalog>>, search: RwSignal<Option<OpdsSearch>>, levels: RwSignal<Vec<(String, String)>>, username: RwSignal<String>,
    password: RwSignal<String>, query: RwSignal<String>, loading: RwSignal<bool>, error: RwSignal<Option<String>>, imports: RwSignal<HashMap<String, String>>,
) -> impl View {
    let endpoint_id = endpoint.id;
    let breadcrumb_core = signals.app_core.clone();
    let breadcrumb = stack_from_iter(levels.get().into_iter().enumerate().map(move |(index, (title, _))| {
        let caption = if index == 0 { title } else { format!("/ {title}") };
        secondary_button(caption).on_click({
            let core = breadcrumb_core.clone();
            move |_| {
                if let Some((_, url)) = levels.get_untracked().get(index).cloned() {
                    request_catalog(core.clone(), endpoint_id, Some(url), credentials_from_signals(endpoint_id, username, password).ok().flatten(), CatalogTransition::Replace(index), catalog, search, levels, loading, error);
                }
                EventPropagation::Stop
            }
        })
    }))
    .style(|style| style.width_full().gap(6).flex_row().flex_wrap(FlexWrap::Wrap));

    let search_row: Box<dyn View> = match search.get() {
        Some(search_definition) => {
            let search_core = signals.app_core.clone();
            let search_button = secondary_button("Search").on_click(move |_| {
                let text = query.get_untracked().trim().to_owned();
                if !text.is_empty() {
                    request_search(search_core.clone(), search_definition.clone(), text.clone(), credentials_from_signals(endpoint_id, username, password).ok().flatten(), CatalogTransition::Search(text), catalog, levels, loading, error);
                }
                EventPropagation::Stop
            });
            Box::new(h_stack((text_input(query).style(|style| style.width(420)), search_button)).style(|style| style.gap(8).items_center()))
        }
        None => Box::new(label(|| "")),
    };

    let mut links = current.navigation.clone();
    links.extend(current.groups.iter().flat_map(|group| group.navigation.clone()));
    let folder_core = signals.app_core.clone();
    let folders = stack_from_iter(links.into_iter().map(move |link| {
        let title = link.title.clone();
        let link_url = link.url;
        secondary_button(format!("▰  {title}  ›")).on_click({
            let core = folder_core.clone();
            move |_| {
                request_catalog(core.clone(), endpoint_id, Some(link_url.clone()), credentials_from_signals(endpoint_id, username, password).ok().flatten(), CatalogTransition::Push(title.clone()), catalog, search, levels, loading, error);
                EventPropagation::Stop
            }
        })
    }))
    .style(|style| style.width_full().gap(8).flex_row().flex_wrap(FlexWrap::Wrap));

    let mut publications = current.publications.clone();
    publications.extend(current.groups.iter().flat_map(|group| group.publications.clone()));
    let publication_count = publications.len();
    let no_navigation = links_are_empty(&current);
    let signals_for_books = signals.clone();
    let books = stack_from_iter(publications.into_iter().map(move |publication| publication_card(publication, endpoint_id, signals_for_books.clone(), username, password, imports, error)))
        .style(|style| style.width_full().gap(14).flex_row().flex_wrap(FlexWrap::Wrap));
    let empty = inline_message("This catalog page has no EPUB books or folders.").style(move |style| if publication_count == 0 && no_navigation { style } else { style.hide() });

    let previous = current.pagination.previous.clone();
    let next = current.pagination.next.clone();
    let has_previous = previous.is_some();
    let has_next = next.is_some();
    let previous_core = signals.app_core.clone();
    let previous_button = secondary_button("← Previous")
        .on_click(move |_| {
            if let Some(url) = previous.clone() {
                request_catalog(
                    previous_core.clone(),
                    endpoint_id,
                    Some(url),
                    credentials_from_signals(endpoint_id, username, password).ok().flatten(),
                    CatalogTransition::Push("Previous page".to_owned()),
                    catalog,
                    search,
                    levels,
                    loading,
                    error,
                );
            }
            EventPropagation::Stop
        })
        .style(move |style| if has_previous { style } else { style.hide() });
    let next_core = signals.app_core.clone();
    let next_button = secondary_button("Next →")
        .on_click(move |_| {
            if let Some(url) = next.clone() {
                request_catalog(next_core.clone(), endpoint_id, Some(url), credentials_from_signals(endpoint_id, username, password).ok().flatten(), CatalogTransition::Push("Next page".to_owned()), catalog, search, levels, loading, error);
            }
            EventPropagation::Stop
        })
        .style(move |style| if has_next { style } else { style.hide() });

    books_scroll(
        v_stack((
            breadcrumb,
            search_row,
            store_error(error),
            inline_message("Loading catalog…").style(move |style| if loading.get() { style } else { style.hide() }),
            folders,
            books,
            empty,
            h_stack((previous_button, next_button)).style(|style| style.gap(8)),
        ))
        .style(|style| style.width_full().gap(16)),
    )
}

fn links_are_empty(catalog: &OpdsCatalog) -> bool {
    catalog.navigation.is_empty() && catalog.groups.iter().all(|group| group.navigation.is_empty())
}

fn publication_card(
    publication: OpdsPublication, endpoint: OpdsEndpointId, signals: Signals, username: RwSignal<String>, password: RwSignal<String>, imports: RwSignal<HashMap<String, String>>, error: RwSignal<Option<String>>,
) -> impl View {
    let publication_id = publication.id.clone();
    let title = publication.title.clone();
    let author = if publication.authors.is_empty() { "Unknown author".to_owned() } else { publication.authors.join(", ") };
    let acquisition = publication.free_epub.clone();
    let action: Box<dyn View> = match acquisition {
        Some(acquisition) => {
            let button_id = publication_id.clone();
            Box::new(secondary_button("Add to library").on_click(move |_| {
                if signals.active_library_id.get_untracked().is_none() {
                    error.set(Some("Create or select a library before adding a book".to_owned()));
                    return EventPropagation::Stop;
                }
                request_import(signals.clone(), button_id.clone(), acquisition.clone(), credentials_from_signals(endpoint, username, password).ok().flatten(), imports, error);
                EventPropagation::Stop
            }))
        }
        None => Box::new(label(|| "No free EPUB".to_owned()).style(|style| style.font_size(12).color(palette().subtle_text))),
    };
    let status_id = publication_id.clone();
    let status = label(move || imports.get().get(&status_id).cloned().unwrap_or_default()).style(|style| style.font_size(12).color(palette().accent));
    v_stack((
        label(move || title.clone()).style(|style| style.width(260).font_size(16).font_bold().text_ellipsis()),
        label(move || author.clone()).style(|style| style.width(260).font_size(12).color(palette().accent).text_ellipsis()),
        action,
        status,
    ))
    .style(|style| style.width(280).min_height(132).padding(14).gap(8).border(1).border_color(palette().border_color).border_radius(10).background(palette().white_surface))
}

fn credentials_from_signals(endpoint: OpdsEndpointId, username: RwSignal<String>, password: RwSignal<String>) -> Result<Option<OpdsCredentials>, String> {
    let username = username.get_untracked().trim().to_owned();
    if username.is_empty() {
        return Ok(None);
    }
    OpdsCredentials::for_endpoint(endpoint, username, password.get_untracked()).map(Some).map_err(|error| error.to_string())
}

fn request_catalog(
    core: AppCore, endpoint: OpdsEndpointId, url: Option<String>, credentials: Option<OpdsCredentials>, transition: CatalogTransition, catalog: RwSignal<Option<OpdsCatalog>>, search: RwSignal<Option<OpdsSearch>>,
    levels: RwSignal<Vec<(String, String)>>, loading: RwSignal<bool>, error: RwSignal<Option<String>>,
) {
    if loading.get_untracked() {
        return;
    }
    loading.set(true);
    error.set(None);
    let send = create_ext_action(Scope::new(), move |result: Result<OpdsCatalog, String>| {
        loading.set(false);
        match result {
            Ok(loaded) => {
                match transition {
                    CatalogTransition::Root => {
                        levels.set(vec![(loaded.title.clone(), loaded.source_url.clone())]);
                        search.set(loaded.search.clone());
                    }
                    CatalogTransition::Push(title) => levels.update(|path| path.push((title, loaded.source_url.clone()))),
                    CatalogTransition::Replace(index) => levels.update(|path| path.truncate(index + 1)),
                    CatalogTransition::Search(query) => levels.update(|path| {
                        path.truncate(1);
                        path.push((format!("Search: {query}"), loaded.source_url.clone()));
                    }),
                }
                catalog.set(Some(loaded));
            }
            Err(message) => error.set(Some(message)),
        }
    });
    std::thread::spawn(move || {
        let result = match url {
            Some(url) => core.opds_catalog_blocking(&url, credentials.as_ref()),
            None => core.opds_endpoint_catalog_blocking(endpoint, credentials.as_ref()),
        }
        .map_err(|error| error.to_string());
        send(result);
    });
}

fn request_search(
    core: AppCore, search: OpdsSearch, query: String, credentials: Option<OpdsCredentials>, transition: CatalogTransition, catalog: RwSignal<Option<OpdsCatalog>>, levels: RwSignal<Vec<(String, String)>>, loading: RwSignal<bool>,
    error: RwSignal<Option<String>>,
) {
    if loading.get_untracked() {
        return;
    }
    loading.set(true);
    error.set(None);
    let send = create_ext_action(Scope::new(), move |result: Result<OpdsCatalog, String>| {
        loading.set(false);
        match result {
            Ok(loaded) => {
                if let CatalogTransition::Search(query) = transition {
                    levels.update(|path| {
                        path.truncate(1);
                        path.push((format!("Search: {query}"), loaded.source_url.clone()));
                    });
                }
                catalog.set(Some(loaded));
            }
            Err(message) => error.set(Some(message)),
        }
    });
    std::thread::spawn(move || send(core.search_opds_blocking(&search, &query, credentials.as_ref()).map_err(|error| error.to_string())));
}

fn request_import(signals: Signals, publication_id: String, acquisition: OpdsEpubAcquisition, credentials: Option<OpdsCredentials>, imports: RwSignal<HashMap<String, String>>, error: RwSignal<Option<String>>) {
    if imports.get_untracked().get(&publication_id).is_some_and(|state| state == "Adding…" || state == "Added") {
        return;
    }
    imports.update(|states| {
        states.insert(publication_id.clone(), "Adding…".to_owned());
    });
    let core = signals.app_core.clone();
    let id_for_result = publication_id.clone();
    let send = create_ext_action(Scope::new(), move |result: Result<(), String>| match result {
        Ok(()) => {
            imports.update(|states| {
                states.insert(id_for_result, "Added".to_owned());
            });
            signals.library_refresh_tick.update(|tick| *tick = tick.saturating_add(1));
        }
        Err(message) => {
            imports.update(|states| {
                states.remove(&id_for_result);
            });
            error.set(Some(message));
        }
    });
    std::thread::spawn(move || send(core.import_opds_epub_blocking("__root__", &acquisition, credentials.as_ref()).map(|_| ()).map_err(|error| error.to_string())));
}

fn store_error(error: RwSignal<Option<String>>) -> impl View {
    label(move || error.get().unwrap_or_default()).style(move |style| if error.get().is_some() { style.padding(10).border_radius(8).color(palette().danger_text).background(palette().danger_bg) } else { style.hide() })
}
