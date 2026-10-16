use super::*;

#[test]
fn preserves_existing_pronunciation_rules() {
    let cases = [
        ("", ""),
        ("The reader opened the book beside the window.", "The reader opened the book beside the window."),
        ("Hello, world!", "Hello, world!"),
        ("I walked home.", "I walked home."),
        ("Chapter IV", "Chapter four"),
        ("IV. The beginning.", "four. The beginning."),
        ("Part mix", "Part mix"),
        ("I.M. Pei and C.S. Lewis met J.R.R. Tolkien.", "I M Pei and C S Lewis met J R R Tolkien."),
        ("William F. Buckley", "William F Buckley"),
        ("Mr. Smith met Dr. Jones. Mrs. Brown said hello.", "Mister Smith met Doctor Jones. Missus Brown said hello."),
        ("He paid 3.14 dollars. Really?", "He paid three point one four dollars. Really?"),
        ("e.g. apples, i.e. fruit; n.b. the note; p.s. see Ph.D. students et al.", "for example apples, that is fruit; note well the note; postscript see Doctor of Philosophy students and others."),
        ("Fig. 2, vol. IV, pp. 12 and 13; approx. 5 kg.", "figure two, volume four, pages twelve and thirteen; approximately five kilograms."),
        ("Jan. 3, 2024 and 3 Sept. 2025.", "January third, twenty twenty-four and September third, twenty twenty-five."),
        ("May flowers bloom. March winds blow.", "May flowers bloom. March winds blow."),
        ("June 12. Jan. 31.", "June twelfth. January thirty-first."),
        ("It costs $12.50 or €15, with 25% off.", "It costs twelve dollars and fifty cents or fifteen euros, with twenty-five percent off."),
        ("He paid $60,000.", "He paid sixty thousand dollars."),
        ("$1, $2, $0.01, $0.50, and $1.01.", "one dollar, two dollars, one cent, fifty cents, and one dollar and one cent."),
        ("USD 60,000 and CAD 60,000.", "sixty thousand United States dollars and sixty thousand Canadian dollars."),
        ("The ratio is 3:2, at 12:30 p.m.", "The ratio is three to two, at twelve thirty Pee Em."),
        ("The date is 2024-01-03 and 3/4/2025.", "The date is January third, twenty twenty-four and 3/4/2025."),
        ("The room is 3 × 4 m, at 20 °C.", "The room is three times four m, at twenty degrees Celsius."),
        ("Use 1/2 cup, ⅔ teaspoon, and 1.5 litres.", "Use half a cup, two thirds teaspoon, and one point five liters."),
        ("2 + 3 = 5, and 6 − 2 = 4.", "two plus three equals five, and six minus two equals four."),
        ("He finished 21st, before the 22nd runner.", "He finished twenty-first, before the twenty-second runner."),
        ("Try 3.5e-4 and 6 × 10³.", "Try three point five times ten to the power of minus four and six times ten to the power of three."),
        ("Visit https://example.com/Jan. or email a.b@example.com.", "Visit https://example.com/Jan. or email a.b@example.com."),
        ("Code `Dr. X = 3` stays literal.", "Code `Dr. X = 3` stays literal."),
        ("Null\0characters and ﬀ ﬁ ﬂ ﬃ ﬄ.", "Nullcharacters and ff fi fl ffi ffl."),
        ("discre\u{ad}\ntionary and para\u{ad}\r\ngraph.", "discretionary and paragraph."),
        ("no\u{ad}\n\njoin and upper\u{ad}\nCase.", "no\n\njoin and upper\nCase."),
        ("He said “Mr. Smith!” Then left.", "He said “Mister Smith!” Then left."),
        ("Ariz. and Calif. are places; Wash. is ambiguous.", "Arizona and California are places; Wash. is ambiguous."),
        ("Jan.3 and 1.Jan and 3 Jan..", "January third and 1.Jan and January third."),
        ("Chapter MMMCMXCIX; Chapter IIX; Appendix CIVIL.", "Chapter three thousand nine hundred and ninety-nine; Chapter IIX; Appendix CIVIL."),
        ("The cafe\u{301} serves ångström-sized treats. 界", "The cafe\u{301} serves ångström-sized treats. 界"),
        ("\"Jan. 3\" at https://example.com followed by Dr. Smith and 1/2 cup.", "\"January third\" at https://example.com followed by Doctor Smith and half a cup."),
    ];
    for (input, expected) in cases {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn spells_general_numbers_after_contextual_rules() {
    for (input, expected) in [
        ("0, 1, 12, and 42.", "zero, one, twelve, and forty-two."),
        ("There are 60,000 books and 1905 houses.", "There are sixty thousand books and one thousand nine hundred and five houses."),
        ("In 1905, he left. Since 2005.", "In nineteen oh five, he left. Since two thousand and five."),
        ("1905", "one thousand nine hundred and five"),
        ("-60,000 and +5", "minus sixty thousand and plus five"),
        ("1.040 and 0.05", "one point zero four zero and zero point zero five"),
        ("007 and 00123", "007 and 00123"),
        ("4,294,967,296", "four billion two hundred and ninety-four million nine hundred and sixty-seven thousand two hundred and ninety-six"),
        ("1,000,000,000,001", "one trillion and one"),
        ("5–10 kg", "five to ten kilograms"),
        ("Chapter 4; Fig. 2; pp. 12–15.", "Chapter four; figure two; pages twelve to fifteen."),
        ("v1.2.3 build_42 A320", "v1.2.3 build_42 A320"),
        ("https://example.com/123 and a123@example.com", "https://example.com/123 and a123@example.com"),
        ("`42 + 5` and 6", "`42 + 5` and six"),
        ("1/2 and 03/04 and 3/4/2025", "1/2 and 03/04 and 3/4/2025"),
    ] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn distinguishes_preposition_in_from_explicit_inches() {
    for (input, expected) in [
        ("1988 in New York", "one thousand nine hundred and eighty-eight in New York"),
        ("1988 in new york", "one thousand nine hundred and eighty-eight in new york"),
        ("There were 12 in the room.", "There were twelve in the room."),
        ("Put 5 in each box.", "Put five in each box."),
        ("1988 in", "one thousand nine hundred and eighty-eight in"),
        ("It measures 5in.", "It measures five inches."),
        ("It measures 5 in.", "It measures five inches."),
        ("1 inch and 5 inches.", "one inch and five inches."),
        ("A 3 × 4 in frame.", "A 3 × 4 in frame."),
        ("A 3 × 4in frame.", "A three by four inches frame."),
        ("A 3 × 4 inches frame.", "A three by four inches frame."),
    ] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn spells_large_integer_digits_without_rounding() {
    for (input, expected) in [
        ("007", "zero zero seven"),
        ("9007199254740993", "nine quadrillion seven trillion one hundred and ninety-nine billion two hundred and fifty-four million seven hundred and forty thousand nine hundred and ninety-three"),
        ("18446744073709551616", "one eight four four six seven four four zero seven three seven zero nine five five one six one six"),
    ] {
        assert_eq!(numbers::integer_words(input).as_deref(), Some(expected), "input: {input:?}");
    }
}

#[test]
fn expands_explicit_compact_currency_amounts() {
    for (input, expected) in [
        ("$60k and $2.5m.", "sixty thousand dollars and two point five million dollars."),
        ("€3M and £4b.", "three million euros and four billion pounds."),
        ("-$2k and +$3M.", "minus two thousand dollars and plus three million dollars."),
        ("1¢, 50¢, and 0¢.", "one cent, fifty cents, and zero cents."),
        ("60k and 2.5m", "60k and two point five meters"),
        ("`$60k 50¢`", "`$60k 50¢`"),
        ("https://example.com/$60k", "https://example.com/$60k"),
        ("$2miles $3kg item$4k $5k_test", "$2miles $3kg item$4k $5k_test"),
        ("$1,000k", "$1,000k"),
        ("50¢each", "50¢each"),
        ("0.5¢", "0.5¢"),
    ] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn expands_typed_fractions_only_with_units() {
    for (input, expected) in [
        ("1/2 cup", "half a cup"),
        ("1 1/2 cups", "one and a half cups"),
        ("3/4 tsp", "three quarters of a teaspoon"),
        ("1/4 tbsp", "a quarter of a tablespoon"),
        ("1/2 kg", "half a kilogram"),
        ("2 1/2 hours", "two and a half hours"),
        ("-1/2 cup", "minus half a cup"),
        ("1\u{00a0}1/2 cups", "one and a half cups"),
        ("1/2 and 1 1/2", "1/2 and 1 1/2"),
        ("03/04 kg", "03/04 kg"),
        ("2025/1/2 cups", "January second, twenty twenty-five cups"),
        ("v1/2 kg and part_1/2 cup", "v1/2 kilograms and part_1/2 cup"),
        ("1/2 people", "1/2 people"),
        ("1/2 cupful", "1/2 cupful"),
        ("1/2\ncup", "1/2\ncup"),
        ("`1/2 cup`", "`1/2 cup`"),
        ("``1/2 cup and `$60k` ``", "``1/2 cup and `$60k` ``"),
        ("```text\n$60k and 1/2 cup\n```", "```text\n$60k and 1/2 cup\n```"),
        ("https://example.com/1/2 cup", "https://example.com/1/2 cup"),
    ] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn collapses_ellipses_for_speech() {
    for (input, expected) in [
        ("Wait.. really... Yes....", "Wait. really. Yes."),
        ("Wait… really…… Yes.….", "Wait. really. Yes."),
        ("“Well...” she said…", "“Well.” she said."),
        ("At 3.14... Dr. Smith paused.", "At three point one four. Doctor Smith paused."),
        ("Visit https://example.com/a...b and wait...", "Visit https://example.com/a...b and wait."),
    ] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn separates_words_at_em_dashes_for_speech() {
    for (input, expected) in [("Did you—did", "Did you did"), ("“Did you—did you hear?”", "“Did you did you hear?”"), ("A well-known writer—perhaps.", "A well-known writer perhaps."), ("Wait—really...", "Wait really.")] {
        assert_eq!(prepare_speech_text(input), expected, "input: {input:?}");
    }
}

#[test]
fn unchanged_text_stays_borrowed_through_every_stage() {
    for input in ["", "The reader opened the book beside the window.", "Hello, world!", "Visit https://example.com/Jan. or email a.b@example.com.", "Code `Dr. X = 3` stays literal.", "The café serves ångström-sized treats. 界"] {
        assert!(matches!(prepare_speech_text(input), Cow::Borrowed(text) if std::ptr::eq(text, input)), "input: {input:?}");
    }
}
