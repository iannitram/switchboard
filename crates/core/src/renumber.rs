//! BAK-3 (#365): a restore gives issues new numbers, so the `#12` in a restored body or comment
//! is rewritten to the number that issue has now. The plain `#N` form is always read. A number
//! behind a repo name (`owner/name#12`) or in a full link
//! (`https://github.com/owner/name/issues/12`, also `/pull/12`) is read only when `owner/name`
//! is the repo being restored (`Refs`); behind any other repo name, in any other URL, in a
//! fenced code block or in a code span on one line it is left as it is, and so is a number
//! nothing is known about.

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

/// The repo a restore's text names itself by (#365): `owner/name#N` and full links to one of
/// `from` (the backup's repo, and the name GitHub answered for when it was renamed; compared
/// without case) are rewritten too, and name `to`, the repo the restore goes to, when that is
/// another one. A reference that already says `to` keeps its spelling.
#[derive(Debug, Clone, Copy)]
pub struct Refs<'a> {
    pub from: &'a [&'a str],
    pub to: &'a str,
}

impl Refs<'_> {
    /// How `owner/name` is written after the rewrite; `None` when it is not the restored repo.
    fn written<'t>(&'t self, named: &'t str) -> Option<&'t str> {
        if !self
            .from
            .iter()
            .any(|from| from.eq_ignore_ascii_case(named))
        {
            return None;
        }
        Some(if named.eq_ignore_ascii_case(self.to) {
            named
        } else {
            self.to
        })
    }
}

/// `body` with every plain `#N` given the number `now` answers for `N`.
pub fn renumber(body: &str, now: &dyn Fn(u64) -> Now) -> Renumbered {
    renumber_for(body, None, now)
}

/// `renumber`, and with `refs` also every `owner/name#N` and full issue or PR link that names
/// the restored repo. Each reference is read once: the text a rewrite wrote is never read again.
pub fn renumber_for(body: &str, refs: Option<&Refs<'_>>, now: &dyn Fn(u64) -> Now) -> Renumbered {
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
            (None, None) => renumber_line(line, refs, now, &mut out),
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

/// The host a full link starts with.
const LINK: &str = "https://github.com/";

/// True for a character a GitHub owner or repo name is made of.
fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')
}

/// A full issue or PR link at the start of `text`: its `owner/name`, where its number starts
/// and where it ends. `None` when `text` is not one, or its number runs into a letter.
fn link_at(text: &str) -> Option<(&str, usize, usize)> {
    let rest = text.strip_prefix(LINK)?;
    let bytes = rest.as_bytes();
    let owner = bytes.iter().take_while(|b| name_byte(**b)).count();
    if owner == 0 || bytes.get(owner) != Some(&b'/') {
        return None;
    }
    let name = bytes[owner + 1..]
        .iter()
        .take_while(|b| name_byte(**b))
        .count();
    let repo_end = owner + 1 + name;
    let kind = ["/issues/", "/pull/"]
        .into_iter()
        .find(|kind| rest[repo_end..].starts_with(kind))?;
    let start = repo_end + kind.len();
    let digits = bytes[start..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let end = start + digits;
    let after = rest[end..].chars().next();
    if name == 0 || digits == 0 || after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some((&rest[..repo_end], LINK.len() + start, LINK.len() + end))
}

/// Where the `owner/name` that ends at `end` starts, not before `from`: both parts made of name
/// characters, and nothing before the owner that makes it part of a longer path or word.
fn repo_before(line: &str, from: usize, end: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let back = |to: usize| {
        to - bytes[from..to]
            .iter()
            .rev()
            .take_while(|b| name_byte(**b))
            .count()
    };
    let name = back(end);
    if name == end || name == from || bytes[name - 1] != b'/' {
        return None;
    }
    let owner = back(name - 1);
    let before = line[..owner].chars().next_back();
    if owner == name - 1 || before.is_some_and(|c| c.is_alphanumeric() || "/@:#&=".contains(c)) {
        return None;
    }
    Some(owner)
}

/// One line outside a fenced block: code spans are copied, each plain `#N` is looked up, and
/// with `refs` each `owner/name#N` and full link that names the restored repo.
fn renumber_line(
    line: &str,
    refs: Option<&Refs<'_>>,
    now: &dyn Fn(u64) -> Now,
    out: &mut Renumbered,
) {
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
            b'h' if refs.is_some() && line[at..].starts_with(LINK) => {
                let found = link_at(&line[at..]).and_then(|(named, start, end)| {
                    let written = refs?.written(named)?;
                    let number = line[at + start..at + end].parse::<u64>().ok()?;
                    Some((named, written, start, end, number))
                });
                let Some((named, written, start, end, number)) = found else {
                    at += 1;
                    continue;
                };
                match now(number) {
                    Now::Is(new) => {
                        let kind = &line[at + LINK.len() + named.len()..at + start];
                        out.text.push_str(&line[from..at]);
                        out.text.push_str(&format!("{LINK}{written}{kind}{new}"));
                        from = at + end;
                    }
                    Now::NotYet => out.waiting = true,
                    Now::Unknown => {}
                }
                at += end;
            }
            b'#' => {
                let digits = bytes[at + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let end = at + 1 + digits;
                let before = line[..at].chars().next_back();
                let after = line[end..].chars().next();
                let ends = digits > 0 && !after.is_some_and(|c| c.is_alphanumeric() || c == '_');
                let plain =
                    ends && !before.is_some_and(|c| c.is_alphanumeric() || "_/#&=".contains(c));
                // `owner/name#N`, when it names the restored repo.
                let named = refs.filter(|_| ends && !plain).and_then(|refs| {
                    let owner = repo_before(line, from, at)?;
                    Some((owner, refs.written(&line[owner..at])?))
                });
                let number = line[at + 1..end]
                    .parse::<u64>()
                    .ok()
                    .filter(|_| plain || named.is_some());
                match number.map(now) {
                    Some(Now::Is(new)) => {
                        match named {
                            Some((owner, written)) => {
                                out.text.push_str(&line[from..owner]);
                                out.text.push_str(written);
                            }
                            None => out.text.push_str(&line[from..at]),
                        }
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

    /// The backup is of `owner/demo`, which GitHub answered for as `Owner/Demo-New`.
    const FROM: [&str; 2] = ["owner/demo", "Owner/Demo-New"];

    fn over(body: &str) -> String {
        let refs = Refs {
            from: &FROM,
            to: "owner/demo",
        };
        renumber_for(body, Some(&refs), &now).text
    }

    fn into_new(body: &str) -> String {
        let refs = Refs {
            from: &FROM,
            to: "owner/restored",
        };
        renumber_for(body, Some(&refs), &now).text
    }

    #[test]
    fn a_full_link_to_the_restored_repo_gets_the_new_number() {
        assert_eq!(
            over("see https://github.com/owner/demo/issues/5 and https://github.com/owner/demo/pull/12."),
            "see https://github.com/owner/demo/issues/3 and https://github.com/owner/demo/pull/40."
        );
        assert_eq!(
            over("[x](https://github.com/OWNER/Demo/issues/5#issuecomment-9) <https://github.com/owner/demo-new/issues/5>"),
            "[x](https://github.com/OWNER/Demo/issues/3#issuecomment-9) <https://github.com/owner/demo/issues/3>",
            "the name is compared without case and keeps its spelling; the other name the \
             backup knows the repo by becomes the one the restore goes to"
        );
    }

    #[test]
    fn a_number_behind_the_restored_repos_name_gets_the_new_number() {
        assert_eq!(
            over("owner/demo#5, (Owner/Demo#12) and #5"),
            "owner/demo#3, (Owner/Demo#40) and #3"
        );
        assert_eq!(over("Owner/Demo-New#5"), "owner/demo#3");
    }

    #[test]
    fn another_repo_another_host_and_code_are_left() {
        for same in [
            "other/repo#5",
            "owner/demo2#5",
            "xowner/demo#5 a/owner/demo#5 @owner/demo#5",
            "https://github.com/other/repo/issues/5",
            "https://github.com/owner/demo2/issues/5",
            "https://github.com/owner/demo/issues/5x",
            "https://github.com/owner/demo/issues/",
            "https://github.com/owner/demo/milestone/5",
            "https://github.com/owner/demo/blob/main/a.md#5",
            "https://example.invalid/owner/demo/issues/5",
            "https://example.invalid/owner/demo#5",
            "http://github.com.example.invalid/owner/demo/issues/5",
            "`owner/demo#5` `https://github.com/owner/demo/issues/5`",
            "```\nowner/demo#5\nhttps://github.com/owner/demo/issues/5\n```\n",
        ] {
            assert_eq!(over(same), same);
        }
    }

    #[test]
    fn a_number_with_no_mapping_is_left_and_one_not_there_yet_waits() {
        let same = "owner/demo#7 https://github.com/owner/demo/issues/7 https://github.com/owner/demo/pull/7";
        assert_eq!(over(same), same);
        let refs = Refs {
            from: &FROM,
            to: "owner/demo",
        };
        for waits in ["owner/demo#9", "https://github.com/owner/demo/issues/9"] {
            let got = renumber_for(waits, Some(&refs), &now);
            assert_eq!(got.text, waits);
            assert!(got.waiting, "{waits}");
        }
        assert!(!renumber_for("other/repo#9 `owner/demo#9`", Some(&refs), &now).waiting);
    }

    /// 5 is 3 now and 3 is 8 now: a number a rewrite wrote is never read as a reference again.
    #[test]
    fn no_reference_is_rewritten_twice() {
        fn chain(old: u64) -> Now {
            match old {
                5 => Now::Is(3),
                3 => Now::Is(8),
                _ => Now::Unknown,
            }
        }
        let refs = Refs {
            from: &FROM,
            to: "owner/demo",
        };
        let text = |body: &str| renumber_for(body, Some(&refs), &chain).text;
        assert_eq!(text("owner/demo#5"), "owner/demo#3");
        assert_eq!(
            text("https://github.com/owner/demo/issues/5"),
            "https://github.com/owner/demo/issues/3"
        );
        assert_eq!(
            text("[#5](https://github.com/owner/demo/issues/5) #5"),
            "[#3](https://github.com/owner/demo/issues/3) #3"
        );
        assert_eq!(
            text("https://github.com/owner/demo/issues/5#5 owner/demo#5#5"),
            "https://github.com/owner/demo/issues/3#5 owner/demo#3#5",
            "a # right after the number is no reference"
        );
    }

    #[test]
    fn a_restore_to_another_repo_names_that_repo() {
        assert_eq!(
            into_new("owner/demo#5 https://github.com/Owner/Demo/issues/5 #5 owner/demo#7"),
            "owner/restored#3 https://github.com/owner/restored/issues/3 #3 owner/demo#7"
        );
        assert_eq!(
            into_new("owner/demo#1 https://github.com/owner/demo/pull/1"),
            "owner/restored#1 https://github.com/owner/restored/pull/1",
            "the same number in the new repo still gets the new repo's name"
        );
    }

    #[test]
    fn without_the_restored_repo_only_plain_numbers_are_read() {
        let same = "owner/demo#5 https://github.com/owner/demo/issues/5";
        assert_eq!(text(same), same);
    }
}
