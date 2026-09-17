use super::*;

/// An opaque sole image does not blend with other group members. Accept its
/// grouping when no alternate blend colour space or knockout behavior is needed.
pub(super) fn group(document: &Document, dictionary: &Dictionary, resources: &Dictionary, state: &mut State) -> Option<()> {
    let Ok(group) = dictionary.get(b"Group") else {
        return Some(());
    };
    let group = resolve(document, group)?.as_dict().ok()?;
    if group.get(b"S").and_then(Object::as_name).ok()? != b"Transparency" {
        return None;
    }
    for (key, value) in group.iter() {
        match key.as_slice() {
            b"Type" | b"S" => (),
            b"I" => {
                resolve(document, value)?.as_bool().ok()?;
            }
            b"K" if !resolve(document, value)?.as_bool().ok()? => (),
            b"CS" => state.group_spaces.push(color::resolve_space(document, resources, value)?),
            _ => return None,
        }
    }
    state.grouped = true;
    Some(())
}
