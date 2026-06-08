use std::ops::Range;

/// Unified DOM node - can be element or text
#[derive(Clone, Debug)]
pub enum DomNode {
    Element(DomElement),
    Text(DomText),
}

/// DOM element node - pure document structure, no layout decisions
#[derive(Clone, Debug)]
pub struct DomElement {
    // Element identity
    pub tag: u16,            // interned tag name
    pub id: Option<u16>,     // interned id
    pub classes: Range<u16>, // range into Document.class_list
    pub namespace: Option<u16>,

    // Tree structure
    pub parent: Option<u32>, // parent node index
    pub children: Vec<u32>,  // child node indices

    // Specific attributes needed
    pub href: Option<u16>,   // For links
    pub src: Option<u16>,    // For images
    pub width: Option<u32>,  // For images
    pub height: Option<u32>, // For images
    pub image_idx: Option<u32>,
    pub attributes: Vec<DomAttribute>,

    // Style reference (filled during style resolution)
    pub style: Option<super::StyleIndices>,
}

#[derive(Clone, Debug)]
pub struct DomAttribute {
    pub name: u16,
    pub local_name: u16,
    pub namespace: Option<u16>,
    pub value: u16,
}

/// DOM text node
#[derive(Clone, Debug)]
pub struct DomText {
    pub text: String,
    pub parent: Option<u32>, // parent node index
}

impl DomElement {
    pub fn new(tag: u16, id: Option<u16>, classes: Range<u16>, namespace: Option<u16>, parent: Option<u32>, href: Option<u16>, src: Option<u16>) -> Self {
        Self { tag, id, classes, namespace, parent, children: Vec::new(), href, src, width: None, height: None, image_idx: None, attributes: Vec::new(), style: None }
    }
}

impl DomText {
    pub fn new(text: String, parent: Option<u32>) -> Self {
        Self { text, parent }
    }
}

impl DomNode {
    pub fn parent(&self) -> Option<u32> {
        match self {
            DomNode::Element(e) => e.parent,
            DomNode::Text(t) => t.parent,
        }
    }

    pub fn as_element(&self) -> Option<&DomElement> {
        match self {
            DomNode::Element(e) => Some(e),
            DomNode::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&DomText> {
        match self {
            DomNode::Element(_) => None,
            DomNode::Text(t) => Some(t),
        }
    }
}
