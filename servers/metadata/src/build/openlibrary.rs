use crate::{fmt, Connection, Deserialize, KeyReference, MetadataError, OpenFlags, Path, PathBuf, Pool, SqliteConnectionManager, MAX_DESCRIPTION_BYTES, QUERY_CONCURRENCY, READ_CACHE_KIB, READ_MMAP_BYTES};

pub(crate) fn open_description_pool(path: &Path) -> Result<Pool<SqliteConnectionManager>, MetadataError> {
    let manager = SqliteConnectionManager::file(path.canonicalize().map_err(error)?).with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).with_init(configure_read_only);
    Pool::builder().max_size(QUERY_CONCURRENCY).min_idle(Some(1)).build(manager).map_err(error)
}

pub(crate) fn configure_read_only(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.execute_batch(&format!("PRAGMA query_only=ON; PRAGMA cache_size=-{READ_CACHE_KIB}; PRAGMA mmap_size={READ_MMAP_BYTES};"))
}

pub(crate) fn open_library_id(value: &str, suffix: char) -> Option<i64> {
    let digits = value
        .strip_prefix(if suffix == 'M' {
            "/books/OL"
        } else if suffix == 'W' {
            "/works/OL"
        } else if suffix == 'A' {
            "/authors/OL"
        } else {
            return None;
        })
        .or_else(|| value.strip_prefix("OL"))?
        .strip_suffix(suffix)?;
    digits.parse().ok()
}

pub(crate) fn deserialize_string_array<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => vec![value],
        serde_json::Value::Array(values) => values.into_iter().filter_map(|value| value.as_str().map(ToOwned::to_owned)).collect(),
        _ => Vec::new(),
    })
}

pub(crate) fn deserialize_optional_string<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => Some(value),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

pub(crate) fn deserialize_optional_description<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(value) => Some(value),
        serde_json::Value::Object(mut value) => value.remove("value").and_then(|value| value.as_str().map(ToOwned::to_owned)),
        _ => None,
    })
}

pub(crate) fn valid_open_library_description(value: &str) -> Option<String> {
    valid_open_library_text(value, MAX_DESCRIPTION_BYTES)
}

pub(crate) fn valid_open_library_text(value: &str, maximum_bytes: usize) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum_bytes || value.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t')) {
        return None;
    }
    Some(value.to_owned())
}

pub(crate) fn deserialize_key_references<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<KeyReference>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    Ok(values.into_iter().filter_map(|value| value.get("key").and_then(serde_json::Value::as_str).map(|key| KeyReference { key: key.to_owned() })).collect())
}

pub(crate) fn deserialize_work_author_references<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<KeyReference>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let values = match value {
        serde_json::Value::Array(values) => values,
        value @ serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };
    Ok(values.into_iter().filter_map(|value| value.get("author").and_then(|author| author.get("key")).or_else(|| value.get("key")).and_then(serde_json::Value::as_str).map(|key| KeyReference { key: key.to_owned() })).collect())
}

pub(crate) fn building_path(output: &Path) -> PathBuf {
    let mut name = output.file_name().unwrap_or_default().to_os_string();
    name.push(".building");
    output.with_file_name(name)
}

pub(crate) fn dump_date(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    name.as_bytes().windows(10).find_map(|window| {
        let value = std::str::from_utf8(window).ok()?;
        (value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-') && value.chars().enumerate().all(|(index, character)| matches!(index, 4 | 7) || character.is_ascii_digit())).then(|| value.to_owned())
    })
}

pub(crate) fn valid_dump_date(value: &str) -> bool {
    value.len() == 10 && value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-') && value.chars().enumerate().all(|(index, character)| matches!(index, 4 | 7) || character.is_ascii_digit())
}

pub(crate) fn error(error: impl fmt::Display) -> MetadataError {
    MetadataError(error.to_string())
}

/// Internal authority keys reserve negatives for Wikidata QIDs; Open Library
/// identifiers remain positive and are never fabricated for other sources.
pub(crate) fn authority_edition_id(value: &str) -> Option<i64> {
    value.strip_prefix("wikidata:Q").and_then(|q| q.parse::<i64>().ok()).filter(|q| *q > 0).map(|q| -q).or_else(|| open_library_id(value, 'M'))
}
pub(crate) fn authority_edition_key(id: i64) -> String {
    if id < 0 {
        format!("wikidata:Q{}", -id)
    } else {
        format!("OL{id}M")
    }
}

/// Optional dump supplements are represented by empty temporary tables once
/// per builder connection. Resolution always sees the current input schema.
pub(crate) fn open_builder_pool(path: &Path) -> Result<Pool<SqliteConnectionManager>, MetadataError> {
    let manager = SqliteConnectionManager::file(path.canonicalize().map_err(error)?).with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).with_init(|c| {
        c.set_prepared_statement_cache_capacity(64);
        for (name, definition) in [
            ("edition_description", "edition_id INTEGER PRIMARY KEY,description TEXT NOT NULL"),
            ("isbn_lcc", "isbn13 INTEGER,notation TEXT,lc_record_id TEXT"),
            ("lc_record", "lc_record_id TEXT PRIMARY KEY,metadata TEXT NOT NULL"),
            ("lc_record_isbn", "isbn13 INTEGER,lc_record_id TEXT"),
            ("lc_record_lcc", "lc_record_id TEXT,notation TEXT"),
            ("lc_title", "title_key TEXT,lc_record_id TEXT"),
        ] {
            let present: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [name], |r| r.get(0))?;
            if !present {
                c.execute_batch(&format!("CREATE TEMP TABLE {name}({definition});"))?;
            }
        }
        configure_read_only(c)
    });
    Pool::builder().max_size(QUERY_CONCURRENCY).min_idle(Some(1)).build(manager).map_err(error)
}
