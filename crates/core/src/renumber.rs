//! BAK-3 (#365): a restore gives issues new numbers, so the `#12` in a restored body or comment
//! is rewritten to the number that issue has now. Only the plain `#N` form is read: a number
//! behind a repo name (`owner/name#12`), in a URL, in a fenced code block or in a code span on
//! one line is left as it is, and so is a number nothing is known about.

/// What became of the issue a `#N` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Now {
    /// It has this number now.
    Is(u64),
    /// It is one of the backup's issues but the repo does not have it yet.
    NotYet,
    /// Not one of the backup's issues (a PR, or a number that never was): left as it is.
    Unknown,
}

/// A body after `renumber`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renumbered {
    pub text: String,
    /// True when a `#N` names an issue that is `Now::NotYet`: the text is not final.
    pub waiting: bool,
}

/// `body` with every plain `#N` given the number `now` answers for `N`.
pub fn renumber(body: &str, now: &dyn Fn(u64) -> Now) -> Renumbered {
    let mut out = Renumbered {
        text: String::with_capacity(body.len()),
        waiting: false,
    };
    // The fence a code block was opened with: its character and how many of them.
    let mut fence: Option<(char, usize)> = None;
    for line in body.split_inclusive('\n') {
        let opened = fence_of(line);
        match (fence, opened) {
            (None, Some(open)) => {
                fence = Some(open);
                out.text.push_str(line);
            }
            (Some((ch, len)), Some((c, n))) if c == ch && n >= len && closes(line, c) => {
                fence = None;
                out.text.push_str(line);
            }
            (Some(_), _) => out.text.push_str(line),
            (None, None) => renumber_line(line, now, &mut out),
        }
    }
    out
}

/// The fence a line starts with: three or more backticks or tildes after at most three spaces.
fn fence_of(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = rest.chars().take_while(|c| *c == ch).count();
    (len >= 3).then_some((ch, len))
}

/// A closing fence has nothing but white space after its run of `ch`.
fn closes(line: &str, ch: char) -> bool {
    line.trim_start_matches(' ')
        .trim_start_matches(ch)
        .trim()
        .is_empty()
}

/// One line outside a fenced block: code spans are copied, each plain `#N` is looked up.
fn renumber_line(line: &str, now: &dyn Fn(u64) -> Now, out: &mut Renumbered) {
    let bytes = line.as_bytes();
    let mut at = 0;
    // Where the text not yet copied starts.
    let mut from = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'`' => {
                let run = bytes[at..].iter().take_while(|b| **b == b'`').count();
                at = match closing_run(bytes, at + run, run) {
                    Some(end) => end,
                    None => at + run,
                };
            }
            b'#' => {
                let digits = bytes[at + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let end = at + 1 + digits;
                let before = line[..at].chars().next_back();
                let after = line[end..].chars().next();
                let plain = digits > 0
                    && !before.is_some_and(|c| c.is_alphanumeric() || "_/#&=".contains(c))
                    && !after.is_some_and(|c| c.is_alphanumeric() || c == '_');
                let number = line[at + 1..end].parse::<u64>().ok().filter(|_| plain);
                match number.map(now) {
                    Some(Now::Is(new)) => {
                        out.text.push_str(&line[from..at]);
                        out.text.push_str(&format!("#{new}"));
                        from = end;
                    }
                    Some(Now::NotYet) => out.waiting = true,
                    Some(Now::Unknown) | None => {}
                }
                at = end;
            }
            _ => at += 1,
        }
    }
    out.text.push_str(&line[from..]);
}

/// The index after the next run of exactly `run` backticks at or after `from`: the end of a
/// code span. `None` when the line has none, so the opening run is plain text.
fn closing_run(bytes: &[u8], from: usize, run: usize) -> Option<usize> {
    let mut at = from;
    while at < bytes.len() {
        if bytes[at] != b'`' {
            at += 1;
            continue;
        }
        let len = bytes[at..].iter().take_while(|b| **b == b'`').count();
        if len == run {
            return Some(at + len);
        }
        at += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 is 3 now, 1 stays 1, 9 is an issue not there yet, anything else is unknown.
    fn now(old: u64) -> Now {
        match old {
            5 => Now::Is(3),
            1 => Now::Is(1),
            12 => Now::Is(40),
            9 => Now::NotYet,
            _ => Now::Unknown,
        }
    }

    fn text(body: &str) -> String {
        renumber(body, &now).text
    }

    #[test]
    fn a_plain_number_gets_the_number_its_issue_has_now() {
        assert_eq!(text("see #5 and #1, not #7"), "see #3 and #1, not #7");
        assert_eq!(text("#5"), "#3");
        assert_eq!(text("(#5) [#12]: fixes #5."), "(#3) [#40]: fixes #3.");
        assert_eq!(
            text("é#5 ü #5"),
            "é#5 ü #3",
            "a letter before it is no reference"
        );
    }

    #[test]
    fn a_number_that_is_part_of_something_else_is_left() {
        for same in [
            "other/repo#5",
            "owner/demo#5",
            "https://example.invalid/page#5",
            "https://example.invalid/#5",
            "colour #5abc and #5_x",
            "&#5; ##5 a=#5",
            "#",
            "# 5",
        ] {
            assert_eq!(text(same), same);
        }
    }

    #[test]
    fn code_spans_and_fenced_blocks_are_left() {
        assert_eq!(text("`#5` and #5"), "`#5` and #3");
        assert_eq!(text("``a ` #5`` #5 `#5"), "``a ` #5`` #3 `#3");
        let block = "#5\n```sh\necho #5\n```\n#5\n~~~\n#5\n```\n#5\n~~~~\n#5\n";
        assert_eq!(
            text(block),
            "#3\n```sh\necho #5\n```\n#3\n~~~\n#5\n```\n#5\n~~~~\n#3\n"
        );
        assert_eq!(text("```\n#5"), "```\n#5", "an open fence runs to the end");
    }

    #[test]
    fn an_issue_that_is_not_there_yet_marks_the_text_as_waiting() {
        let got = renumber("#5 then #9", &now);
        assert_eq!(got.text, "#3 then #9");
        assert!(got.waiting);
        assert!(!renumber("#5 then `#9` and #7", &now).waiting);
    }

    #[test]
    fn line_ends_are_kept() {
        assert_eq!(text("a #5\r\nb #5\n\n"), "a #3\r\nb #3\n\n");
        assert_eq!(text(""), "");
    }
}
