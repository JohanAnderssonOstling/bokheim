use crate::components::{create_book_title_label, style_book_cover};
use crate::page_navigation::{Page, ReaderState, Signals};
use crossbeam::channel::{unbounded, Receiver};
use epub_svelte_lib::scanner::{DownloadState, LibraryEvent};
use epub_svelte_lib::BookID;
use floem::event::EventPropagation;
use floem::peniko::Color;
use floem::prelude::{
    button, container, h_stack, label, v_stack, Decorators, ScrollExt,
    SignalGet, Stack,
};
use floem::style::FlexWrap;
use floem::views::svg;
use floem::views::{img, stack, stack_from_iter};
use floem::{IntoView, View};
use std::f64::consts::PI;
use std::path::PathBuf;
use std::time::Instant;
use epub_svelte_lib::library::{Book, Dir};

#[derive(Clone, Eq, Hash, PartialEq)]
pub struct LibraryState {
    pub library_id: String,
    pub folder_id: String,
}

impl LibraryState {
    pub fn library_view(&self, signals: Signals) -> impl View {
        let main_view = self.dir_view(signals.clone())
            .scroll()
            .style(move |s| s.width_full().height_full().background(Color::WHITE));
        v_stack((main_view,)).style(|s| s.width_full().flex_grow(1.0))
    }
    pub fn dir_view(&self, signals: Signals) -> impl View {
        let library_handles = signals.library_handles.get_untracked();
        let library = library_handles.get(&self.library_id).unwrap();
        let now = Instant::now();
        let dir_content = library.get_dir_contents(&self.folder_id);

        let image_loading_time = now.elapsed();
        let now = Instant::now();

        let book_stack = stack_from_iter(dir_content.books.into_iter().map(|book_cover| {
            self.create_book_cover(book_cover, signals.clone())
        }))
            .style(move |s| {
                s.gap(40)
                    .flex_row()
                    .flex_wrap(FlexWrap::Wrap)
                    .flex_grow(1.0)
            });

        let book_stack_id = book_stack.id();
        let image_decoding_time = now.elapsed();

        let dir_stack = stack_from_iter(
            dir_content.dirs.into_iter()
                .map(|dir| create_dir_cover(dir, signals.clone())),
        )
            .style(move |s| s.gap(20).flex_row().flex_wrap(FlexWrap::Wrap).size_full());

        let diagnostic_stack = h_stack((
            label(move || format!("Image loading: {} ms ", image_loading_time.as_millis())),
            label(move || format!("Image decoding: {} ms ", image_decoding_time.as_millis())),
            button(label(move || "Diagnostic")).on_click(move |s| {
                book_stack_id.inspect();
                EventPropagation::Continue
            }),
        ));

        v_stack((diagnostic_stack, book_stack, dir_stack))
            .style(move |s| s.margin(20))
            .scroll()
            .into_view()

    }
    pub fn create_book_cover(&self, book: Book, signals: Signals, ) -> Stack {
        let title_label = create_book_title_label(book.title.clone());
        //let cover = cover.unwrap_or_else(|| Vec::new());
        let library_id = self.library_id.clone();
        let cover = match book.cover {
            None => {container(label(move || "Placeholder".to_string()))}
            Some(book_cover) => {
                let cover_image = img(move || book_cover.clone())
                    .on_click(move |s| {
                        let reader_state = ReaderState {
                            book_id: book.id.clone(),
                            library_id: library_id.clone(),
                            book_title: book.title.clone(),
                        };
                        signals.navigate(Page::Reader(reader_state));
                        EventPropagation::Continue
                    });
                let cover_image = style_book_cover(cover_image);
                container(cover_image)
            }
        };


        //let cover_stack = stack((cover_image,))
        //  .style(|s| s.width(200).height(320).items_center().justify_center());
        v_stack((cover, title_label))
    }
}


fn throttle_library_events(rx: Receiver<LibraryEvent>) -> Receiver<LibraryEvent> {
    let (tx, out_rx) = unbounded::<LibraryEvent>();
    std::thread::spawn(move || loop {
        if let Ok(event) = rx.recv() {
            let _ = tx.send(event);
        } else {
            break;
        }
    });
    out_rx
}





fn create_dir_cover(dir: Dir, signals: Signals) -> impl View {
    let background = Color::WHITE;
    let box_shadow = Color::rgba8(0, 0, 0, 25);
    let name_label = label(move || format!("📁 {}", dir.name))
        .on_click(move |s| {
            //signals.navigate(Page::Library);
            EventPropagation::Continue
        })
        .style(move |s| {
            s.border_radius(8.0)
                .font_size(16)
                .padding(15)
                .background(background)
                .box_shadow_blur(2)
                .box_shadow_color(box_shadow)
                .box_shadow_spread(1)
        });
    name_label.into_view()
}

fn download_indicator_view(state: DownloadState) -> Box<dyn View> {
    match state {
        DownloadState::Downloaded => Box::new(container(label(|| "")).style(|s| {
            s.size(18, 18)
                .border_radius(999)
                .background(Color::rgb8(16, 185, 129))
        })),
        DownloadState::NotDownloaded => Box::new(container(label(|| "")).style(|s| {
            s.size(18, 18)
                .border_radius(999)
                .background(Color::rgb8(239, 68, 68))
        })),
        DownloadState::Downloading(progress) => {
            let svg_markup = download_progress_svg(progress);
            Box::new(stack((
                container(label(|| "")).style(|s| {
                    s.size(26, 26)
                        .border_radius(999)
                        .background(Color::rgba8(249, 115, 22, 30))
                }),
                svg(svg_markup).style(|s| s.size(26, 26)),
            )))
        }
    }
}

fn download_progress_svg(progress: f32) -> String {
    let progress = progress.clamp(0.0, 1.0) as f64;
    let radius = 10.0;
    let circumference = 2.0 * PI * radius;
    let dash = circumference * progress;
    let dasharray = format!("{:.2} {:.2}", dash, circumference);
    format!(
        "<svg viewBox=\"0 0 32 32\" width=\"32\" height=\"32\">
            <circle cx=\"16\" cy=\"16\" r=\"14\" stroke=\"#f97316\" stroke-width=\"4\" fill=\"none\" opacity=\"0.25\" />
            <circle cx=\"16\" cy=\"16\" r=\"14\" stroke=\"#f97316\" stroke-width=\"4\" fill=\"none\" stroke-dasharray=\"{dasharray}\" stroke-dashoffset=\"{circumference}\" stroke-linecap=\"round\" transform=\"rotate(-90 16 16)\" />
        </svg>"
    )
}




