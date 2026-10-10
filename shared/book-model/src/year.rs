use std::collections::BTreeSet;

/// Returns a year only when the supplied date evidence contains exactly one
/// plausible four-digit book year.
pub fn unique_book_year<'a>(values: impl Iterator<Item = &'a str>) -> Option<i32> {
    let mut years = BTreeSet::new();
    for value in values {
        for run in value.split(|character: char| !character.is_ascii_digit()) {
            if run.len() == 4 {
                if let Ok(year) = run.parse::<i32>() {
                    if (1000..=2100).contains(&year) {
                        years.insert(year);
                    }
                }
            }
        }
    }
    (years.len() == 1).then(|| *years.first().expect("one book year exists"))
}
