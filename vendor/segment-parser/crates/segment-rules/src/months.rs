//! Existing month-name and abbreviation catalogs shared with the segment parser.
//! Numeric values identify months; consumers own formatting and pronunciation.

pub const ENGLISH_MONTHS: &[(&str, u32)] = &[
    ("january", 1), ("jan", 1), ("jan.", 1),
    ("february", 2), ("feb", 2), ("feb.", 2),
    ("march", 3), ("mar", 3), ("mar.", 3),
    ("april", 4), ("apr", 4), ("apr.", 4),
    ("may", 5),
    ("june", 6), ("jun", 6), ("jun.", 6),
    ("july", 7), ("jul", 7), ("jul.", 7),
    ("august", 8), ("aug", 8), ("aug.", 8),
    ("september", 9), ("sep", 9), ("sep.", 9), ("sept", 9), ("sept.", 9),
    ("october", 10), ("oct", 10), ("oct.", 10),
    ("november", 11), ("nov", 11), ("nov.", 11),
    ("december", 12), ("dec", 12), ("dec.", 12),
];

pub const GERMAN_MONTHS: &[(&str, u32)] = &[
    ("januar", 1), ("jan", 1), ("jan.", 1),
    ("februar", 2), ("feb", 2), ("feb.", 2),
    ("märz", 3), ("maerz", 3), ("mrz", 3), ("mrz.", 3),
    ("april", 4), ("apr", 4), ("apr.", 4),
    ("mai", 5),
    ("juni", 6), ("jun", 6), ("jun.", 6),
    ("juli", 7), ("jul", 7), ("jul.", 7),
    ("august", 8), ("aug", 8), ("aug.", 8),
    ("september", 9), ("sep", 9), ("sep.", 9),
    ("oktober", 10), ("okt", 10), ("okt.", 10),
    ("november", 11), ("nov", 11), ("nov.", 11),
    ("dezember", 12), ("dez", 12), ("dez.", 12),
];

pub const SWEDISH_MONTHS: &[(&str, u32)] = &[
    ("januari", 1), ("jan", 1), ("jan.", 1),
    ("februari", 2), ("feb", 2), ("feb.", 2),
    ("mars", 3),
    ("april", 4), ("apr", 4), ("apr.", 4),
    ("maj", 5),
    ("juni", 6), ("jun", 6), ("jun.", 6),
    ("juli", 7), ("jul", 7), ("jul.", 7),
    ("augusti", 8), ("aug", 8), ("aug.", 8),
    ("september", 9), ("sep", 9), ("sep.", 9),
    ("oktober", 10), ("okt", 10), ("okt.", 10),
    ("november", 11), ("nov", 11), ("nov.", 11),
    ("december", 12), ("dec", 12), ("dec.", 12),
];
