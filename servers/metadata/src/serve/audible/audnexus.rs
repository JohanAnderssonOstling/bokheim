//! Audnexus normalizes Audible data; it is not independent recording evidence.
use super::*;

impl AudibleClient {
    pub(super) async fn audnexus_product(&self, region: &str, asin: &str, source: DiscoverySource) -> Result<Candidate, String> {
        self.audnexus_book(region, asin, source).await.map(|(candidate, _)| candidate)
    }

    pub(super) async fn audnexus_book(&self, region: &str, asin: &str, source: DiscoverySource) -> Result<(Candidate, Value), String> {
        let (url, bytes, cached) = self.audnexus_response(&format!("/books/{asin}"), region).await?;
        let value = serde_json::from_slice(&bytes).map_err(|error| format!("Invalid Audnexus response: {error}"))?;
        let candidate = parse_book(&value, region, asin, source)?;
        self.persist_audnexus(url, bytes, cached).await?;
        Ok((candidate, value))
    }

    pub(super) async fn audnexus_chapters(&self, candidate: &Candidate) -> Result<(Vec<AudiobookChapter>, u64), String> {
        let (url, bytes, cached) = self.audnexus_response(&format!("/books/{}/chapters", candidate.asin), &candidate.region).await?;
        let value = serde_json::from_slice(&bytes).map_err(|error| format!("Invalid Audnexus response: {error}"))?;
        let timeline = parse_timeline(&value, &candidate.region, &candidate.asin)?;
        self.persist_audnexus(url, bytes, cached).await?;
        Ok(timeline)
    }

    async fn audnexus_response(&self, path: &str, region: &str) -> Result<(String, Arc<Vec<u8>>, bool), String> {
        let mut url = reqwest::Url::parse(&format!("{}{path}", self.audnexus_origin)).map_err(|error| error.to_string())?;
        url.query_pairs_mut().append_pair("region", region);
        let key = url.to_string();
        if let Some(cache) = &self.persistent {
            if let Some(bytes) = cache.get(&key).await? {
                tracing::debug!(url = %key, "using persisted Audnexus response");
                return Ok((key, bytes, true));
            }
        }
        Ok((key, self.get(url).await?, false))
    }

    async fn persist_audnexus(&self, url: String, bytes: Arc<Vec<u8>>, cached: bool) -> Result<(), String> {
        if !cached {
            if let Some(cache) = &self.persistent {
                cache.put(url, bytes).await?;
            }
        }
        Ok(())
    }
}

fn identity(value: &Value, region: &str, asin: &str) -> Result<(), String> {
    if value.get("asin").and_then(Value::as_str) != Some(asin) || value.get("region").and_then(Value::as_str) != Some(region) {
        return Err("Audnexus returned a different ASIN or region".into());
    }
    Ok(())
}

fn parse_book(value: &Value, region: &str, asin: &str, source: DiscoverySource) -> Result<Candidate, String> {
    identity(value, region, asin)?;
    let mut normalized = value.clone();
    for (from, to) in [("publisherName", "publisher_name"), ("releaseDate", "release_date"), ("description", "merchandising_summary"), ("formatType", "format_type"), ("runtimeLengthMin", "runtime_length_min")] {
        if let Some(field) = value.get(from) {
            normalized[to] = field.clone();
        }
    }
    let mut candidate = parse_product(&normalized, region, source)?;
    candidate.metadata_provider = "audnexus".into();
    Ok(candidate)
}

fn parse_timeline(value: &Value, region: &str, asin: &str) -> Result<(Vec<AudiobookChapter>, u64), String> {
    identity(value, region, asin)?;
    if value.get("isAccurate").and_then(Value::as_bool) != Some(true) {
        return Err("Audnexus chapter timing is not verified accurate".into());
    }
    let duration = value.get("runtimeLengthMs").and_then(Value::as_u64).ok_or("Audnexus precise runtime missing")?;
    fn nodes(value: &Value, depth: usize, count: &mut usize) -> Result<Value, String> {
        if depth > 32 {
            return Err("Audnexus chapter tree exceeds depth limit".into());
        }
        let input = value.as_array().ok_or("Audnexus chapter list missing")?;
        let mut output = Vec::new();
        for node in input {
            *count += 1;
            if *count > 10_000 {
                return Err("Audnexus chapter tree exceeds size limit".into());
            }
            let mut normalized = serde_json::json!({
                "title": node.get("title"),
                "length_ms": node.get("lengthMs"),
                "start_offset_ms": node.get("startOffsetMs"),
            });
            if let Some(children) = node.get("chapters") {
                normalized["chapters"] = nodes(children, depth + 1, count)?;
            }
            output.push(normalized);
        }
        Ok(Value::Array(output))
    }
    let chapters = nodes(value.get("chapters").ok_or("Audnexus chapters missing")?, 0, &mut 0)?;
    parse_chapters(&serde_json::json!({"content_metadata":{"chapter_info":{
        "runtime_length_ms":duration, "chapters":chapters
    }}}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chapter_validation_rejects_inaccurate_partial_and_wrong_recording_data() {
        let mut value = json!({"asin":"B000000001","region":"us","isAccurate":true,"runtimeLengthMs":1000,
            "chapters":[{"title":"Opening","startOffsetMs":0,"lengthMs":500},
                        {"title":"Conclusion","startOffsetMs":500,"lengthMs":500}]});
        assert_eq!(parse_timeline(&value, "us", "B000000001").unwrap().1, 1000);
        assert!(parse_timeline(&value, "uk", "B000000001").is_err());
        value["isAccurate"] = json!(false);
        assert!(parse_timeline(&value, "us", "B000000001").is_err());
        value["isAccurate"] = json!(true);
        value["chapters"][1]["lengthMs"] = json!(400);
        assert!(parse_timeline(&value, "us", "B000000001").is_err());
        value["chapters"][1]["lengthMs"] = json!(500);
        value["chapters"][1]["startOffsetMs"] = json!(400);
        assert!(parse_timeline(&value, "us", "B000000001").is_err());
    }
}
