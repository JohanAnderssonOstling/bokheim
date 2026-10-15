use structure::PartOfSpeech;

pub fn part_of_speech_from_penn(tag: &str) -> Option<PartOfSpeech> {
    Some(match tag {
        "NN" | "NN:U" | "NN:UN" | "NNS" => PartOfSpeech::Noun,
        "NNP" | "NNPS" => PartOfSpeech::Propn,
        "JJ" | "JJR" | "JJS" => PartOfSpeech::Adj,
        "VB" | "VBD" | "VBG" | "VBN" | "VBP" | "VBZ" => PartOfSpeech::Verb,
        "MD" => PartOfSpeech::Aux,
        "IN" | "RP" => PartOfSpeech::Adp,
        "TO" | "POS" => PartOfSpeech::Part,
        "DT" | "PDT" | "WDT" | "PRP$" => PartOfSpeech::Det,
        "CC" => PartOfSpeech::Cconj,
        "RB" | "RBR" | "RBS" | "WRB" | "RB_SENT" => PartOfSpeech::Adv,
        "PRP" | "WP" | "EX" => PartOfSpeech::Pron,
        "CD" | "ORD" => PartOfSpeech::Num,
        "UH" => PartOfSpeech::Intj,
        "SYM" => PartOfSpeech::Sym,
        "PCT" | "." | "," | ":" => PartOfSpeech::Punct,
        "" | "UNKNOWN" | "SENT_START" | "SENT_END" => return None,
        _ => return None,
    })
}

pub fn part_of_speech_from_folkets_class(class: &str) -> Option<PartOfSpeech> {
    Some(match class {
        "nn" => PartOfSpeech::Noun,
        "pm" => PartOfSpeech::Propn,
        "vb" => PartOfSpeech::Verb,
        "hjälpverb" => PartOfSpeech::Aux,
        "jj" | "pc" => PartOfSpeech::Adj,
        "ab" => PartOfSpeech::Adv,
        "pn" | "hp" => PartOfSpeech::Pron,
        "ps" | "article" => PartOfSpeech::Det,
        "pp" => PartOfSpeech::Adp,
        "kn" => PartOfSpeech::Cconj,
        "sn" => PartOfSpeech::Sconj,
        "in" => PartOfSpeech::Intj,
        "rg" | "ro" => PartOfSpeech::Num,
        "ie" => PartOfSpeech::Part,
        _ => return None,
    })
}

pub fn unambiguous_folkets_part_of_speech(class_attribute: &str) -> Option<PartOfSpeech> {
    class_attribute.split(',').filter_map(|part| part_of_speech_from_folkets_class(part.trim())).try_fold(None, |found, part_of_speech| match found {
        None => Some(Some(part_of_speech)),
        Some(existing) if existing == part_of_speech => Some(found),
        Some(_) => None,
    }).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penn_content_tags_map_to_upos() {
        assert_eq!(part_of_speech_from_penn("NN"), Some(PartOfSpeech::Noun));
        assert_eq!(part_of_speech_from_penn("NN:UN"), Some(PartOfSpeech::Noun));
        assert_eq!(part_of_speech_from_penn("NNP"), Some(PartOfSpeech::Propn));
        assert_eq!(part_of_speech_from_penn("VBZ"), Some(PartOfSpeech::Verb));
        assert_eq!(part_of_speech_from_penn("MD"), Some(PartOfSpeech::Aux));
        assert_eq!(part_of_speech_from_penn("JJS"), Some(PartOfSpeech::Adj));
        assert_eq!(part_of_speech_from_penn("IN"), Some(PartOfSpeech::Adp));
        assert_eq!(part_of_speech_from_penn("RP"), Some(PartOfSpeech::Adp));
        assert_eq!(part_of_speech_from_penn("TO"), Some(PartOfSpeech::Part));
        assert_eq!(part_of_speech_from_penn("POS"), Some(PartOfSpeech::Part));
        assert_eq!(part_of_speech_from_penn("PRP$"), Some(PartOfSpeech::Det));
        assert_eq!(part_of_speech_from_penn("WDT"), Some(PartOfSpeech::Det));
        assert_eq!(part_of_speech_from_penn("CC"), Some(PartOfSpeech::Cconj));
        assert_eq!(part_of_speech_from_penn("WRB"), Some(PartOfSpeech::Adv));
        assert_eq!(part_of_speech_from_penn("PRP"), Some(PartOfSpeech::Pron));
        assert_eq!(part_of_speech_from_penn("EX"), Some(PartOfSpeech::Pron));
        assert_eq!(part_of_speech_from_penn("CD"), Some(PartOfSpeech::Num));
        assert_eq!(part_of_speech_from_penn("UH"), Some(PartOfSpeech::Intj));
        assert_eq!(part_of_speech_from_penn("PCT"), Some(PartOfSpeech::Punct));
    }

    #[test]
    fn penn_non_pos_tags_are_none() { ["", "UNKNOWN", "SENT_START", "SENT_END"].into_iter().for_each(|tag| assert_eq!(part_of_speech_from_penn(tag), None, "{tag}")); }

    #[test]
    fn folkets_single_classes_map_to_upos() {
        assert_eq!(part_of_speech_from_folkets_class("nn"), Some(PartOfSpeech::Noun));
        assert_eq!(part_of_speech_from_folkets_class("pm"), Some(PartOfSpeech::Propn));
        assert_eq!(part_of_speech_from_folkets_class("vb"), Some(PartOfSpeech::Verb));
        assert_eq!(part_of_speech_from_folkets_class("hjälpverb"), Some(PartOfSpeech::Aux));
        assert_eq!(part_of_speech_from_folkets_class("jj"), Some(PartOfSpeech::Adj));
        assert_eq!(part_of_speech_from_folkets_class("pc"), Some(PartOfSpeech::Adj));
        assert_eq!(part_of_speech_from_folkets_class("ab"), Some(PartOfSpeech::Adv));
        assert_eq!(part_of_speech_from_folkets_class("pp"), Some(PartOfSpeech::Adp));
        assert_eq!(part_of_speech_from_folkets_class("kn"), Some(PartOfSpeech::Cconj));
        assert_eq!(part_of_speech_from_folkets_class("sn"), Some(PartOfSpeech::Sconj));
        assert_eq!(part_of_speech_from_folkets_class("ps"), Some(PartOfSpeech::Det));
    }

    #[test]
    fn folkets_non_pos_markers_are_none() { ["abbrev", "prefix", "suffix", "latin", ""].into_iter().for_each(|class| assert_eq!(part_of_speech_from_folkets_class(class), None, "{class}")); }

    #[test]
    fn folkets_multi_class_resolution() {
        assert_eq!(unambiguous_folkets_part_of_speech("nn, abbrev"), Some(PartOfSpeech::Noun));
        assert_eq!(unambiguous_folkets_part_of_speech("jj, pc"), Some(PartOfSpeech::Adj));
        assert_eq!(unambiguous_folkets_part_of_speech("ab, jj"), None);
        assert_eq!(unambiguous_folkets_part_of_speech("nn, vb"), None);
        assert_eq!(unambiguous_folkets_part_of_speech("jj, nn, ab"), None);
        assert_eq!(unambiguous_folkets_part_of_speech("abbrev"), None);
    }

}
