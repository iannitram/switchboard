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

/// Log the turn's rating row in the chat's thread in the app data folder, and return it. The
/// thread must exist, `turn_index` must name one of its turns (`ThreadStore::turns`), that
/// turn must be an answer, and `service` and `model` must be the ones that answered it; else it
/// is refused (#372). A second rating of a turn replaces the first (`ThreadStore::upsert`).
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
    let store = ThreadStore::new(data_dir);
    let turn = answer_turn(&store, handoff_id, turn_index)?;
    let (lane, answered_by) = (
        turn.lane.as_deref().unwrap_or(""),
        turn.model.as_deref().unwrap_or(""),
    );
    if lane != service || answered_by != model {
        return Err(CoreError::Refused(format!(
            "turn {turn_index} of thread {handoff_id} was answered by {lane} {answered_by}, not {service} {model}"
        )));
    }
    store.upsert(handoff_id, &row)?;
    Ok(row)
}

/// The answer a rating names: turn `turn_index` of thread `handoff_id` (`ThreadStore::turns`).
/// Refused when the thread does not exist, has no such turn, or the turn is not an answer.
fn answer_turn(
    store: &ThreadStore,
    handoff_id: &str,
    turn_index: u32,
) -> Result<JobRow, CoreError> {
    if !store.exists(handoff_id)? {
        return Err(CoreError::Refused(format!("no thread {handoff_id}")));
    }
    let Some(turn) = store
        .turns(handoff_id)?
        .into_iter()
        .nth(turn_index as usize)
    else {
        return Err(CoreError::Refused(format!(
            "no turn {turn_index} in thread {handoff_id}"
        )));
    };
    if turn.role.as_deref() != Some("assistant") {
        return Err(CoreError::Refused(format!(
            "turn {turn_index} of thread {handoff_id} is not an answer"
        )));
    }
    Ok(turn)
}

/// #384: `log_rating` for a caller that holds only the thread and the turn (the CLI's `rate`,
/// MCP's `rate`): the service, model and kind are read from the turn itself. The refusals are
/// `log_rating`'s: an unknown rating, no such thread, no such turn, a turn that is not an
/// answer. A refusal returns before any file is touched.
pub fn rate_turn(
    data_dir: &Path,
    handoff_id: &str,
    turn_index: u32,
    rating: &str,
) -> Result<JobRow, CoreError> {
    if thumbs_from_name(rating).is_none() {
        return Err(CoreError::Refused(format!("unknown rating: {rating}")));
    }
    let store = ThreadStore::new(data_dir);
    let turn = answer_turn(&store, handoff_id, turn_index)?;
    let row = rating_entry(
        handoff_id,
        turn_index,
        rating,
        turn.lane.as_deref().unwrap_or(""),
        turn.model.as_deref().unwrap_or(""),
        turn.kind.as_deref(),
    )?;
    store.upsert(handoff_id, &row)?;
    Ok(row)
}
