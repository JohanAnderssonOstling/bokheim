//! Chapter points from a CUE sheet. Audio remains in the discovered MP3 tracks.

use book_model::{AudiobookChapter, AudiobookTrack};

struct Point {
    file: String,
    title: Option<String>,
    offset_ms: u64,
}

fn argument(input: &str) -> Option<&str> {
    let input = input.trim();
    if let Some(quoted) = input.strip_prefix('"') {
        Some(quoted.split_once('"')?.0)
    } else {
        input.split_whitespace().next()
    }
}

pub(super) fn chapters(input: &str, tracks: &[AudiobookTrack]) -> Result<Vec<AudiobookChapter>, String> {
    let mut file = None::<String>;
    let mut current = None::<(Option<String>, Option<u64>)>;
    let mut points = Vec::<Point>::new();
    for line in input.lines() {
        let line = line.trim();
        let Some((command, rest)) = line.split_once(char::is_whitespace) else { continue };
        let rest = rest.trim();
        match command.to_ascii_uppercase().as_str() {
            "FILE" => {
                if let Some((title, offset_ms)) = current.take() {
                    points.push(Point { file: file.clone().ok_or("CUE track has no FILE")?, title, offset_ms: offset_ms.ok_or("CUE track has no INDEX 01")? });
                }
                let name = argument(rest).ok_or("CUE FILE has no filename")?;
                if !name.to_ascii_lowercase().ends_with(".mp3") { return Err("CUE references a non-MP3 file".into()); }
                file = Some(name.to_owned());
            }
            "TRACK" => {
                if let Some((title, offset_ms)) = current.take() {
                    points.push(Point { file: file.clone().ok_or("CUE track has no FILE")?, title, offset_ms: offset_ms.ok_or("CUE track has no INDEX 01")? });
                }
                if file.is_none() || !rest.split_whitespace().nth(1).is_some_and(|kind| kind.eq_ignore_ascii_case("AUDIO")) { return Err("CUE track is not audio".into()); }
                current = Some((None, None));
            }
            "TITLE" if current.is_some() => {
                let title = if rest.starts_with('"') { argument(rest).ok_or("CUE TITLE is empty")? } else { rest };
                current.as_mut().unwrap().0 = Some(title.to_owned());
            }
            "INDEX" if current.is_some() => {
                let mut fields = rest.split_whitespace();
                if fields.next() == Some("01") {
                    // mm:ss:ff, with 75 frames to the second.
                    let mut parts = fields.next().ok_or("missing CUE INDEX 01 position")?.split(':').map(|part| part.parse::<u64>().ok());
                    let (Some(Some(minutes)), Some(Some(seconds)), Some(Some(frames)), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return Err("invalid CUE INDEX 01".into()) };
                    if seconds >= 60 || frames >= 75 { return Err("invalid CUE INDEX 01".into()); }
                    let offset_ms = minutes.checked_mul(60).and_then(|value| value.checked_add(seconds)).and_then(|value| value.checked_mul(1000)).and_then(|value| value.checked_add((frames * 1000 + 37) / 75)).ok_or("invalid CUE INDEX 01")?;
                    current.as_mut().unwrap().1 = Some(offset_ms);
                }
            }
            _ => {},
        }
    }
    if let Some((title, offset_ms)) = current.take() {
        points.push(Point { file: file.ok_or("CUE track has no FILE")?, title, offset_ms: offset_ms.ok_or("CUE track has no INDEX 01")? });
    }
    if points.is_empty() { return Err("CUE has no INDEX 01 chapters".into()); }
    let mut starts = Vec::with_capacity(points.len());
    for (index, point) in points.into_iter().enumerate() {
        let file = point.file.replace('\\', "/");
        if file.starts_with('/') || file.split('/').any(|part| part.is_empty() || part == "." || part == ".." || part.contains(':')) { return Err("unsafe CUE filename".into()); }
        let exact = tracks.iter().filter(|track| track.name.eq_ignore_ascii_case(&file)).collect::<Vec<_>>();
        let matches = if exact.is_empty() {
            tracks.iter().filter(|track| track.name.rsplit('/').next().is_some_and(|name| name.eq_ignore_ascii_case(&file))).collect::<Vec<_>>()
        } else { exact };
        if matches.len() != 1 { return Err(format!("CUE filename does not identify one MP3: {file}")); }
        let track = matches[0];
        let start = track.start_ms.checked_add(point.offset_ms).ok_or("CUE position overflow")?;
        if start >= track.end_ms || starts.last().is_some_and(|(_, previous): &(String, u64)| *previous >= start) {
            return Err("CUE chapter positions are outside the audio or out of order".into());
        }
        starts.push((point.title.filter(|title| !title.trim().is_empty()).unwrap_or_else(|| format!("Chapter {}", index + 1)), start));
    }
    let duration = tracks.last().ok_or("audiobook has no MP3 tracks")?.end_ms;
    if starts.first().is_some_and(|(_, start)| *start > 0) {
        starts.insert(0, ("Opening".into(), 0));
    }
    Ok(starts.iter().enumerate().map(|(index, (title, start_ms))| AudiobookChapter {
        title: title.clone(), start_ms: *start_ms, end_ms: starts.get(index + 1).map_or(duration, |(_, next)| *next),
    }).collect())
}
