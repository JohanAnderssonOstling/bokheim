//! Structured range storage. Full boundary strings exist only at the API/editor boundary.
use crate::lcc::lcc_endpoint;
use crate::runtime_index::cutter_components;
use rusqlite::{params, Connection};

struct Boundary {
    letters: String,
    // NULL means the entire letter class: zero at the lower end, infinity at the upper end.
    number: Option<f64>,
    cutters: Vec<(String, String)>,
}

impl Boundary {
    fn parse(text: &str, upper: bool) -> Result<Self, String> {
        let endpoint = lcc_endpoint(text, upper).ok_or_else(|| format!("invalid stored LCC boundary {text}"))?;
        Ok(Self { number: (endpoint.code != endpoint.letters).then_some(endpoint.number), letters: endpoint.letters, cutters: cutter_components(&endpoint.remainder) })
    }

    fn format(&self) -> String {
        let mut text = self.letters.clone();
        if let Some(number) = self.number {
            text.push_str(&number.to_string());
        }
        for (letters, digits) in &self.cutters {
            text.push('.');
            text.push_str(letters);
            text.push_str(digits);
        }
        text
    }
}

pub fn insert_range(connection: &Connection, concept_id: i64, start: &str, end: &str) -> Result<(), String> {
    let start = Boundary::parse(start, false)?;
    let end = Boundary::parse(end, true)?;
    connection
        .prepare_cached("INSERT INTO lcc_range(concept_id,start_letters,start_number,start_cutters,end_letters,end_number,end_cutters) VALUES(?1,?2,?3,?4,?5,?6,?7)")
        .map_err(|e| e.to_string())?
        .execute(params![concept_id, start.letters, start.number, serde_json::to_string(&start.cutters).map_err(|e| e.to_string())?, end.letters, end.number, serde_json::to_string(&end.cutters).map_err(|e| e.to_string())?])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_ranges(connection: &Connection) -> Result<Vec<(i64, String, String)>, String> {
    let mut statement = connection
        .prepare("SELECT concept_id,start_letters,start_number,start_cutters,end_letters,end_number,end_cutters FROM lcc_range ORDER BY concept_id,start_letters,start_number,start_cutters,end_letters,end_number,end_cutters")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<f64>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, Option<f64>>(5)?, r.get::<_, String>(6)?)))
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for row in rows {
        let (id, sl, sn, sc, el, en, ec) = row.map_err(|e| e.to_string())?;
        let start = Boundary { letters: sl, number: sn, cutters: serde_json::from_str(&sc).map_err(|e| e.to_string())? };
        let end = Boundary { letters: el, number: en, cutters: serde_json::from_str(&ec).map_err(|e| e.to_string())? };
        let (start_text, end_text) = (start.format(), end.format());
        // Fail explicitly on malformed persisted structures as well as malformed text.
        for (boundary, text, upper) in [(&start, &start_text, false), (&end, &end_text, true)] {
            let parsed = Boundary::parse(text, upper)?;
            if boundary.letters != parsed.letters || boundary.number != parsed.number || boundary.cutters != parsed.cutters {
                return Err(format!("noncanonical structured LCC boundary for concept {id}"));
            }
        }
        result.push((id, start_text, end_text));
    }
    Ok(result)
}
