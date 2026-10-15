use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BoundaryPolicy {
    IpAddress,
    Time,
}

impl BoundaryPolicy {
    pub(super) fn allows_end(self, context: &ParseContext<'_>, end_idx: usize) -> bool {
        let stream = context.stream();
        structure::parser_api::units(stream).get(end_idx).is_none_or(|unit| {
            unit.is_tag()
                || unit.kind() == UnitKind::Whitespace
                || structure::parser_api::unit_text(stream, end_idx).is_some_and(|text| self.allows_right_text(context, text))
        })
    }

    fn allows_right_text(self, context: &ParseContext<'_>, text: &str) -> bool {
        is_context_close_unit(context, text)
            || match self {
                Self::IpAddress => matches!(text, "." | "," | ";" | ":" | "!" | "?"),
                Self::Time => matches!(text, "." | "," | ";" | "!" | "?"),
            }
    }
}

pub(super) fn allows_boundary_start(context: &ParseContext<'_>, start_idx: usize) -> bool {
    if start_idx == 0 {
        return true;
    }
    let stream = context.stream();
    structure::parser_api::units(stream).get(start_idx - 1).is_none_or(|unit| unit.is_tag() || unit.kind() == UnitKind::Whitespace || structure::parser_api::unit_text(stream, start_idx - 1).is_some_and(|text| is_context_open_unit(context, text)))
}
