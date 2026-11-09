//! FD-6, FD-7: the packet a chat carries when it moves to another model.

/// One chat turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// `user` or `assistant`.
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// The rolling summary, rewritten by the outgoing model at each handoff, never stacked.
    pub summary: String,
    /// The last 6 turns, word for word, oldest first.
    pub last_turns: Vec<Turn>,
    /// Every file link gathered so far, first-seen order, no repeats.
    pub links: Vec<String>,
}

/// FD-6: how many turns go word for word.
pub const LAST_TURNS: usize = 6;

/// FD-6: build the packet; never the full transcript.
pub fn build(summary: &str, turns: &[Turn], links: &[String]) -> Packet {
    let start = turns.len().saturating_sub(LAST_TURNS);
    let mut seen = std::collections::HashSet::new();
    let unique_links: Vec<String> = links
        .iter()
        .filter(|link| seen.insert(link.as_str()))
        .cloned()
        .collect();
    Packet {
        summary: summary.to_string(),
        last_turns: turns[start..].to_vec(),
        links: unique_links,
    }
}

/// FD-7: the file links and open questions the summary leaves out (empty when it names all).
/// A link or question counts as named when its exact text appears in the summary.
pub fn missing_from_summary(
    summary: &str,
    links: &[String],
    open_questions: &[String],
) -> Vec<String> {
    todo!("FD-7")
}
