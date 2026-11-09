//! Tests for FD-4 (`sample_size` gives N from the rows logged for a kind: 6 plus one per 20
//! rows, capped at 30; `learned_pick` gives the cheapest model with at least 80% passes over its
//! last N rated chat rows, where implicit and ledger rows do not count). Rows are built in code;
//! no service is called.

use switchboard_core::frontdoor::router::{learned_pick, sample_size};
use switchboard_core::joblog::{JobRow, RowSource};

/// A rated chat row of kind "chat" from the gemini lane.
fn rated(model: &str, rating: &str, cost: Option<f64>) -> JobRow {
    JobRow {
        kind: Some("chat".to_string()),
        lane: Some("gemini".to_string()),
        model: Some(model.to_string()),
        rating: Some(rating.to_string()),
        cost_usd: cost,
        source: RowSource::Chat,
        ..JobRow::default()
    }
}

fn passes(model: &str, count: usize) -> Vec<JobRow> {
    (0..count)
        .map(|_| rated(model, "pass", Some(0.01)))
        .collect()
}

/// Logged chat rows with no rating: ledger rows, so they count toward N only.
fn unrated_ledger(count: usize) -> Vec<JobRow> {
    (0..count)
        .map(|_| JobRow {
            kind: Some("chat".to_string()),
            lane: Some("gemini".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            ..JobRow::default()
        })
        .collect()
}

#[test]
fn sample_size_starts_at_six_and_rises_per_twenty_rows() {
    assert_eq!(sample_size(0), 6);
    assert_eq!(sample_size(5), 6);
    assert_eq!(sample_size(19), 6);
    assert_eq!(sample_size(20), 7);
    assert_eq!(sample_size(26), 7);
    assert_eq!(sample_size(240), 18);
}

#[test]
fn sample_size_caps_at_thirty() {
    assert_eq!(sample_size(480), 30);
    assert_eq!(sample_size(600), 30);
    assert_eq!(sample_size(100_000), 30);
}

#[test]
fn five_rated_rows_is_none() {
    let rows = passes("gemini-2.5-flash", 5);
    assert_eq!(learned_pick("chat", &rows), None);
}

#[test]
fn six_rated_rows_at_full_pass_qualifies() {
    let rows = passes("gemini-2.5-flash", 6);
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn twenty_six_logged_rows_need_seven_rated() {
    let mut six = passes("gemini-2.5-flash", 6);
    six.extend(unrated_ledger(20));
    assert_eq!(learned_pick("chat", &six), None);

    let mut seven = passes("gemini-2.5-flash", 7);
    seven.extend(unrated_ledger(19));
    assert_eq!(
        learned_pick("chat", &seven),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn six_hundred_logged_rows_need_thirty_rated() {
    let mut twenty_nine = passes("gemini-2.5-flash", 29);
    twenty_nine.extend(unrated_ledger(571));
    assert_eq!(learned_pick("chat", &twenty_nine), None);

    let mut thirty = passes("gemini-2.5-flash", 30);
    thirty.extend(unrated_ledger(570));
    assert_eq!(
        learned_pick("chat", &thirty),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn implicit_rows_count_toward_logged_rows_but_not_as_rated() {
    // 13 ledger rows, one unrated implicit row and 6 rated passes: 20 logged, so N = 7. Without
    // the implicit row the total would be 19 and N = 6, which would qualify.
    let mut rows = unrated_ledger(13);
    rows.push(JobRow {
        kind: Some("chat".to_string()),
        lane: Some("gemini".to_string()),
        model: Some("gemini-2.5-flash".to_string()),
        implicit: true,
        ..JobRow::default()
    });
    rows.extend(passes("gemini-2.5-flash", 6));
    assert_eq!(learned_pick("chat", &rows), None);
}

#[test]
fn four_of_six_does_not_qualify_five_of_six_does() {
    let mut four = passes("gemini-2.5-flash", 4);
    four.push(rated("gemini-2.5-flash", "down", Some(0.01)));
    four.push(rated("gemini-2.5-flash", "down", Some(0.01)));
    assert_eq!(learned_pick("chat", &four), None);

    let mut five = passes("gemini-2.5-flash", 5);
    five.push(rated("gemini-2.5-flash", "down", Some(0.01)));
    assert_eq!(
        learned_pick("chat", &five),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn up_counts_as_a_pass() {
    let rows: Vec<JobRow> = (0..6)
        .map(|_| rated("gemini-2.5-flash", "up", Some(0.01)))
        .collect();
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn only_the_last_n_rated_rows_count() {
    // Eight rated rows: the two old downs fall outside the last six.
    let mut rows = vec![
        rated("gemini-2.5-flash", "down", Some(0.01)),
        rated("gemini-2.5-flash", "down", Some(0.01)),
    ];
    rows.extend(passes("gemini-2.5-flash", 6));
    assert!(learned_pick("chat", &rows).is_some());

    // Six passes then two downs: the last six hold four passes, so it does not qualify.
    let mut recent_downs = passes("gemini-2.5-flash", 6);
    recent_downs.push(rated("gemini-2.5-flash", "down", Some(0.01)));
    recent_downs.push(rated("gemini-2.5-flash", "down", Some(0.01)));
    assert_eq!(learned_pick("chat", &recent_downs), None);
}

#[test]
fn implicit_rows_are_not_counted_as_rated() {
    let rows: Vec<JobRow> = (0..6)
        .map(|_| JobRow {
            implicit: true,
            ..rated("gemini-2.5-flash", "pass", Some(0.01))
        })
        .collect();
    assert_eq!(learned_pick("chat", &rows), None);
}

#[test]
fn other_kinds_are_not_counted() {
    // Five chat rows and six code rows: chat has 5 logged, so N = 6 and only 5 are rated.
    let mut rows = passes("gemini-2.5-flash", 5);
    rows.extend((0..6).map(|_| JobRow {
        kind: Some("code".to_string()),
        ..rated("gemini-2.5-flash", "pass", Some(0.01))
    }));
    assert_eq!(learned_pick("chat", &rows), None);
    // The six code rows are their own kind: N = 6 and all six pass.
    assert_eq!(
        learned_pick("code", &rows),
        Some(("gemini".to_string(), "gemini-2.5-flash".to_string()))
    );
}

#[test]
fn ledger_rows_are_not_counted_as_rated() {
    let rows: Vec<JobRow> = (0..6)
        .map(|_| JobRow {
            source: RowSource::Ledger,
            ..rated("gemini-2.5-flash", "pass", Some(0.01))
        })
        .collect();
    assert_eq!(learned_pick("chat", &rows), None);
}

#[test]
fn cheaper_model_wins_even_with_a_later_name() {
    let mut rows: Vec<JobRow> = (0..6)
        .map(|_| rated("a-model", "pass", Some(0.02)))
        .collect();
    rows.extend(passes("z-model", 6));
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("gemini".to_string(), "z-model".to_string()))
    );
}

#[test]
fn equal_cost_goes_to_the_smaller_model_name() {
    let mut rows = passes("z-model", 6);
    rows.extend(passes("a-model", 6));
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("gemini".to_string(), "a-model".to_string()))
    );
}

#[test]
fn no_cost_counts_as_zero() {
    let mut rows: Vec<JobRow> = (0..6).map(|_| rated("z-model", "pass", None)).collect();
    rows.extend(passes("a-model", 6));
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("gemini".to_string(), "z-model".to_string()))
    );
}

#[test]
fn returned_service_is_the_rows_lane() {
    let rows: Vec<JobRow> = (0..6)
        .map(|_| JobRow {
            lane: Some("deepseek".to_string()),
            ..rated("deepseek-chat", "pass", Some(0.001))
        })
        .collect();
    assert_eq!(
        learned_pick("chat", &rows),
        Some(("deepseek".to_string(), "deepseek-chat".to_string()))
    );
}
