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
    let last_turns = turns[start..].to_vec();
    let mut kept: Vec<String> = Vec::new();
    for link in links {
        if !kept.contains(link) {
            kept.push(link.clone());
        }
    }
    Packet {
        summary: summary.to_string(),
        last_turns,
        links: kept,
    }
}

/// FD-8: the text a new Claude session starts from: the summary, then the last turns as `user:` /
/// `assistant:` lines, then the links, one per line, under the headings `Summary`, `Last turns`,
/// `Links`. A packet with no links keeps the `Links` heading and has no link lines.
pub fn as_prompt(packet: &Packet) -> String {
    let turns: Vec<String> = packet
        .last_turns
        .iter()
        .map(|turn| format!("{}: {}", turn.role, turn.text))
        .collect();
    format!(
        "Summary\n{}\n\nLast turns\n{}\n\nLinks\n{}",
        packet.summary,
        turns.join("\n"),
        packet.links.join("\n"),
    )
}

/// FD-7: the file links and open questions the summary leaves out (empty when it names all).
/// A link or question counts as named when its exact text appears in the summary.
pub fn missing_from_summary(
    summary: &str,
    links: &[String],
    open_questions: &[String],
) -> Vec<String> {
    links
        .iter()
        .chain(open_questions.iter())
        .filter(|item| !summary.contains(item.as_str()))
        .cloned()
        .collect()
}

/// FD-7: the summary to use. Calls `ask_again` at most once, with the missing items, when `first`
/// leaves any out; the second answer is used as it comes (never a third ask). Returns the summary
/// and the items it still leaves out.
pub fn checked_summary(
    first: String,
    links: &[String],
    open_questions: &[String],
    ask_again: &mut dyn FnMut(&[String]) -> String,
) -> (String, Vec<String>) {
    let missing = missing_from_summary(&first, links, open_questions);
    if missing.is_empty() {
        return (first, missing);
    }
    let second = ask_again(&missing);
    let still_missing = missing_from_summary(&second, links, open_questions);
    (second, still_missing)
}
