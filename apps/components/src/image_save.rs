//! Save the full image bytes, independent of the viewer's zoom and fit.
use std::{path::PathBuf, sync::Arc};

use gpui::{App, FileDialogFilter, Image, Window};
use gpui_component::{WindowExt, notification::Notification};
use ui_text::text;

pub fn image_saving_available() -> bool {
    !cfg!(any(target_family = "wasm", target_os = "android", target_os = "ios", feature = "mobile", feature = "kobo"))
}

pub fn save_image(image: Arc<Image>, window: &mut Window, cx: &mut App) {
    let extension = image.format.extension();
    let directory = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let name = format!("image.{extension}");
    let destination = cx.prompt_for_new_path_with_filters(&directory, Some(&name), vec![FileDialogFilter { name: extension.to_uppercase(), extensions: vec![extension.into()] }]);
    window.spawn(cx, async move |cx| {
        let result = async {
            let Some(path) = destination.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())? else { return Ok(false) };
            cx.background_executor().spawn(async move { std::fs::write(path, &image.bytes).map_err(|error| error.to_string()) }).await?;
            Ok::<_, String>(true)
        }.await;
        let _ = cx.update(|window, cx| match result {
            Ok(false) => {},
            Ok(true) => window.push_notification(Notification::success(text!(common::image_saved)), cx),
            Err(error) => {
                log::warn!("could not save image: {error}");
                window.push_notification(Notification::error(text!(common::could_not_save_image)), cx);
            }
        });
    }).detach();
}
