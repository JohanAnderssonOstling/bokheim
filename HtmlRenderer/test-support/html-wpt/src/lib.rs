//! Test-only adapters for consuming upstream Web Platform Tests unchanged.
//!
//! This crate deliberately contains no renderer knowledge. Owning stage crates
//! translate their public output contracts into the corresponding WPT form.

use std::fmt;

/// A declarative helper used by the CSS parsing Web Platform Tests.
///
/// The adapter intentionally understands helper calls and JavaScript string
/// literals, not JavaScript execution. Calls whose arguments are computed by
/// script are returned separately as unsupported instead of being discarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CssParsingTestKind {
    ValidValue,
    InvalidValue,
    ComputedValue,
    ValidAndComputedValue,
    ShorthandValue,
    ValidSelector,
    InvalidSelector,
    ValidForgivingSelector,
    ValidRule,
    InvalidRule,
    ValidLayerImport,
    InvalidLayerImport,
    ValidSupportsImport,
    InvalidSupportsImport,
    UnsupportedSupportsImport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CssParsingTest {
    pub kind: CssParsingTestKind,
    pub arguments: Vec<String>,
    pub line: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedCssParsingCall {
    pub helper: String,
    pub line: usize,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CssParsingExtraction {
    pub tests: Vec<CssParsingTest>,
    pub unsupported_calls: Vec<UnsupportedCssParsingCall>,
}

/// Extract literal declarative CSS parsing tests from an original WPT HTML
/// file without running its JavaScript.
pub fn extract_css_parsing_tests(source: &str) -> CssParsingExtraction {
    let bytes = source.as_bytes();
    let static_loops = collect_static_loop_bindings(source);
    let mut extraction = CssParsingExtraction::default();
    let mut cursor = 0;
    let mut line = 1;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[cursor];
                skip_js_string(bytes, &mut cursor, &mut line, quote);
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor += 2;
                while cursor + 1 < bytes.len() && !(bytes[cursor] == b'*' && bytes[cursor + 1] == b'/') {
                    if bytes[cursor] == b'\n' {
                        line += 1;
                    }
                    cursor += 1;
                }
                cursor = (cursor + 2).min(bytes.len());
            }
            byte if is_js_identifier_start(byte) => {
                let start = cursor;
                let call_line = line;
                cursor += 1;
                while cursor < bytes.len() && is_js_identifier_continue(bytes[cursor]) {
                    cursor += 1;
                }
                let helper = &source[start..cursor];
                if preceding_word(source, start) == Some("function") {
                    skip_js_function_body(source, &mut cursor, &mut line);
                    continue;
                }
                let mut open = cursor;
                while open < bytes.len() && bytes[open].is_ascii_whitespace() {
                    if bytes[open] == b'\n' {
                        line += 1;
                    }
                    open += 1;
                }
                if (!helper.starts_with("test_") && helper != "assert_valid") || bytes.get(open) != Some(&b'(') {
                    cursor = open;
                    continue;
                }

                match collect_js_call_arguments(source, open, &mut line) {
                    Ok((raw_arguments, end)) => {
                        cursor = end;
                        if helper == "assert_valid" {
                            match expand_is_where_assertion(&raw_arguments, call_line) {
                                Ok(tests) => extraction.tests.extend(tests),
                                Err(reason) => extraction.unsupported_calls.push(UnsupportedCssParsingCall { helper: helper.to_owned(), line: call_line, reason }),
                            }
                            continue;
                        }
                        let kind = css_test_kind(helper);
                        let required_arguments = kind.map(required_argument_count).unwrap_or(raw_arguments.len());
                        let arguments = expand_static_arguments(&raw_arguments, start, &static_loops, required_arguments);
                        match (kind, arguments) {
                            (Some(kind), Some(argument_sets)) => extraction.tests.extend(argument_sets.into_iter().map(|arguments| CssParsingTest { kind, arguments, line: call_line })),
                            (None, _) => extraction.unsupported_calls.push(UnsupportedCssParsingCall { helper: helper.to_owned(), line: call_line, reason: "helper is not implemented by the direct adapter".to_owned() }),
                            (Some(_), None) => {
                                extraction.unsupported_calls.push(UnsupportedCssParsingCall { helper: helper.to_owned(), line: call_line, reason: "one or more arguments are computed JavaScript rather than string literals".to_owned() })
                            }
                        }
                    }
                    Err(reason) => {
                        extraction.unsupported_calls.push(UnsupportedCssParsingCall { helper: helper.to_owned(), line: call_line, reason });
                        cursor = open + 1;
                    }
                }
            }
            b'\n' => {
                line += 1;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }

    extraction
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StaticLoopBinding {
    name: String,
    values: Vec<String>,
    body_start: usize,
    body_end: usize,
}

/// Find the small, declarative `for (const name of ["a", "b"])` matrices
/// used by CSS parsing WPTs. This is data expansion, not JavaScript execution.
fn collect_static_loop_bindings(source: &str) -> Vec<StaticLoopBinding> {
    let bytes = source.as_bytes();
    let mut bindings = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes.get(cursor..cursor + 3) != Some(b"for")
            || cursor.checked_sub(1).and_then(|index| bytes.get(index)).is_some_and(|byte| is_js_identifier_continue(*byte))
            || bytes.get(cursor + 3).is_some_and(|byte| is_js_identifier_continue(*byte))
        {
            advance_js_token(bytes, &mut cursor);
            continue;
        }

        let Some(binding) = parse_static_loop_binding(source, cursor) else {
            cursor += 3;
            continue;
        };
        cursor += 3;
        bindings.push(binding);
    }
    bindings
}

fn parse_static_loop_binding(source: &str, start: usize) -> Option<StaticLoopBinding> {
    let bytes = source.as_bytes();
    let mut cursor = start + 3;
    skip_ascii_whitespace(bytes, &mut cursor);
    expect_byte(bytes, &mut cursor, b'(')?;
    skip_ascii_whitespace(bytes, &mut cursor);
    if bytes.get(cursor..cursor + 5) == Some(b"const") {
        cursor += 5;
    } else if bytes.get(cursor..cursor + 3) == Some(b"let") {
        cursor += 3;
    } else {
        return None;
    }
    skip_ascii_whitespace(bytes, &mut cursor);
    let name_start = cursor;
    while bytes.get(cursor).is_some_and(|byte| is_js_identifier_continue(*byte)) {
        cursor += 1;
    }
    let name = source.get(name_start..cursor)?.to_owned();
    if name.is_empty() {
        return None;
    }
    skip_ascii_whitespace(bytes, &mut cursor);
    if bytes.get(cursor..cursor + 2) != Some(b"of") {
        return None;
    }
    cursor += 2;
    skip_ascii_whitespace(bytes, &mut cursor);
    let array_start = cursor;
    expect_byte(bytes, &mut cursor, b'[')?;
    let array_end = find_matching_delimiter(source, array_start, b'[', b']')?;
    let values = split_js_values(source.get(array_start + 1..array_end)?)?.into_iter().map(parse_js_string_literal).collect::<Option<Vec<_>>>()?;
    cursor = array_end + 1;
    skip_ascii_whitespace(bytes, &mut cursor);
    expect_byte(bytes, &mut cursor, b')')?;
    skip_ascii_whitespace(bytes, &mut cursor);
    let body_start = cursor;
    expect_byte(bytes, &mut cursor, b'{')?;
    let body_end = find_matching_delimiter(source, body_start, b'{', b'}')?;
    Some(StaticLoopBinding { name, values, body_start, body_end })
}

fn expand_static_arguments(raw_arguments: &[&str], call_offset: usize, loops: &[StaticLoopBinding], required_arguments: usize) -> Option<Vec<Vec<String>>> {
    let raw_arguments = &raw_arguments[..raw_arguments.len().min(required_arguments)];
    let enclosing = loops.iter().filter(|binding| binding.body_start < call_offset && call_offset < binding.body_end).collect::<Vec<_>>();
    let mut environments = vec![Vec::<(&str, &str)>::new()];
    for binding in enclosing {
        let mut expanded = Vec::new();
        for environment in &environments {
            for value in &binding.values {
                let mut next = environment.clone();
                next.push((&binding.name, value));
                expanded.push(next);
            }
        }
        environments = expanded;
    }

    environments.into_iter().map(|environment| raw_arguments.iter().map(|argument| evaluate_static_string(argument, &environment)).collect()).collect()
}

fn evaluate_static_string(argument: &str, environment: &[(&str, &str)]) -> Option<String> {
    if let Some(value) = parse_js_string_literal(argument) {
        return Some(value);
    }
    let argument = argument.trim();
    if let Some((_, value)) = environment.iter().find(|(name, _)| *name == argument) {
        return Some((*value).to_owned());
    }
    if !(argument.starts_with('`') && argument.ends_with('`')) {
        return None;
    }

    let mut expanded = argument[1..argument.len() - 1].to_owned();
    for (name, value) in environment {
        expanded = expanded.replace(&format!("${{{name}}}"), value);
    }
    (!expanded.contains("${")).then_some(expanded)
}

fn expand_is_where_assertion(raw_arguments: &[&str], line: usize) -> Result<Vec<CssParsingTest>, String> {
    let [valid, pattern, expected, ..] = raw_arguments else {
        return Err("assert_valid requires boolean, pattern, and expected pattern arguments".to_owned());
    };
    let kind = match valid.trim() {
        "true" => CssParsingTestKind::ValidSelector,
        "false" => CssParsingTestKind::ValidForgivingSelector,
        _ => return Err("assert_valid validity is not a literal boolean".to_owned()),
    };
    let pattern = parse_js_string_literal(pattern).ok_or_else(|| "assert_valid pattern is not a string literal".to_owned())?;
    let expected = if expected.trim() == "null" { None } else { Some(parse_js_string_literal(expected).ok_or_else(|| "assert_valid expected pattern is not null or a string literal".to_owned())?) };

    Ok(["is", "where"]
        .into_iter()
        .map(|pseudo| {
            let selector = pattern.replace("{}", &format!(":{pseudo}"));
            let mut arguments = vec![selector.clone()];
            if let Some(expected) = &expected {
                arguments.push(expected.replace("{}", &format!(":{pseudo}")));
            } else {
                arguments.push(selector);
            }
            CssParsingTest { kind, arguments, line }
        })
        .collect())
}

fn skip_js_function_body(source: &str, cursor: &mut usize, line: &mut usize) {
    let bytes = source.as_bytes();
    let Some(open) = bytes.get(*cursor..).and_then(|tail| tail.iter().position(|byte| *byte == b'{')).map(|offset| *cursor + offset) else {
        return;
    };
    let Some(close) = find_matching_delimiter(source, open, b'{', b'}') else {
        return;
    };
    *line += bytes[open..=close].iter().filter(|byte| **byte == b'\n').count();
    *cursor = close + 1;
}

fn find_matching_delimiter(source: &str, open_at: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(open_at) != Some(&open) {
        return None;
    }
    let mut cursor = open_at + 1;
    let mut depth = 1usize;
    let mut ignored_line = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[cursor];
                skip_js_string(bytes, &mut cursor, &mut ignored_line, quote);
            }
            byte if byte == open => {
                depth += 1;
                cursor += 1;
            }
            byte if byte == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }
    None
}

fn split_js_values(source: &str) -> Option<Vec<&str>> {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut start = 0;
    let mut values = Vec::new();
    let mut ignored_line = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[cursor];
                skip_js_string(bytes, &mut cursor, &mut ignored_line, quote);
            }
            b',' => {
                values.push(source[start..cursor].trim());
                cursor += 1;
                start = cursor;
            }
            _ => cursor += 1,
        }
    }
    let final_value = source[start..].trim();
    if !final_value.is_empty() {
        values.push(final_value);
    }
    Some(values)
}

fn skip_ascii_whitespace(bytes: &[u8], cursor: &mut usize) {
    while bytes.get(*cursor).is_some_and(u8::is_ascii_whitespace) {
        *cursor += 1;
    }
}

fn expect_byte(bytes: &[u8], cursor: &mut usize, expected: u8) -> Option<()> {
    (bytes.get(*cursor) == Some(&expected)).then(|| *cursor += 1)
}

fn advance_js_token(bytes: &[u8], cursor: &mut usize) {
    let mut ignored_line = 0;
    match bytes.get(*cursor).copied() {
        Some(quote @ (b'\'' | b'"' | b'`')) => skip_js_string(bytes, cursor, &mut ignored_line, quote),
        Some(_) => *cursor += 1,
        None => {}
    }
}

fn css_test_kind(helper: &str) -> Option<CssParsingTestKind> {
    Some(match helper {
        "test_valid_value" => CssParsingTestKind::ValidValue,
        "test_invalid_value" => CssParsingTestKind::InvalidValue,
        "test_computed_value" => CssParsingTestKind::ComputedValue,
        "test_valid_and_computed_value" => CssParsingTestKind::ValidAndComputedValue,
        "test_shorthand_value" => CssParsingTestKind::ShorthandValue,
        "test_valid_selector" => CssParsingTestKind::ValidSelector,
        "test_invalid_selector" => CssParsingTestKind::InvalidSelector,
        "test_valid_forgiving_selector" => CssParsingTestKind::ValidForgivingSelector,
        "test_valid_rule" => CssParsingTestKind::ValidRule,
        "test_invalid_rule" => CssParsingTestKind::InvalidRule,
        "test_valid_layer_import" => CssParsingTestKind::ValidLayerImport,
        "test_invalid_layer_import" => CssParsingTestKind::InvalidLayerImport,
        "test_valid_supports_import" => CssParsingTestKind::ValidSupportsImport,
        "test_invalid_supports_import" => CssParsingTestKind::InvalidSupportsImport,
        "test_unsupported_supports_import" => CssParsingTestKind::UnsupportedSupportsImport,
        _ => return None,
    })
}

fn required_argument_count(kind: CssParsingTestKind) -> usize {
    match kind {
        CssParsingTestKind::ValidValue | CssParsingTestKind::InvalidValue | CssParsingTestKind::ComputedValue | CssParsingTestKind::ValidAndComputedValue | CssParsingTestKind::ShorthandValue => 2,
        CssParsingTestKind::ValidSelector
        | CssParsingTestKind::InvalidSelector
        | CssParsingTestKind::ValidForgivingSelector
        | CssParsingTestKind::ValidRule
        | CssParsingTestKind::InvalidRule
        | CssParsingTestKind::ValidLayerImport
        | CssParsingTestKind::InvalidLayerImport
        | CssParsingTestKind::ValidSupportsImport
        | CssParsingTestKind::InvalidSupportsImport
        | CssParsingTestKind::UnsupportedSupportsImport => 1,
    }
}

fn is_js_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$')
}

fn is_js_identifier_continue(byte: u8) -> bool {
    is_js_identifier_start(byte) || byte.is_ascii_digit()
}

fn preceding_word(source: &str, start: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut end = start;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut word_start = end;
    while word_start > 0 && is_js_identifier_continue(bytes[word_start - 1]) {
        word_start -= 1;
    }
    (word_start < end).then_some(&source[word_start..end])
}

fn skip_js_string(bytes: &[u8], cursor: &mut usize, line: &mut usize, quote: u8) {
    *cursor += 1;
    while *cursor < bytes.len() {
        match bytes[*cursor] {
            b'\\' => {
                *cursor += 1;
                if bytes.get(*cursor) == Some(&b'\n') {
                    *line += 1;
                }
                *cursor = (*cursor + 1).min(bytes.len());
            }
            byte if byte == quote => {
                *cursor += 1;
                return;
            }
            b'\n' => {
                *line += 1;
                *cursor += 1;
            }
            _ => *cursor += 1,
        }
    }
}

fn collect_js_call_arguments<'a>(source: &'a str, open: usize, line: &mut usize) -> Result<(Vec<&'a str>, usize), String> {
    let bytes = source.as_bytes();
    let mut cursor = open + 1;
    let mut argument_start = cursor;
    let mut arguments = Vec::new();
    let mut parentheses = 1usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[cursor];
                skip_js_string(bytes, &mut cursor, line, quote);
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < bytes.len() && bytes[cursor] != b'\n' {
                    cursor += 1;
                }
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor += 2;
                while cursor + 1 < bytes.len() && !(bytes[cursor] == b'*' && bytes[cursor + 1] == b'/') {
                    if bytes[cursor] == b'\n' {
                        *line += 1;
                    }
                    cursor += 1;
                }
                cursor = (cursor + 2).min(bytes.len());
            }
            b'(' => {
                parentheses += 1;
                cursor += 1;
            }
            b')' => {
                parentheses -= 1;
                if parentheses == 0 {
                    let final_argument = source[argument_start..cursor].trim();
                    if !final_argument.is_empty() {
                        arguments.push(final_argument);
                    }
                    return Ok((arguments, cursor + 1));
                }
                cursor += 1;
            }
            b'[' => {
                brackets += 1;
                cursor += 1;
            }
            b']' => {
                brackets = brackets.saturating_sub(1);
                cursor += 1;
            }
            b'{' => {
                braces += 1;
                cursor += 1;
            }
            b'}' => {
                braces = braces.saturating_sub(1);
                cursor += 1;
            }
            b',' if parentheses == 1 && brackets == 0 && braces == 0 => {
                arguments.push(source[argument_start..cursor].trim());
                cursor += 1;
                argument_start = cursor;
            }
            b'\n' => {
                *line += 1;
                cursor += 1;
            }
            _ => cursor += 1,
        }
    }

    Err("unterminated helper call".to_owned())
}

fn parse_js_string_literal(argument: &str) -> Option<String> {
    let bytes = argument.as_bytes();
    let quote = *bytes.first()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || bytes.last().copied()? != quote || (quote == b'`' && argument.contains("${")) {
        return None;
    }

    let mut output = String::new();
    let mut cursor = 1;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != b'\\' {
            let character = argument[cursor..bytes.len() - 1].chars().next()?;
            output.push(character);
            cursor += character.len_utf8();
            continue;
        }

        cursor += 1;
        let escaped = *bytes.get(cursor)?;
        cursor += 1;
        match escaped {
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'v' => output.push('\u{000b}'),
            b'0' => output.push('\0'),
            b'\n' => {}
            b'x' => {
                let value = u8::from_str_radix(std::str::from_utf8(bytes.get(cursor..cursor + 2)?).ok()?, 16).ok()?;
                output.push(char::from(value));
                cursor += 2;
            }
            b'u' => {
                let digits = std::str::from_utf8(bytes.get(cursor..cursor + 4)?).ok()?;
                output.push(char::from_u32(u32::from_str_radix(digits, 16).ok()?)?);
                cursor += 4;
            }
            other => output.push(char::from(other)),
        }
    }
    Some(output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptingMode {
    Both,
    Off,
    On,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeConstructionCase {
    pub data: String,
    pub errors: Vec<String>,
    pub new_errors: Vec<String>,
    pub fragment_context: Option<String>,
    pub scripting: ScriptingMode,
    pub document: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeConstructionParseError {
    pub case_index: usize,
    pub message: String,
}

impl fmt::Display for TreeConstructionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "WPT tree-construction case {}: {}", self.case_index, self.message)
    }
}

impl std::error::Error for TreeConstructionParseError {}

/// Parse the original html5lib/WPT tree-construction `.dat` format.
///
/// Test data and expected document dumps are retained verbatim apart from the
/// section-terminating newline specified by the upstream format.
pub fn parse_tree_construction_file(source: &str) -> Result<Vec<TreeConstructionCase>, TreeConstructionParseError> {
    let lines: Vec<&str> = source.lines().collect();
    let mut cursor = 0;
    let mut cases = Vec::new();

    while cursor < lines.len() {
        while cursor < lines.len() && lines[cursor].is_empty() {
            cursor += 1;
        }
        if cursor == lines.len() {
            break;
        }

        let case_index = cases.len() + 1;
        expect_marker(&lines, &mut cursor, "#data", case_index)?;
        let data = collect_until(&lines, &mut cursor, "#errors", case_index)?.join("\n");
        cursor += 1;

        let mut errors = Vec::new();
        while cursor < lines.len() && !is_control_marker(lines[cursor]) {
            if !lines[cursor].is_empty() {
                errors.push(lines[cursor].to_owned());
            }
            cursor += 1;
        }
        let mut new_errors = Vec::new();
        if cursor < lines.len() && lines[cursor] == "#new-errors" {
            cursor += 1;
            while cursor < lines.len() && !is_control_marker(lines[cursor]) {
                if !lines[cursor].is_empty() {
                    new_errors.push(lines[cursor].to_owned());
                }
                cursor += 1;
            }
        }

        let fragment_context = if cursor < lines.len() && lines[cursor] == "#document-fragment" {
            cursor += 1;
            let context = lines.get(cursor).ok_or_else(|| parse_error(case_index, "missing fragment context"))?.to_string();
            cursor += 1;
            Some(context)
        } else {
            None
        };

        let scripting = match lines.get(cursor).copied() {
            Some("#script-off") => {
                cursor += 1;
                ScriptingMode::Off
            }
            Some("#script-on") => {
                cursor += 1;
                ScriptingMode::On
            }
            _ => ScriptingMode::Both,
        };

        expect_marker(&lines, &mut cursor, "#document", case_index)?;
        let document_start = cursor;
        while cursor < lines.len() && lines[cursor] != "#data" {
            cursor += 1;
        }
        let mut document_end = cursor;
        while document_end > document_start && lines[document_end - 1].is_empty() {
            document_end -= 1;
        }
        if document_start == document_end {
            return Err(parse_error(case_index, "missing expected document dump"));
        }

        cases.push(TreeConstructionCase { data, errors, new_errors, fragment_context, scripting, document: lines[document_start..document_end].join("\n") });
    }

    Ok(cases)
}

fn collect_until<'a>(lines: &'a [&str], cursor: &mut usize, marker: &str, case_index: usize) -> Result<Vec<&'a str>, TreeConstructionParseError> {
    let start = *cursor;
    while *cursor < lines.len() && lines[*cursor] != marker {
        *cursor += 1;
    }
    if *cursor == lines.len() {
        return Err(parse_error(case_index, &format!("missing {marker} section")));
    }
    Ok(lines[start..*cursor].to_vec())
}

fn expect_marker(lines: &[&str], cursor: &mut usize, marker: &str, case_index: usize) -> Result<(), TreeConstructionParseError> {
    match lines.get(*cursor).copied() {
        Some(actual) if actual == marker => {
            *cursor += 1;
            Ok(())
        }
        Some(actual) => Err(parse_error(case_index, &format!("expected {marker}, found {actual:?}"))),
        None => Err(parse_error(case_index, &format!("expected {marker}, found end of file"))),
    }
}

fn is_control_marker(line: &str) -> bool {
    matches!(line, "#new-errors" | "#document-fragment" | "#script-off" | "#script-on" | "#document")
}

fn parse_error(case_index: usize, message: &str) -> TreeConstructionParseError {
    TreeConstructionParseError { case_index, message: message.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::{CssParsingTestKind, ScriptingMode, extract_css_parsing_tests, parse_tree_construction_file};

    #[test]
    fn extracts_literal_css_helpers_and_reports_dynamic_calls() {
        let source = r#"
            test_valid_value("margin", "10px");
            // test_invalid_value("ignored", "comment")
            test_computed_value('white-space', 'pre\nwrap', "pre wrap");
            test_invalid_value(property, "bad");
        "#;
        let extraction = extract_css_parsing_tests(source);

        assert_eq!(extraction.tests.len(), 2);
        assert_eq!(extraction.tests[0].kind, CssParsingTestKind::ValidValue);
        assert_eq!(extraction.tests[0].arguments, ["margin", "10px"]);
        assert_eq!(extraction.tests[1].kind, CssParsingTestKind::ComputedValue);
        assert_eq!(extraction.tests[1].arguments, ["white-space", "pre\nwrap"]);
        assert_eq!(extraction.unsupported_calls.len(), 1);
        assert_eq!(extraction.unsupported_calls[0].helper, "test_invalid_value");
    }

    #[test]
    fn expands_literal_selector_generators_without_javascript() {
        let source = r#"
            function assert_valid(valid, pattern, expected_pattern, description) {
                test_valid_selector(pattern, expected_pattern);
            }
            assert_valid(true, "{}(div )", "{}(div)", "trailing whitespace");
            assert_valid(false, "{}(::before)", null, "forgiving selector");
        "#;
        let extraction = extract_css_parsing_tests(source);

        assert_eq!(extraction.unsupported_calls, []);
        assert_eq!(extraction.tests.len(), 4);
        assert_eq!(extraction.tests[0].kind, CssParsingTestKind::ValidSelector);
        assert_eq!(extraction.tests[0].arguments[0], ":is(div )");
        assert_eq!(extraction.tests[1].arguments[0], ":where(div )");
        assert_eq!(extraction.tests[2].kind, CssParsingTestKind::ValidForgivingSelector);
    }

    #[test]
    fn expands_static_keyword_matrices_and_ignores_optional_serialization_arrays() {
        let source = r#"
            for (const first of ['none', 'auto']) {
                for (const second of ['trim-start', 'space-first']) {
                    test_invalid_value("text-spacing", `${first} ${second}`);
                }
            }
            test_valid_selector('*#id', ['*#id', '#id']);
        "#;
        let extraction = extract_css_parsing_tests(source);

        assert_eq!(extraction.unsupported_calls, []);
        assert_eq!(extraction.tests.len(), 5);
        assert_eq!(extraction.tests[0].arguments, ["text-spacing", "none trim-start"]);
        assert_eq!(extraction.tests[3].arguments, ["text-spacing", "auto space-first"]);
        assert_eq!(extraction.tests[4].arguments, ["*#id"]);
    }

    #[test]
    fn parses_multiple_cases_without_rewriting_payloads() {
        let source = "#data\n<p>One<p>Two\n#errors\nold error\n#new-errors\nnew error\n#document\n| <html>\n|   <head>\n|   <body>\n|     <p>\n|       \"One\"\n\n#data\n<b>x\n#errors\n#document-fragment\nsvg svg\n#script-off\n#document\n| <b>\n|   \"x\"\n";
        let cases = parse_tree_construction_file(source).unwrap();

        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].data, "<p>One<p>Two");
        assert_eq!(cases[0].errors, ["old error"]);
        assert_eq!(cases[0].new_errors, ["new error"]);
        assert_eq!(cases[0].scripting, ScriptingMode::Both);
        assert_eq!(cases[1].fragment_context.as_deref(), Some("svg svg"));
        assert_eq!(cases[1].scripting, ScriptingMode::Off);
        assert_eq!(cases[1].document, "| <b>\n|   \"x\"");
    }

    #[test]
    fn reports_the_case_containing_a_malformed_section() {
        let error = parse_tree_construction_file("#data\nx\n#document\n| <html>\n").unwrap_err();
        assert_eq!(error.case_index, 1);
        assert!(error.message.contains("#errors"));
    }
}
