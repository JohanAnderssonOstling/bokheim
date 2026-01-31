use floem::action::open_file;
use floem::event::{Event, EventPropagation};
use floem::file::{FileDialogOptions, FileInfo};
use floem::prelude::SignalUpdate;
use floem::reactive::WriteSignal;
use epub_svelte_lib::home::{create_library, get_library_entries, LibraryEntry};
use crate::page_navigation::Signals;

pub fn add_library_on_click(set_libraries: WriteSignal<Vec<LibraryEntry>>, ) -> impl FnMut(&Event) -> EventPropagation {
    move |_| {
        let file_dialog_options = FileDialogOptions::new().select_directories().title("Select Folder");
        open_file(file_dialog_options,
            {
                let set_libraries = set_libraries.clone();
                move |file_info| {
                    let Some(file_info) = file_info else { return };
                    for  new_library_path in file_info.path.iter() {
                        create_library(new_library_path);
                    }
                    let entries = get_library_entries();
                    set_libraries.set(entries);
                }
            },
        );

        EventPropagation::Continue
    }
}
fn display_files(file: FileInfo) -> String {
    let paths: Vec<&str> = file.path.iter().filter_map(|p| p.to_str()).collect();
    paths.join("\n")
}