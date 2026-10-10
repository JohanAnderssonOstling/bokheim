//! Container-relative resource classification and path resolution.

pub(crate) fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

pub(crate) fn is_container_resource(uri: &str) -> bool {
    !uri.starts_with("//") && !uri.split('/').next().unwrap_or_default().contains(':')
}

pub fn resolve_resource_href(base: &str, href: &str) -> String {
    if !is_container_resource(href) {
        return href.to_owned();
    }
    let href = href.split('#').next().unwrap_or(href);
    let href = href.split('?').next().unwrap_or(href);
    if let Some(root_relative) = href.strip_prefix('/') {
        return normalize_path(root_relative);
    }
    let base_dir = base.rsplit_once('/').map(|x| x.0).unwrap_or("");
    let combined = if base_dir.is_empty() { href.to_string() } else { format!("{}/{}", base_dir.trim_end_matches('/'), href) };
    normalize_path(&combined)
}

pub(crate) fn local_resource_uri(base: &str, href: &str) -> std::io::Result<String> {
    if !is_container_resource(href) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "external book resources are unsupported"));
    }
    Ok(resolve_resource_href(base, href))
}
