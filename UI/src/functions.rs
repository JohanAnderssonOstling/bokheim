use crate::library_page::LibraryState;
use crate::page_navigation::Signals;
use epub_svelte_lib::home::{create_library, get_library_entries};
use floem::action::{exec_after, open_file};
use floem::event::{Event, EventPropagation};
use floem::file::FileDialogOptions;
use floem::prelude::{SignalGet, SignalUpdate, SignalWith};
use std::collections::HashMap;
use std::time::Duration;

pub fn add_library_on_click(signals: Signals) -> impl FnMut(&Event) -> EventPropagation {
    move |_| {
        let file_dialog_options = FileDialogOptions::new().select_directories().title("Select Folder");
        open_file(file_dialog_options, {
            let signals = signals.clone();
            move |file_info| {
                let Some(file_info) = file_info else { return };
                for new_library_path in file_info.path.iter() {
                    let name = new_library_path.file_name().and_then(|s| s.to_str()).unwrap_or("Library").to_string();
                    if let Err(err) = create_library(name, Some(new_library_path.clone())) {
                        eprintln!("failed to create library at {}: {}", new_library_path.display(), err);
                    }
                }
                let entries = get_library_entries();
                let signals = signals.clone();
                exec_after(Duration::from_millis(0), move |_| {
                    signals.libraries.update(|libs| {
                        let mut existing_by_id: HashMap<String, LibraryState> =
                            libs.iter().cloned().map(|s| (s.meta.id.clone(), s)).collect();
                        let next = entries
                            .iter()
                            .map(|entry| {
                                existing_by_id.remove(&entry.library_id).unwrap_or_else(|| {
                                    LibraryState::new(entry.library_id.clone(), entry.library_name.clone(), entry.library_path.clone())
                                })
                            })
                            .collect::<Vec<_>>();
                        *libs = next;
                    });
                    if signals.active_library_id.get_untracked().is_none() {
                        let first_id = signals.libraries.with_untracked(|v| v.first().map(|s| s.meta.id.clone()));
                        signals.active_library_id.set(first_id);
                    }
                });
            }
        });

        EventPropagation::Continue
    }
}
