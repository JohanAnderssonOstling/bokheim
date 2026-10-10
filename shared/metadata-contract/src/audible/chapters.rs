//! AudiobookChapter plans never alter audio and never scale playback timestamps.
use super::*;
use crate::matching::bibliographic_match_key as key;

const MAX_ALIGNMENT_CELLS: usize = 1_000_000;

fn number(word: &str) -> Option<u32> {
    if let Ok(value) = word.parse() {
        return Some(value);
    }
    let words = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty"];
    if let Some(index) = words.iter().position(|w| *w == word) {
        return Some(index as u32);
    }
    match word {
        "thirty" => return Some(30),
        "forty" => return Some(40),
        "fifty" => return Some(50),
        "sixty" => return Some(60),
        "seventy" => return Some(70),
        "eighty" => return Some(80),
        "ninety" => return Some(90),
        _ => {}
    }
    let roman = word
        .chars()
        .map(|c| match c {
            'i' => Some(1i32),
            'v' => Some(5),
            'x' => Some(10),
            'l' => Some(50),
            'c' => Some(100),
            'd' => Some(500),
            'm' => Some(1000),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if roman.is_empty() {
        return None;
    }
    let value: i32 = roman.iter().enumerate().map(|(i, v)| if roman.get(i + 1).is_some_and(|next| next > v) { -*v } else { *v }).sum();
    (value > 0).then_some(value as u32)
}

fn numbered_prefix(words: &[&str]) -> Option<(u32, usize)> {
    let first = number(words.first()?)?;
    if first >= 20 && first % 10 == 0 {
        if let Some(unit) = words.get(1).and_then(|w| number(w)).filter(|n| (1..10).contains(n)) {
            return Some((first + unit, 2));
        }
    }
    Some((first, 1))
}

fn generic_section(value: &str) -> Option<&'static str> {
    let value = value.split(" by ").next().unwrap_or(value);
    match value {
        "intro" | "introduction" => Some("introduction"),
        "foreword" | "forward" => Some("foreword"),
        "preface" => Some("preface"),
        "prologue" => Some("prologue"),
        "epilogue" => Some("epilogue"),
        "acknowledgments" | "acknowledgements" => Some("acknowledgements"),
        "afterword" => Some("afterword"),
        "opening credits" => Some("opening credits"),
        "closing credits" | "end credits" => Some("closing credits"),
        "dedication" => Some("dedication"),
        _ => None,
    }
}

// Descriptive suffixes do not change a heading's structural role.
fn section(value: &str) -> Option<&'static str> {
    generic_section(value).or_else(|| {
        let words = value.split_whitespace().collect::<Vec<_>>();
        words.first().and_then(|word| generic_section(word)).or_else(|| (words.len() >= 2).then(|| words[..2].join(" ")).and_then(|prefix| generic_section(&prefix)))
    })
}

pub fn descriptive(title: &str, book: &str) -> bool {
    let clean = key(title);
    let book_key = key(book);
    if clean.is_empty() || clean == book_key || matches!(clean.as_str(), "full audiobook" | "audiobook" | "untitled") || generic_section(&clean).is_some() {
        return false;
    }
    // File-derived headings often repeat the book title followed by a track
    // number. The repeated title is not a descriptive chapter heading.
    if !book_key.is_empty() {
        if let Some(suffix) = clean.strip_prefix(&book_key).and_then(|rest| rest.strip_prefix(' ')) {
            if !descriptive(suffix, "") {
                return false;
            }
        }
    }
    let words = clean.split_whitespace().collect::<Vec<_>>();
    let rest = if words.first().is_some_and(|w| matches!(*w, "chapter" | "chap" | "track" | "disc" | "disk" | "part" | "book" | "volume" | "kapitel")) { &words[1..] } else { &words[..] };
    if rest.is_empty() || rest.iter().all(|word| number(word).is_some() || matches!(*word, "hundred" | "thousand" | "and" | "chapter" | "chap" | "track" | "disc" | "disk" | "part" | "book" | "volume" | "kapitel")) {
        return false;
    }
    if let Some((_, used)) = numbered_prefix(rest) {
        return rest.len() > used;
    }
    // Numeric disc/track strings and punctuation-only headings are generic.
    !words.iter().all(|word| word.chars().all(|c| c.is_ascii_digit()))
}

#[derive(Clone)]
struct Label {
    number: Option<u32>,
    context: Option<(String, u32)>,
    section: Option<&'static str>,
    named: Option<String>,
    part: bool,
}

fn labels(chapters: &[AudiobookChapter], book: &str) -> Vec<Label> {
    let mut context = None;
    chapters
        .iter()
        .map(|chapter| {
            let clean = key(&chapter.title);
            let mut words = clean.split_whitespace().collect::<Vec<_>>();
            // Disc and track markers are never chapter-number anchors.
            if words.first().is_some_and(|w| matches!(*w, "disc" | "disk" | "track")) {
                if let Some((_, used)) = numbered_prefix(&words[1..]) {
                    words.drain(..used + 1);
                }
            }
            let part = words.first().is_some_and(|w| matches!(*w, "part" | "book" | "volume"));
            let explicit = part || words.first().is_some_and(|w| matches!(*w, "chapter" | "chap" | "kapitel"));
            let prefix = usize::from(explicit);
            let numbered = if explicit { numbered_prefix(&words[prefix..]) } else { None };
            if part {
                if let Some((number, _)) = numbered {
                    context = Some((words[0].to_owned(), number));
                }
            }
            let body = numbered.map(|(_, n)| words[prefix + n..].join(" ")).unwrap_or_else(|| words.join(" "));
            Label { number: numbered.map(|(n, _)| n), context: context.clone(), section: section(&clean), named: (descriptive(&chapter.title, book) && !body.is_empty()).then_some(body), part }
        })
        .collect()
}

fn evidence(a: &Label, b: &Label) -> u16 {
    if a.context.is_some() && b.context.is_some() && a.context != b.context {
        return 0;
    }
    if a.named.is_some() && a.named == b.named {
        return 5;
    }
    if a.section.is_some() && a.section == b.section {
        return 3;
    }
    if a.number.is_some() && a.number == b.number && a.part == b.part {
        return 2;
    }
    0
}

fn valid(chapters: &[AudiobookChapter], duration: u64) -> bool {
    !chapters.is_empty() && chapters.iter().all(|c| c.start_ms < c.end_ms && c.end_ms <= duration) && chapters.windows(2).all(|w| w[0].end_ms <= w[1].start_ms)
}

fn compatible(local: &[AudiobookChapter], remote: &[AudiobookChapter], book: &str, pairs: &[(usize, usize)]) -> bool {
    let a = labels(local, book);
    let b = labels(remote, book);
    pairs.iter().all(|(i, j)| {
        !(a[*i].section.is_some() && b[*j].section.is_some() && a[*i].section != b[*j].section)
            && !(a[*i].section.is_some() && b[*j].number.is_some())
            && !(b[*j].section.is_some() && a[*i].number.is_some())
            && !(a[*i].number.is_some() && b[*j].number.is_some() && (a[*i].number != b[*j].number || a[*i].part != b[*j].part))
            && !(a[*i].context.is_some() && b[*j].context.is_some() && a[*i].context != b[*j].context)
    })
}

fn rename(local: &[AudiobookChapter], remote: &[AudiobookChapter], book: &str, pairs: &[(usize, usize)], method: AlignmentMethod) -> Option<ChapterPlan> {
    let mut chapters = local.to_vec();
    let mut renamed = 0;
    for (i, j) in pairs {
        if !descriptive(&chapters[*i].title, book) && descriptive(&remote[*j].title, book) && chapters[*i].title != remote[*j].title {
            chapters[*i].title = remote[*j].title.clone();
            renamed += 1;
        }
    }
    (renamed > 0).then(|| ChapterPlan { method, chapters, renamed, detail: "Descriptive names added; local timestamps preserved".into() })
}

pub fn plan(request: &LookupRequest, candidate: &Candidate) -> Option<ChapterPlan> {
    let local = &request.chapters;
    let remote = &candidate.chapters;
    let duration = candidate.duration_ms?;
    if !valid(remote, duration) || !valid(local, request.duration_ms) || !remote.iter().any(|c| descriptive(&c.title, &request.title)) {
        return None;
    }
    if local.len() == remote.len() {
        let pairs = (0..local.len()).map(|i| (i, i)).collect::<Vec<_>>();
        if compatible(local, remote, &request.title, &pairs) || generated_sequence_with_identical_boundaries(local, remote) {
            return rename(local, remote, &request.title, &pairs, AlignmentMethod::EqualCountNames);
        }
    }
    if let Some(pairs) = anchors(local, remote, &request.title, request.duration_ms) {
        if let Some(plan) = rename(local, remote, &request.title, &pairs, AlignmentMethod::SectionAnchors) {
            return Some(plan);
        }
    }
    if let Some(pairs) = duration_pattern(local, remote, &request.title) {
        if let Some(plan) = rename(local, remote, &request.title, &pairs, AlignmentMethod::DurationPattern) {
            return Some(plan);
        }
    }
    // Recording selection has already established the candidate. Accept its
    // descriptive TOC within the user-selected 1% runtime limit even when local
    // track segmentation or edition ISBNs differ. Never rescale chapter starts.
    if request.duration_ms > 0 && request.duration_ms.abs_diff(duration) <= request.duration_ms / 100 && remote.first()?.start_ms == 0 {
        let mut chapters = remote.iter().filter(|chapter| chapter.start_ms < request.duration_ms).cloned().collect::<Vec<_>>();
        let last = chapters.last_mut()?;
        last.end_ms = request.duration_ms;
        if !valid(&chapters, request.duration_ms) {
            return None;
        }
        let renamed = chapters.iter().filter(|c| descriptive(&c.title, &request.title)).count();
        return Some(ChapterPlan {
            method: AlignmentMethod::ImportedBoundaries,
            chapters,
            renamed,
            detail: "Accepted provider TOC within 1% total runtime; ISBN and track-count differences allowed; starts preserved, entries beyond local end omitted and final end clamped".into(),
        });
    }
    None
}

// A file can number every track "Chapter N", including credits and interludes.
// Override those apparent number conflicts only when the entire varied timeline
// independently agrees. Explicit local section names retain their meaning.
fn generated_sequence_with_identical_boundaries(local: &[AudiobookChapter], remote: &[AudiobookChapter]) -> bool {
    local.len() >= 5
        && local.len() == remote.len()
        && local.iter().enumerate().all(|(index, chapter)| {
            let clean = key(&chapter.title);
            let words = clean.split_whitespace().collect::<Vec<_>>();
            words.first().is_some_and(|word| matches!(*word, "chapter" | "chap" | "track" | "kapitel")) && numbered_prefix(&words[1..]).is_some_and(|(n, used)| n as usize == index + 1 && used + 1 == words.len())
        })
        && local.iter().zip(remote).all(|(a, b)| a.start_ms.abs_diff(b.start_ms) <= 250 && a.end_ms.abs_diff(b.end_ms) <= 250)
        && distinct_durations(local.iter().map(|c| c.end_ms - c.start_ms).collect()) >= 3
}

fn anchors(local: &[AudiobookChapter], remote: &[AudiobookChapter], book: &str, duration: u64) -> Option<Vec<(usize, usize)>> {
    let n = local.len();
    let m = remote.len();
    if (n + 1).checked_mul(m + 1)? > MAX_ALIGNMENT_CELLS {
        return None;
    }
    let a = labels(local, book);
    let b = labels(remote, book);
    let mut dp = vec![0u16; (n + 1) * (m + 1)];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * (m + 1) + j] = dp[(i + 1) * (m + 1) + j].max(dp[i * (m + 1) + j + 1]).max(evidence(&a[i], &b[j]) + dp[(i + 1) * (m + 1) + j + 1]);
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut pairs = Vec::new();
    while i < n && j < m {
        let score = evidence(&a[i], &b[j]);
        if score > 0 && dp[i * (m + 1) + j] == score + dp[(i + 1) * (m + 1) + j + 1] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[(i + 1) * (m + 1) + j] >= dp[i * (m + 1) + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    if pairs.len() < 3 {
        return None;
    }
    let mut good = vec![false; pairs.len()];
    let mut intervals = 0;
    let mut agreeing = 0;
    for (index, w) in pairs.windows(2).enumerate() {
        let ld = local[w[1].0].start_ms - local[w[0].0].start_ms;
        let rd = remote[w[1].1].start_ms - remote[w[0].1].start_ms;
        if ld < 10_000 || rd < 10_000 {
            continue;
        }
        intervals += 1;
        if ld.abs_diff(rd) <= 5000.max(rd / 50) {
            agreeing += 1;
            good[index] = true;
            good[index + 1] = true;
        }
    }
    let span = local[pairs.last()?.0].start_ms - local[pairs.first()?.0].start_ms;
    if intervals < 2 || agreeing * 5 < intervals * 4 || span < duration / 2 {
        return None;
    }
    // Only supported anchors are renamed; do not interpolate through conflicts.
    Some(pairs.into_iter().zip(good).filter_map(|(p, good)| good.then_some(p)).collect())
}

/// Experimental pattern alignment: bounded 1:1 and adjacent 1:2..4 matches,
/// at least five independent matches and 90% audio coverage on both sides.
/// Comparison never rescales local timestamps.
fn duration_pattern(local: &[AudiobookChapter], remote: &[AudiobookChapter], book: &str) -> Option<Vec<(usize, usize)>> {
    let (n, m) = (local.len(), remote.len());
    if n < 5 || m < 5 || (n + 1).checked_mul(m + 1)? > MAX_ALIGNMENT_CELLS {
        return None;
    }
    if local.windows(2).any(|w| w[0].end_ms.abs_diff(w[1].start_ms) > 100) || remote.windows(2).any(|w| w[0].end_ms.abs_diff(w[1].start_ms) > 100) {
        return None;
    }
    let prefix = |cs: &[AudiobookChapter]| {
        let mut p = vec![0u64];
        for c in cs {
            p.push(p.last().unwrap() + c.end_ms - c.start_ms);
        }
        p
    };
    let a = prefix(local);
    let b = prefix(remote);
    let mut dp = vec![0f64; (n + 1) * (m + 1)];
    let mut back = vec![(0usize, 0usize); dp.len()];
    for i in 1..=n {
        dp[i * (m + 1)] = i as f64 * 1.5;
        back[i * (m + 1)] = (1, 0);
    }
    for j in 1..=m {
        dp[j] = j as f64 * 1.5;
        back[j] = (0, 1);
    }
    for i in 1..=n {
        for j in 1..=m {
            let at = i * (m + 1) + j;
            dp[at] = dp[at - (m + 1)] + 1.5;
            back[at] = (1, 0);
            if dp[at - 1] + 1.5 < dp[at] {
                dp[at] = dp[at - 1] + 1.5;
                back[at] = (0, 1);
            }
            for (x, y) in [(1, 1), (1, 2), (1, 3), (1, 4), (2, 1), (3, 1), (4, 1)] {
                if i < x || j < y {
                    continue;
                }
                let ld = a[i] - a[i - x];
                let rd = b[j] - b[j - y];
                let cost = dp[(i - x) * (m + 1) + j - y] + (ld.abs_diff(rd) as f64 / 2000.max(ld.max(rd) / 100) as f64).min(6.0) + 0.25 * (x + y - 2) as f64;
                if cost < dp[at] {
                    dp[at] = cost;
                    back[at] = (x, y);
                }
            }
        }
    }
    let (mut i, mut j) = (n, m);
    let mut pairs = Vec::new();
    let (mut matched, mut la, mut ra) = (0, 0u64, 0u64);
    let mut local_lengths = Vec::new();
    let mut remote_lengths = Vec::new();
    while i > 0 || j > 0 {
        let (x, y) = back[i * (m + 1) + j];
        let ld = a[i] - a[i - x];
        let rd = b[j] - b[j - y];
        if x > 0 && y > 0 && ld.min(rd) >= 20_000 && ld.abs_diff(rd) <= 2000.max(ld.max(rd) / 100) {
            matched += 1;
            la += ld;
            ra += rd;
            local_lengths.extend((i - x..i).map(|k| a[k + 1] - a[k]));
            remote_lengths.extend((j - y..j).map(|k| b[k + 1] - b[k]));
            // Aggregated segments support the recording, but only unambiguous
            // one-to-one intervals receive a name in this conservative plan.
            if x == 1 && y == 1 {
                pairs.push((i - 1, j - 1));
            }
        }
        i -= x;
        j -= y;
    }
    pairs.reverse();
    if matched < 5 || la * 10 < a[n] * 9 || ra * 10 < b[m] * 9 || !compatible(local, remote, book, &pairs) || distinct_durations(local_lengths) < 3 || distinct_durations(remote_lengths) < 3 {
        return None;
    }
    Some(pairs)
}

// Repeated fixed-length tracks are one observation, regardless of their count.
// Require distinct durations on both sides, separated beyond matching tolerance.
fn distinct_durations(mut lengths: Vec<u64>) -> usize {
    lengths.sort_unstable();
    let mut representatives = Vec::<u64>::new();
    for length in lengths {
        if representatives.last().is_none_or(|previous| length.abs_diff(*previous) > 4000.max(length / 50)) {
            representatives.push(length);
        }
    }
    representatives.len()
}

pub fn recording_alignment(request: &LookupRequest, candidate: &Candidate) -> bool {
    let Some(duration) = candidate.duration_ms else {
        return false;
    };
    valid(&request.chapters, request.duration_ms)
        && valid(&candidate.chapters, duration)
        && (anchors(&request.chapters, &candidate.chapters, &request.title, request.duration_ms).is_some() || duration_pattern(&request.chapters, &candidate.chapters, &request.title).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chapters(names: &[&str], lengths: &[u64]) -> Vec<AudiobookChapter> {
        let mut start = 0;
        names
            .iter()
            .zip(lengths)
            .map(|(name, len)| {
                let c = AudiobookChapter { title: (*name).into(), start_ms: start, end_ms: start + len };
                start += len;
                c
            })
            .collect()
    }
    fn request(local: Vec<AudiobookChapter>) -> LookupRequest {
        LookupRequest { title: "A Specific Book".into(), authors: vec![], duration_ms: local.last().unwrap().end_ms, recording: RecordingEvidence::default(), chapters: local }
    }
    fn candidate(remote: Vec<AudiobookChapter>) -> Candidate {
        Candidate {
            asin: "B000000001".into(),
            region: "us".into(),
            title: "A Specific Book".into(),
            subtitle: None,
            authors: vec![],
            narrators: vec![],
            publisher: None,
            description: None,
            release_date: None,
            isbns: vec![],
            abridged: None,
            provider_duration_ms: None,
            duration_ms: Some(remote.last().unwrap().end_ms),
            chapters: remote,
            metadata_provider: "audible".into(),
            chapter_provider: Some("audible".into()),
            source: DiscoverySource::Api,
        }
    }
    #[test]
    fn equal_count_renames_generic_only_and_keeps_all_times_despite_runtime_mismatch() {
        let r = request(chapters(&["Chapter 1", "A descriptive local title"], &[100_000, 200_000]));
        let c = candidate(chapters(&["Chapter One: Origins", "Chapter Two: Consequences"], &[130_000, 220_000]));
        let p = plan(&r, &c).unwrap();
        assert_eq!(p.method, AlignmentMethod::EqualCountNames);
        assert_eq!(p.renamed, 1);
        assert_eq!(p.chapters[1], r.chapters[1]);
        assert_eq!(p.chapters[0].end_ms, 100_000);
        assert_eq!(p.chapters[0].title, "Chapter One: Origins");
    }
    #[test]
    fn extra_foreword_aligns_numbered_chapters_without_shifting_timestamps() {
        let r = request(chapters(&["Chapter I", "Chapter Two", "Chapter 3", "Chapter 4"], &[100_000, 200_000, 130_000, 180_000]));
        let c = candidate(chapters(&["Foreword", "Chapter 1: Origins", "Chapter II: Development", "Chapter Three: Conflict", "Chapter Four: Resolution"], &[60_000, 100_000, 200_000, 130_000, 180_000]));
        let p = plan(&r, &c).unwrap();
        assert_eq!(p.method, AlignmentMethod::SectionAnchors);
        assert_eq!(p.renamed, 4);
        assert_eq!(p.chapters[0].start_ms, 0);
        assert_eq!(p.chapters[0].title, "Chapter 1: Origins");
        assert!(recording_alignment(&r, &c));
    }

    #[test]
    fn malformed_timestamps_and_generic_provider_names_never_produce_plans() {
        let r = request(chapters(&["Chapter 1", "Chapter 2"], &[100_000; 2]));
        let mut c = candidate(chapters(&["Chapter One", "Chapter Two"], &[100_000; 2]));
        assert!(plan(&r, &c).is_none());
        c.chapters[0].title = "A rich name".into();
        c.chapters[0].end_ms = 150_000;
        assert!(plan(&r, &c).is_none());
    }

    #[test]
    fn repeated_fixed_length_tracks_are_not_independent_alignment_evidence() {
        let names = (1..=10).map(|i| format!("Track {i}")).collect::<Vec<_>>();
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let r = request(chapters(&refs, &[60_000; 10]));
        let names = (1..=11).map(|i| format!("Unrelated descriptive title {i}")).collect::<Vec<_>>();
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let c = candidate(chapters(&refs, &[60_000; 11]));
        assert!(!recording_alignment(&r, &c));
        assert!(plan(&r, &c).is_none());
    }

    #[test]
    fn varied_duration_pattern_still_aligns_after_an_extra_intro() {
        let r = request(chapters(&["Track 1", "Track 2", "Track 3", "Track 4", "Track 5", "Track 6"], &[47_000, 83_000, 131_000, 197_000, 269_000, 347_000]));
        let c = candidate(chapters(
            &["Introduction", "An early beginning", "A different path", "The turning point", "An unexpected challenge", "A final journey", "A lasting conclusion"],
            &[21_000, 47_000, 83_000, 131_000, 197_000, 269_000, 347_000],
        ));
        let p = plan(&r, &c).unwrap();
        assert_eq!(p.method, AlignmentMethod::DurationPattern);
        assert_eq!(p.renamed, 6);
        assert_eq!(p.chapters[0].start_ms, 0);
        assert_eq!(p.chapters[0].title, "An early beginning");
    }


}
