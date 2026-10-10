//! Wire invariants shared by worker adapters, independent of browser values.
pub fn request_id(value: f64) -> Result<u32, String> {
    if !value.is_finite() || value < 1.0 || value > u32::MAX as f64 || value.fract() != 0.0 {
        return Err("Invalid request id".into());
    }
    Ok(value as u32)
}

/// A reply carries exactly one success payload or error, including empty bytes.
pub fn reply<T>(payload: Option<T>, error: Option<String>) -> Result<Result<T, String>, String> {
    match (payload, error) {
        (Some(payload), None) => Ok(Ok(payload)),
        (None, Some(error)) => Ok(Err(error)),
        (None, None) => Err("Missing reply payload".into()),
        (Some(_), Some(_)) => Err("Reply contains both bytes and error".into()),
    }
}


/// Millisecond timers must fit the wire format and cannot wrap or panic.
pub fn milliseconds(value: f64) -> Result<u32, String> {
    if !value.is_finite() || !(0.0..=u32::MAX as f64).contains(&value) {
        return Err("Invalid duration".into());
    }
    Ok(value as u32)
}
