const MAX_VALUE_BYTES: usize = 4096;

pub(crate) fn bounded(value: &str) -> Result<String, ()> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_VALUE_BYTES || value.chars().any(char::is_control) {
        return Err(());
    }
    Ok(value.to_owned())
}
