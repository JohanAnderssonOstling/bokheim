//! The ISBN key shared by snapshot, provider, and crawl tables.
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use std::fmt;

/// A checksum-verified ISBN-13. ISBN-10 input becomes its 978-prefixed form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Isbn13(i64);

impl Isbn13 {
    /// Accepts ISBN-10 or ISBN-13 digits among any separators, as dumps and
    /// catalog records write them.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let compact = value.bytes().filter(|byte| byte.is_ascii_digit() || matches!(byte, b'X' | b'x')).collect::<Vec<_>>();
        match compact.len() {
            10 => {
                let valid = compact[..9].iter().all(u8::is_ascii_digit)
                    && compact.iter().enumerate().map(|(index, byte)| (10 - index as u32) * if byte.is_ascii_digit() { u32::from(byte - b'0') } else { 10 }).sum::<u32>() % 11 == 0;
                let first_twelve = 978_000_000_000 + digits(&compact[..9]);
                valid.then(|| Self(first_twelve * 10 + check_digit(first_twelve)))
            }
            13 if compact.iter().all(u8::is_ascii_digit) => Self::from_digits(digits(&compact)),
            _ => None,
        }
    }

    fn from_digits(value: i64) -> Option<Self> {
        ((0..10_000_000_000_000).contains(&value) && check_digit(value / 10) == value % 10).then_some(Self(value))
    }

    /// The ISBN-10 of a 978-prefixed ISBN.
    pub(crate) fn to_isbn10(self) -> Option<String> {
        if self.0 / 10_000_000_000 != 978 {
            return None;
        }
        let body = format!("{:09}", self.0 / 10 % 1_000_000_000);
        let sum = body.bytes().enumerate().map(|(index, byte)| u32::from(byte - b'0') * (10 - index as u32)).sum::<u32>();
        let check = (11 - sum % 11) % 11;
        Some(format!("{body}{}", if check == 10 { 'X' } else { char::from(b'0' + check as u8) }))
    }
}

fn digits(bytes: &[u8]) -> i64 {
    bytes.iter().fold(0, |value, byte| value * 10 + i64::from(byte - b'0'))
}

/// The ISBN-13 check digit over the first twelve digits.
fn check_digit(first_twelve: i64) -> i64 {
    let sum = (0..12).map(|position| first_twelve / 10_i64.pow(11 - position) % 10 * if position % 2 == 0 { 1 } else { 3 }).sum::<i64>();
    (10 - sum % 10) % 10
}

impl fmt::Display for Isbn13 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:013}", self.0)
    }
}

impl ToSql for Isbn13 {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0))
    }
}

impl FromSql for Isbn13 {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let value = i64::column_result(value)?;
        Self::from_digits(value).ok_or(FromSqlError::OutOfRange(value))
    }
}
