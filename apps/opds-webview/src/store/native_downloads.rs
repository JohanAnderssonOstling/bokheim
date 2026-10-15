//! Forward native download percentages; unknown sizes stay indeterminate.
use super::Action;
use gtk::glib::prelude::*;
use std::{cell::Cell, path::PathBuf, rc::Rc, sync::mpsc::Sender};
use url::Url;
use webkit2gtk::{Download, DownloadExt, URIResponseExt, WebContextExt, WebViewExt, URIRequestExt};
use wry::{WebView, WebViewExtUnix};

pub(super) fn observe(page: &WebView, sender: Sender<Action>) {
    let native = page.webview();
    let Some(context) = native.context() else { return };
    let page = native.downgrade();
    context.connect_download_started(move |_, download| {
        if page.upgrade().is_none_or(|page| download.web_view().as_ref() != Some(&page)) { return; }
        let watched = download.clone();
        let deferred = sender.clone();
        let finished = Rc::new(Cell::new(false));
        download.connect_finished({ let finished = finished.clone(); move |_| finished.set(true) });
        download.connect_failed({ let finished = finished.clone(); move |_, _| finished.set(true) });
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            if finished.get() { return gtk::glib::ControlFlow::Break; }
            if !client_platform_runtime::transfers::allowed() {
                if let Some(url) = watched.request().and_then(|request| request.uri()) {
                    let _ = deferred.send(Action::DownloadDeferred(url.to_string()));
                }
                watched.cancel();
                return gtk::glib::ControlFlow::Break;
            }
            if watched.estimated_progress() >= 1.0 { gtk::glib::ControlFlow::Break } else { gtk::glib::ControlFlow::Continue }
        });
        let previous = Rc::new(Cell::new(None));
        download.connect_created_destination({
            let previous = previous.clone();
            let sender = sender.clone();
            move |download, _| send_progress(download, &previous, &sender)
        });
        download.connect_estimated_progress_notify({
            let sender = sender.clone();
            move |download| send_progress(download, &previous, &sender)
        });
    });
}

fn send_progress(download: &Download, previous: &Cell<Option<u8>>, sender: &Sender<Action>) {
    if download.response().is_none_or(|response| response.content_length() == 0) { return; }
    let percent = (download.estimated_progress().clamp(0.0, 1.0) * 100.0).floor() as u8;
    if previous.get() == Some(percent) { return; }
    let Some(destination) = download.destination() else { return };
    let path = Url::parse(&destination).ok().and_then(|url| url.to_file_path().ok())
        .unwrap_or_else(|| PathBuf::from(destination.as_str()));
    previous.set(Some(percent));
    let _ = sender.send(Action::DownloadProgress { path, percent });
}
