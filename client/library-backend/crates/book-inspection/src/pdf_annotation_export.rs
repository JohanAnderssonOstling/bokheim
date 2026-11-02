//! Reconcile Bokheim's marks in a PDF while preserving other annotations.
use crate::{MetadataResult, PdfSourceIdentity};
use book_model::{Annotation, AnnotationStyle};
use content_address::BookId;
use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
    dictionary,
};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, Write},
};

/// A parsed PDF retained while preparing and writing current annotations.
/// Reusing it avoids loading all document objects a second time.
pub struct PreparedPdfAnnotationExport {
    document: Document,
}
impl PreparedPdfAnnotationExport {
    pub fn source_identity(&self) -> Option<PdfSourceIdentity> {
        PdfSourceIdentity::from_document(&self.document)
    }

    pub fn write(self, writer: impl Write, source: PdfSourceIdentity, annotations: &[Annotation]) -> MetadataResult<()> {
        write_document_annotations(self.document, writer, source, annotations)
    }
}

fn write_document_annotations(mut document: Document, mut writer: impl Write, mut source: PdfSourceIdentity, annotations: &[Annotation]) -> MetadataResult<()> {
    if document.is_encrypted() {
        return Err("encrypted PDFs cannot be rewritten without their password".into());
    }
    // Older exports also leave unreferenced annotation objects after deletion.
    // Strip our historical payload from those objects before saving the PDF.
    for object in document.objects.values_mut() {
        if let Ok(mark) = object.as_dict_mut() {
            if mark.get(b"Type").and_then(Object::as_name).ok() == Some(b"Annot") {
                mark.remove(b"BokheimModel");
            }
        }
    }
    let pages = document.get_pages().into_values().collect::<Vec<_>>();
    let mut old = HashMap::new();
    let mut retained = Vec::new();
    for (page_index, &page) in pages.iter().enumerate() {
        let entries = match document.get_dictionary(page)?.get(b"Annots") {
            Ok(value) => document.dereference(value)?.1.as_array()?.clone(),
            Err(_) => Vec::new(),
        };
        let mut other = Vec::new();
        for value in entries {
            let object = document.dereference(&value)?.1;
            let name = object.as_dict().ok().and_then(|dict| dict.get(b"NM").ok()).map(lopdf::decode_text_string).transpose()?;
            if let Some(name) = name.filter(|name| name.starts_with("bokheim:")) {
                // Only library history authorizes replacing or deleting an owned
                // mark. Preserve saved marks missing from this database as well.
                let known_id = &name["bokheim:".len()..];
                if !annotations.iter().any(|annotation| annotation.id == known_id) {
                    other.push(value);
                    continue;
                }
                let id = match value.as_reference() {
                    Ok(id) => id,
                    Err(_) => document.add_object(object.clone()),
                };
                old.insert(name, id);
            } else if let Some(id) = annotation_id(object.as_dict()?, page_index, source.book_id)? {
                if annotations.iter().any(|annotation| annotation.id == id) {
                    // Adopt a foreign original instead of adding a duplicate.
                    let object_id = match value.as_reference() {
                        Ok(object_id) => object_id,
                        Err(_) => document.add_object(object.clone()),
                    };
                    old.insert(format!("bokheim:{id}"), object_id);
                } else {
                    other.push(value);
                }
            } else {
                other.push(value);
            }
        }
        retained.push(other);
    }
    for annotation in annotations.iter().filter(|a| !a.deleted && !a.is_external_pdf_annotation()) {
        annotation.validate()?;
        if annotation.content_hash != source.book_id {
            return Err("annotation belongs to a different PDF".into());
        }
        let anchor = annotation.anchor.pdf_anchor().ok_or("PDF annotation has no page geometry")?;
        let index = anchor.page_index as usize;
        let page = *pages.get(index).ok_or("annotation page is outside this PDF")?;
        let geometry = Geometry::page(&document, page)?;
        let name = format!("bokheim:{}", annotation.id);
        let previous = old.get(&name).copied();
        if let Some(mut mark) = previous.and_then(|id| document.get_dictionary(id).ok()).filter(|mark| mark.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Text")).cloned() {
            // Preserve sticky-note icons, popups and appearances, rather than
            // turning a foreign sticky note into a highlight.
            if let Some(rect) = anchor.rects.first() {
                let a = geometry.point(rect.left, rect.top);
                let b = geometry.point(rect.left + rect.width, rect.top + rect.height);
                mark.set("Rect", vec![a[0].min(b[0]).into(), a[1].min(b[1]).into(), a[0].max(b[0]).into(), a[1].max(b[1]).into()]);
            }
            mark.set("P", page);
            mark.set("C", vec![Object::Real(((annotation.color >> 16) & 255) as f32 / 255.0), Object::Real(((annotation.color >> 8) & 255) as f32 / 255.0), Object::Real((annotation.color & 255) as f32 / 255.0)]);
            mark.set("NM", Object::string_literal(name));
            mark.set("Contents", pdf_text(&annotation.note));
            mark.remove(b"BokheimModel");
            let id = previous.expect("existing note");
            document.objects.insert(id, mark.into());
            retained[index].push(Object::Reference(id));
            continue;
        }
        if anchor.rects.is_empty() {
            return Err("PDF annotation has no rectangles".into());
        }
        let mut quads = Vec::new();
        let mut bounds = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        let mut operations = Vec::new();
        let rgb = [((annotation.color >> 16) & 255) as f32 / 255.0, ((annotation.color >> 8) & 255) as f32 / 255.0, (annotation.color & 255) as f32 / 255.0];
        operations.push(Operation::new("q", vec![]));
        operations.push(Operation::new("gs", vec![Object::Name(b"GS".to_vec())]));
        operations.push(Operation::new("rg", rgb.into_iter().map(Object::Real).collect()));
        operations.push(Operation::new("RG", rgb.into_iter().map(Object::Real).collect()));
        operations.push(Operation::new("w", vec![Object::Real(1.0)]));
        for rect in &anchor.rects {
            let [left, top, width, height] = [rect.left, rect.top, rect.width, rect.height];
            if ![left, top, width, height].iter().all(|n| n.is_finite()) || left < 0.0 || top < 0.0 || width <= 0.0 || height <= 0.0 || left + width > 1.00001 || top + height > 1.00001 {
                return Err("invalid normalized PDF annotation rectangle".into());
            }
            let points = [geometry.point(left, top), geometry.point(left + width, top), geometry.point(left, top + height), geometry.point(left + width, top + height)];
            for [x, y] in points {
                quads.extend([Object::Real(x), Object::Real(y)]);
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x);
                bounds[3] = bounds[3].max(y);
            }
            if annotation.style == AnnotationStyle::Highlight {
                operations.push(point_op("m", points[0]));
                for point in [points[1], points[3], points[2]] {
                    operations.push(point_op("l", point));
                }
                operations.push(Operation::new("h", vec![]));
                operations.push(Operation::new("f", vec![]));
            } else {
                let level = if annotation.style == AnnotationStyle::Strikethrough { top + height * 0.5 } else { top + height * 0.95 };
                operations.push(point_op("m", geometry.point(left, level)));
                if annotation.style == AnnotationStyle::Squiggly {
                    let count = ((width * geometry.display_width() / 2.0).ceil() as usize).clamp(2, 4096);
                    for step in 1..=count {
                        let v = if step % 2 == 0 { level } else { (level - height * 0.15).max(top) };
                        operations.push(point_op("l", geometry.point(left + width * step as f32 / count as f32, v)));
                    }
                } else {
                    operations.push(point_op("l", geometry.point(left + width, level)));
                }
                operations.push(Operation::new("S", vec![]));
            }
        }
        operations.push(Operation::new("Q", vec![]));
        let previous_appearance =
            previous.and_then(|id| document.get_dictionary(id).ok()).and_then(|dict| dict.get(b"AP").ok()).and_then(|value| value.as_dict().ok()).and_then(|dict| dict.get(b"N").ok()).and_then(|value| value.as_reference().ok());
        let appearance = Stream::new(
            dictionary! { "Type"=>"XObject","Subtype"=>"Form","BBox"=>bounds.into_iter().map(Object::Real).collect::<Vec<_>>(),"Resources"=>dictionary! {"ExtGState"=>dictionary! {"GS"=>dictionary! {"Type"=>"ExtGState","ca"=>if annotation.style==AnnotationStyle::Highlight {0.25}else{1.0},"CA"=>1.0,"BM"=>"Multiply"}}}},
            Content { operations }.encode()?,
        );
        let appearance = if let Some(id) = previous_appearance {
            document.objects.insert(id, appearance.into());
            id
        } else {
            document.add_object(appearance)
        };
        let kind = match annotation.style {
            AnnotationStyle::Highlight => "Highlight",
            AnnotationStyle::Underline => "Underline",
            AnnotationStyle::Squiggly => "Squiggly",
            AnnotationStyle::Strikethrough => "StrikeOut",
        };
        let generated = dictionary! {"Type"=>"Annot","Subtype"=>kind,"P"=>page,"Rect"=>bounds.into_iter().map(Object::Real).collect::<Vec<_>>(),"QuadPoints"=>quads,"C"=>rgb.into_iter().map(Object::Real).collect::<Vec<_>>(),"CA"=>1.0,"F"=>4,"NM"=>Object::string_literal(name),"Contents"=>pdf_text(&annotation.note),"T"=>Object::string_literal("Bokheim"),"AP"=>dictionary! {"N"=>appearance}};
        let mut mark = previous.and_then(|id| document.get_dictionary(id).ok()).cloned().unwrap_or_default();
        mark.remove(b"BokheimModel");
        for (key, value) in generated.iter() {
            mark.set(key.clone(), value.clone());
        }
        let id = if let Some(id) = previous {
            document.objects.insert(id, mark.into());
            id
        } else {
            document.add_object(mark)
        };
        retained[index].push(Object::Reference(id));
    }
    for (page, annotations) in pages.into_iter().zip(retained) {
        document.get_dictionary_mut(page)?.set("Annots", annotations);
    }
    // Recovered provenance remains a hint until server confirmation. Do not
    // erase it merely because this device has not confirmed that revision yet.
    if source.canonical_hash.is_none() {
        if let Some(previous) = PdfSourceIdentity::from_document(&document).filter(|previous| previous.book_id == source.book_id) {
            source.canonical_hash = previous.canonical_hash;
        }
    }
    source.write(&mut document)?;
    document.save_to(&mut writer)?;
    Ok(())
}
fn point_op(operator: &str, point: [f32; 2]) -> Operation {
    Operation::new(operator, point.into_iter().map(Object::Real).collect())
}
fn pdf_text(text: &str) -> Object {
    Object::string_literal([vec![0xfe, 0xff], text.encode_utf16().flat_map(u16::to_be_bytes).collect()].concat())
}

struct Geometry {
    bounds: [f32; 4],
    rotation: i64,
}
impl Geometry {
    fn page(document: &Document, page: ObjectId) -> MetadataResult<Self> {
        let inherited = |key: &[u8]| -> MetadataResult<Option<Object>> {
            let mut current = page;
            let mut visited = HashSet::new();
            while visited.insert(current) {
                let dict = document.get_dictionary(current)?;
                if let Ok(value) = dict.get(key) {
                    return Ok(Some(document.dereference(value)?.1.clone()));
                }
                match dict.get(b"Parent").and_then(Object::as_reference) {
                    Ok(parent) => current = parent,
                    Err(_) => return Ok(None),
                }
            }
            Err("cyclic PDF page inheritance".into())
        };
        let media = inherited(b"MediaBox")?.ok_or("PDF page has no MediaBox")?;
        let media = coordinates(&media)?;
        let mut bounds = inherited(b"CropBox")?.map(|crop| coordinates(&crop)).transpose()?.unwrap_or(media);
        bounds = [bounds[0].max(media[0]), bounds[1].max(media[1]), bounds[2].min(media[2]), bounds[3].min(media[3])];
        if !bounds.iter().all(|value| value.is_finite()) || bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return Err("invalid PDF page bounds".into());
        }
        let rotation = inherited(b"Rotate")?.map(|value| value.as_i64()).transpose()?.unwrap_or(0).rem_euclid(360);
        if !matches!(rotation, 0 | 90 | 180 | 270) {
            return Err("invalid PDF page rotation".into());
        }
        Ok(Self { bounds, rotation })
    }
    fn point(&self, u: f32, v: f32) -> [f32; 2] {
        let [x, y, right, top] = self.bounds;
        let w = right - x;
        let h = top - y;
        match self.rotation {
            90 => [x + v * w, y + u * h],
            180 => [x + (1.0 - u) * w, y + v * h],
            270 => [x + (1.0 - v) * w, y + (1.0 - u) * h],
            _ => [x + u * w, y + (1.0 - v) * h],
        }
    }
    fn display_width(&self) -> f32 {
        if matches!(self.rotation, 90 | 270) { self.bounds[3] - self.bounds[1] } else { self.bounds[2] - self.bounds[0] }
    }
}
fn coordinates(value: &Object) -> MetadataResult<[f32; 4]> {
    let array = value.as_array()?;
    if array.len() != 4 {
        return Err("invalid PDF page box".into());
    }
    let values = array
        .iter()
        .map(|value| match value {
            Object::Integer(n) => Ok(*n as f32),
            Object::Real(n) => Ok(*n),
            _ => Err("non-numeric PDF page box".into()),
        })
        .collect::<MetadataResult<Vec<_>>>()?;
    Ok(values.try_into().expect("four coordinates"))
}

/// Parse once for publication. Annotation discovery belongs to reader rendering;
/// rewriting retains foreign marks without importing them into the library.
pub fn prepare_pdf_annotation_export(mut reader: impl Read + Seek + Send + 'static, book: BookId, known: &[Annotation]) -> MetadataResult<PreparedPdfAnnotationExport> {
    let document = Document::load_from(&mut reader)?;
    if document.is_encrypted() {
        return Err("encrypted PDFs cannot be rewritten without their password".into());
    }
    if PdfSourceIdentity::from_document(&document).is_some_and(|source| source.book_id != book) {
        return Err("PDF source identity belongs to another book".into());
    }
    if known.iter().any(|annotation| annotation.content_hash != book) {
        return Err("PDF annotations belong to another book".into());
    }
    Ok(PreparedPdfAnnotationExport { document })
}

fn annotation_id(mark: &Dictionary, page: usize, book: BookId) -> MetadataResult<Option<String>> {
    if !matches!(mark.get(b"Subtype").and_then(Object::as_name).ok(), Some(b"Highlight" | b"Underline" | b"Squiggly" | b"StrikeOut" | b"Text")) {
        return Ok(None);
    }
    let name = mark.get(b"NM").ok().map(lopdf::decode_text_string).transpose()?;
    if let Some(id) = name.as_deref().and_then(|name| name.strip_prefix("bokheim:")) {
        return Ok(Some(id.to_owned()));
    }
    let mut digest = blake3::Hasher::new();
    digest.update(book.as_bytes());
    if let Some(name) = name.filter(|name| !name.is_empty()) {
        digest.update(b"name\0");
        digest.update(name.as_bytes());
    } else {
        digest.update(format!("{page}:{:?}:{:?}:{:?}", mark.get(b"Subtype")?, mark.get(b"Rect")?, mark.get(b"QuadPoints").ok()).as_bytes());
    }
    Ok(Some(format!("pdf-import:{}", digest.finalize().to_hex())))
}
