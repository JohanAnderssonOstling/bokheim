use crate::page_navigation::Signals;
use epub_svelte_lib::home::{create_library, get_library_entries, LibraryEntry};
use floem::action::open_file;
use floem::event::{Event, EventPropagation};
use floem::file::{FileDialogOptions, FileInfo};
use floem::prelude::SignalUpdate;
use floem::reactive::WriteSignal;

pub fn add_library_on_click(set_libraries: WriteSignal<Vec<LibraryEntry>>) -> impl FnMut(&Event) -> EventPropagation {
    move |_| {
        let file_dialog_options = FileDialogOptions::new().select_directories().title("Select Folder");
        open_file(file_dialog_options, {
            let set_libraries = set_libraries.clone();
            move |file_info| {
                let Some(file_info) = file_info else { return };
                for new_library_path in file_info.path.iter() {
                    let name = new_library_path.file_name().and_then(|s| s.to_str()).unwrap_or("Library").to_string();
                    if let Err(err) = create_library(name, Some(new_library_path.clone())) {
                        eprintln!("failed to create library at {}: {}", new_library_path.display(), err);
                    }
                }
                let entries = get_library_entries();
                set_libraries.set(entries);
            }
        });

        EventPropagation::Continue
    }
}
fn display_files(file: FileInfo) -> String {
    let paths: Vec<&str> = file.path.iter().filter_map(|p| p.to_str()).collect();
    paths.join("\n")
}
