//! FD-9: a thumbs press on one answer, logged as its rating row in the chat's thread (FD-13).
//! The shell's `chat_rate` command is a thin call into `log_rating`.

use super::adapters::Answer;
use super::sticky::rating_row;
use super::thread::ThreadStore;
use crate::joblog::JobRow;
use crate::CoreError;
use std::path::Path;

/// The rating name the face sends, byte for byte: `up` or `down`; any other text is no rating.
pub fn thumbs_from_name(rating: &str) -> Option<bool> {
    match rating {
        "up" => Some(true),
        "down" => Some(false),
        _ => None,
    }
}

/// The rating row's id: the thread's handoff id and the turn's index.
pub fn rating_id(handoff_id: &str, turn_index: u32) -> String {
    format!("{handoff_id}#rating-{turn_index}")
}

/// Build the rating row for one answer (no file is touched): `rating_row` with the turn's id.
/// Refused when the rating is not `up` or `down`, or the handoff id, service or model is empty.
pub fn rating_entry(
    handoff_id: &str,
    turn_index: u32,
    rating: &str,
    service: &str,
    model: &str,
    kind: Option<&str>,
) -> Result<JobRow, CoreError> {
    let Some(thumbs_up) = thumbs_from_name(rating) else {
        return Err(CoreError::Refused(format!("unknown rating: {rating}")));
    };
    if handoff_id.is_empty() {
        return Err(CoreError::Refused("rating with no handoff id".to_string()));
    }
    if service.is_empty() {
        return Err(CoreError::Refused("rating with no service".to_string()));
    }
    if model.is_empty() {
        return Err(CoreError::Refused("rating with no model".to_string()));
    }
    let answer = Answer {
        text: String::new(),
        service: service.to_string(),
        model: model.to_string(),
        tokens_in: None,
        tokens_out: None,
        cost_usd: None,
        handoff: None,
    };
    Ok(JobRow {
        id: Some(rating_id(handoff_id, turn_index)),
        ..rating_row(&answer, kind, handoff_id, thumbs_up)
    })
}

/// Append the turn's rating row to the chat's thread in the app data folder, and return it.
/// A refusal returns before any file is touched.
pub fn log_rating(
    data_dir: &Path,
    handoff_id: &str,
    turn_index: u32,
    rating: &str,
    service: &str,
    model: &str,
    kind: Option<&str>,
) -> Result<JobRow, CoreError> {
    let row = rating_entry(handoff_id, turn_index, rating, service, model, kind)?;
    ThreadStore::new(data_dir).append(handoff_id, &row)?;
    Ok(row)
}
