//! AUTO-3: R25's review ladder as a pure state machine. Sizes climb Easy -> Medium -> Hard ->
//! Opus -> Fable; each try is a review or a check of a fix; no I/O, so no stubs.

use switchboard_core::autopilot::review::{
    first_rung, step, Outcome, ReviewState, Rung, SMALL_FIX_LOC,
};

fn reviewing(rung: Rung, try_n: u8) -> ReviewState {
    ReviewState::Reviewing { rung, try_n }
}

fn checking(rung: Rung, try_n: u8) -> ReviewState {
    ReviewState::CheckingFix { rung, try_n }
}

fn fixed(loc: u32, docs_or_tests_only: bool) -> Outcome {
    Outcome::Fixed {
        loc,
        docs_or_tests_only,
    }
}

#[test]
fn first_rung_maps_the_three_sizes_in_any_case() {
    assert_eq!(first_rung("easy"), Rung::Easy);
    assert_eq!(first_rung("EASY"), Rung::Easy);
    assert_eq!(first_rung("Medium"), Rung::Medium);
    assert_eq!(first_rung("mEdIuM"), Rung::Medium);
    assert_eq!(first_rung("hard"), Rung::Hard);
    assert_eq!(first_rung("HARD"), Rung::Hard);
}

#[test]
fn first_rung_sends_unknown_text_to_hard() {
    assert_eq!(first_rung(""), Rung::Hard);
    assert_eq!(first_rung("huge"), Rung::Hard);
    assert_eq!(first_rung(" easy"), Rung::Hard);
}

#[test]
fn pass_and_small_or_docs_fixes_pass_at_try_one() {
    assert_eq!(
        step(&reviewing(Rung::Easy, 1), &Outcome::Pass),
        ReviewState::Passed
    );
    assert_eq!(
        step(&reviewing(Rung::Easy, 1), &fixed(30, false)),
        ReviewState::Passed
    );
    assert_eq!(
        step(&reviewing(Rung::Easy, 1), &fixed(200, true)),
        ReviewState::Passed
    );
    assert_eq!(SMALL_FIX_LOC, 30);
}

#[test]
fn a_larger_code_fix_is_checked_at_the_same_rung_and_try() {
    assert_eq!(
        step(&reviewing(Rung::Easy, 1), &fixed(31, false)),
        checking(Rung::Easy, 1)
    );
    assert_eq!(
        step(&reviewing(Rung::Medium, 2), &fixed(31, false)),
        checking(Rung::Medium, 2)
    );
}

#[test]
fn checking_fix_passes_on_pass() {
    assert_eq!(
        step(&checking(Rung::Hard, 1), &Outcome::Pass),
        ReviewState::Passed
    );
}

#[test]
fn checking_fix_failure_is_a_failed_try() {
    assert_eq!(
        step(&checking(Rung::Easy, 2), &Outcome::Failed),
        reviewing(Rung::Easy, 3)
    );
    assert_eq!(
        step(&checking(Rung::Easy, 2), &fixed(5, false)),
        reviewing(Rung::Easy, 3)
    );
}

#[test]
fn reviewing_failure_is_a_failed_try() {
    assert_eq!(
        step(&reviewing(Rung::Medium, 1), &Outcome::Failed),
        reviewing(Rung::Medium, 2)
    );
}

#[test]
fn three_failed_tries_at_easy_climb_to_medium() {
    let s1 = step(&reviewing(Rung::Easy, 1), &Outcome::Failed);
    assert_eq!(s1, reviewing(Rung::Easy, 2));
    let s2 = step(&s1, &Outcome::Failed);
    assert_eq!(s2, reviewing(Rung::Easy, 3));
    assert_eq!(step(&s2, &Outcome::Failed), reviewing(Rung::Medium, 1));
}

#[test]
fn three_failed_tries_climb_through_hard_and_opus() {
    assert_eq!(
        step(&reviewing(Rung::Medium, 3), &Outcome::Failed),
        reviewing(Rung::Hard, 1)
    );
    assert_eq!(
        step(&reviewing(Rung::Hard, 3), &Outcome::Failed),
        reviewing(Rung::Opus, 1)
    );
}

#[test]
fn three_failed_tries_at_opus_reach_fable_as_at_fable() {
    let s = step(&reviewing(Rung::Opus, 3), &Outcome::Failed);
    assert_eq!(s, ReviewState::AtFable);
}

#[test]
fn a_failed_try_at_fable_is_at_fable() {
    assert_eq!(
        step(&reviewing(Rung::Fable, 1), &Outcome::Failed),
        ReviewState::AtFable
    );
}

#[test]
fn needs_me_stops_both_live_states() {
    assert_eq!(
        step(&reviewing(Rung::Easy, 1), &Outcome::NeedsMe),
        ReviewState::NeedsMe
    );
    assert_eq!(
        step(&checking(Rung::Opus, 2), &Outcome::NeedsMe),
        ReviewState::NeedsMe
    );
}

#[test]
fn final_states_stay_put_for_any_outcome() {
    let outcomes = [
        Outcome::Pass,
        fixed(1, true),
        fixed(100, false),
        Outcome::Failed,
        Outcome::NeedsMe,
    ];
    let finals = [
        ReviewState::Passed,
        ReviewState::NeedsMe,
        ReviewState::AtFable,
    ];
    for state in &finals {
        for outcome in &outcomes {
            assert_eq!(&step(state, outcome), state);
        }
    }
}
