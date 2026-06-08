use lightningcss::rules::CssRule;
use lightningcss::selector::{Component, Selector};
use lightningcss::stylesheet::StyleSheet;
use rustc_data_structures::fx::FxHashMap;
use std::collections::HashSet;

// ============================================================================
// Bloom Filter for fast ancestor rejection
// ============================================================================

#[derive(Clone, Copy, Default)]
pub struct AncestorFilter {
    bits: u64,
}

impl AncestorFilter {
    pub fn new() -> Self {
        Self { bits: 0 }
    }

    #[inline]
    fn hash(s: &str) -> u64 {
        let h = s.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        1u64 << (h % 64)
    }

    #[inline]
    pub fn insert(&mut self, s: &str) {
        self.bits |= Self::hash(s);
    }

    #[inline]
    pub fn might_contain(&self, s: &str) -> bool {
        (self.bits & Self::hash(s)) != 0
    }

    /// Insert tag, id, and all classes from an element
    #[allow(dead_code)]
    pub fn insert_element(&mut self, tag: &str, id: Option<&str>, classes: Option<&str>) {
        self.insert(tag);
        if let Some(id) = id {
            self.insert(id);
        }
        if let Some(c) = classes {
            for class in c.split_whitespace() {
                self.insert(class);
            }
        }
    }
}

// ============================================================================
// Selector Index for O(1) candidate lookup
// ============================================================================

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RuleKey {
    pub stylesheet_idx: usize,
    pub rule_idx: usize,
    pub source_order: usize,
}

/// Index of rule keys, keyed by selector target (tag/class/id)
pub struct SelectorIndex {
    by_tag: FxHashMap<String, Vec<RuleKey>>,
    by_class: FxHashMap<String, Vec<RuleKey>>,
    by_id: FxHashMap<String, Vec<RuleKey>>,
    universal: Vec<RuleKey>, // *, [attr], :pseudo with no tag/class/id
}

impl SelectorIndex {
    pub fn new() -> Self {
        Self { by_tag: FxHashMap::default(), by_class: FxHashMap::default(), by_id: FxHashMap::default(), universal: Vec::new() }
    }

    /// Build index from a stylesheet
    #[allow(dead_code)]
    pub fn from_stylesheet(stylesheet: &StyleSheet) -> Self {
        Self::from_stylesheets(std::slice::from_ref(stylesheet))
    }

    /// Build index from stylesheets in cascade source order.
    pub fn from_stylesheets(stylesheets: &[StyleSheet]) -> Self {
        let mut index = Self::new();
        let mut source_order = 0usize;
        for (stylesheet_idx, stylesheet) in stylesheets.iter().enumerate() {
            for (rule_idx, rule) in stylesheet.rules.0.iter().enumerate() {
                if let CssRule::Style(style_rule) = rule {
                    let key = RuleKey { stylesheet_idx, rule_idx, source_order };
                    for selector in &style_rule.selectors.0 {
                        index.add_selector(selector, key);
                    }
                    source_order += 1;
                }
            }
        }
        index
    }

    /// Add a selector to the index
    pub fn add_selector(&mut self, selector: &Selector, key: RuleKey) {
        let mut has_tag = false;
        let mut has_class = false;
        let mut has_id = false;

        // Only look at rightmost (target) sequence
        let iter = selector.iter();
        for component in iter {
            match component {
                Component::LocalName(name) => {
                    let tag = name.lower_name.0.as_ref().to_string();
                    self.by_tag.entry(tag).or_default().push(key);
                    has_tag = true;
                }
                Component::Class(class) => {
                    let class_str = class.0.as_ref().to_string();
                    self.by_class.entry(class_str).or_default().push(key);
                    has_class = true;
                }
                Component::ID(id) => {
                    let id_str = id.0.as_ref().to_string();
                    self.by_id.entry(id_str).or_default().push(key);
                    has_id = true;
                }
                _ => {}
            }
        }

        // If no specific target, it's universal (matches anything)
        if !has_tag && !has_class && !has_id {
            self.universal.push(key);
        }
    }

    /// Get candidate rule indices for an element
    pub fn get_candidates(&self, tag: &str, id: Option<&str>, classes: Option<&str>) -> Vec<RuleKey> {
        let mut candidates: Vec<RuleKey> = Vec::new();
        let mut seen = HashSet::new();

        // Add by tag
        if let Some(indices) = self.by_tag.get(tag) {
            for &idx in indices {
                if seen.insert(idx) {
                    candidates.push(idx);
                }
            }
        }

        // Add by id
        if let Some(id) = id {
            if let Some(indices) = self.by_id.get(id) {
                for &idx in indices {
                    if seen.insert(idx) {
                        candidates.push(idx);
                    }
                }
            }
        }

        // Add by class
        if let Some(classes) = classes {
            for class in classes.split_whitespace() {
                if let Some(indices) = self.by_class.get(class) {
                    for &idx in indices {
                        if seen.insert(idx) {
                            candidates.push(idx);
                        }
                    }
                }
            }
        }

        // Always include universal selectors
        for &idx in &self.universal {
            if seen.insert(idx) {
                candidates.push(idx);
            }
        }

        candidates
    }
}
