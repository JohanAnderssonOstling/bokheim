use bokheim_ui_components as components;
use gpui::prelude::*;
use gpui::{AnyElement, Entity};

use crate::BrowserView;

impl BrowserView {
    pub(crate) fn render_folder_breadcrumb(&self, entity: Entity<Self>) -> impl IntoElement {
        let root_target = entity.clone();
        let mut breadcrumb = components::folder_breadcrumb().child(components::folder_breadcrumb_item("Library", self.folder_path.is_empty()).on_click(move |_, _, cx| {
            root_target.update(cx, |this, cx| {
                this.set_folder_path(Vec::new(), cx);
            });
        }));

        for (index, crumb) in self.folder_path.iter().enumerate() {
            let target = entity.clone();
            let path = self.folder_path[..=index].to_vec();
            let current = index + 1 == self.folder_path.len();
            breadcrumb = breadcrumb.child(components::folder_breadcrumb_item(crumb.name.clone(), current).on_click(move |_, _, cx| {
                target.update(cx, |this, cx| {
                    this.set_folder_path(path.clone(), cx);
                });
            }));
        }
        breadcrumb
    }

    pub(crate) fn render_folders(&self, entity: Entity<Self>, columns: usize, theme: components::BrowserTheme) -> AnyElement {
        let mut dashboard = components::folder_dashboard();
        if !self.folder_directories.is_empty() {
            let mut folders = components::folder_buttons();
            for directory in self.folder_directories.iter() {
                let target = entity.clone();
                let mut path = self.folder_path.clone();
                path.push(directory.clone());
                let label = format!("{} ({})", directory.name, directory.book_count);
                folders = folders.child(components::folder_button(format!("folder-button-{}", directory.id), label, theme).on_click(move |_, _, cx| {
                    target.update(cx, |this, cx| {
                        this.set_folder_path(path.clone(), cx);
                    });
                }));
            }
            dashboard = dashboard.child(folders);
        }

        dashboard.child(self.render_book_grid(entity, columns, theme)).into_any_element()
    }
}
