//! The native browser's start document, shared by desktop and Android.
use gpui_component::Colorize;
use crate::BrowserTheme;

/// Each shortcut shows its icon and title. Its URL is only a link target.
pub fn browser_start_page<'a>(pages: impl IntoIterator<Item = (&'a str, &'a str, Option<&'a str>)>, theme: BrowserTheme) -> String {
    let mut shortcuts = String::new();
    for (url, title, icon) in pages.into_iter().take(8) {
        let letter = title.chars().find(|c| c.is_alphanumeric()).unwrap_or('?').to_uppercase().to_string();
        let image = icon.map(|url| format!(
            r#"<img src="{}" alt="" referrerpolicy="no-referrer" onload="this.previousElementSibling.hidden=true" onerror="this.hidden=true">"#, escape(url)
        )).unwrap_or_default();
        shortcuts.push_str(&format!(
            r#"<a class="shortcut" href="{}"><span class="icon"><span>{}</span>{}</span><span class="title">{}</span></a>"#,
            escape(url), escape(&letter), image, escape(title),
        ));
    }
    let section = if shortcuts.is_empty() { String::new() } else {
        format!("<main><h1>Most visited</h1><nav aria-label=\"Most visited\">{shortcuts}</nav></main>")
    };
    format!(r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Start page</title><style>
:root {{ background: {background}; color: {text}; font: 16px '{font}', sans-serif; }}
* {{ box-sizing: border-box; }}
body {{ margin: 0; }}
main {{ max-width: 720px; margin: 64px auto; padding: 0 24px; }}
h1 {{ margin: 0 0 24px; font-size: 18px; font-weight: 500; }}
nav {{ display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px; }}
.shortcut {{ display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 16px 8px;
    border-radius: 8px; color: inherit; text-decoration: none; text-align: center; min-width: 0; }}
.shortcut:hover, .shortcut:active {{ background: {hover}; }}
.shortcut:focus-visible {{ outline: 2px solid {accent}; outline-offset: 2px; }}
.icon {{ position: relative; display: grid; place-items: center; width: 48px; height: 48px;
    background: {raised}; border-radius: 10px; color: {muted}; font-size: 24px; }}
img {{ position: absolute; width: 100%; height: 100%; padding: 6px; object-fit: contain; border-radius: inherit; }}
[hidden] {{ display: none; }}
.title {{ display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical;
    overflow: hidden; overflow-wrap: anywhere; font-size: 14px; line-height: 1.4; }}
@media (max-width: 520px) {{ main {{ margin-top: 32px; }} nav {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }} }}
</style></head><body>{section}</body></html>"#,
        background = theme.page_bg.to_hex(), text = theme.text.to_hex(), font = theme.ui_font_family.family_name(),
        hover = theme.hover.to_hex(), accent = theme.accent.to_hex(), raised = theme.raised_bg.to_hex(), muted = theme.text_muted.to_hex(),
    )
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_page_metadata_cannot_inject_markup_and_addresses_are_only_link_targets() {
        let url = "https://example.com/books?q=one&lang=en";
        let html = browser_start_page([(url, "Books <script>alert('title')</script>", Some("https://example.com/icon.png\" onerror=\"alert('icon')"))], BrowserTheme::default());
        assert!(!html.contains("<script>"));
        assert!(html.contains("Books &lt;script&gt;alert(&#39;title&#39;)&lt;/script&gt;"));
        assert!(html.contains("icon.png&quot; onerror=&quot;alert(&#39;icon&#39;)"));
        assert_eq!(html.matches("https://example.com/books").count(), 1);
        assert!(html.contains("href=\"https://example.com/books?q=one&amp;lang=en\""));
    }
}
