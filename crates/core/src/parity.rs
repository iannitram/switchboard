use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    pub id: &'a str,
    pub tauri: Option<&'a str>,
    pub cli: Option<&'a str>,
    pub mcp: Option<&'a str>,
    pub ui_only: Option<&'a str>,
}

/// The window's command names from `generate_handler![...]` in main.rs, paths dropped. An error
/// names what is missing, so a moved or renamed list is not read as "no commands".
pub fn read_handlers(main_rs: &str) -> Result<Vec<String>, String> {
    // A commented-out command is not registered.
    let main_rs = &strip_rust_comments(main_rs);
    let start = main_rs
        .find("generate_handler![")
        .ok_or("main.rs has no generate_handler![ list")?;
    let rest = &main_rs[start + "generate_handler![".len()..];
    let end = rest
        .find(']')
        .ok_or("main.rs's generate_handler![ list has no closing ]")?;

    Ok(rest[..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            name.rsplit_once("::")
                .map_or(name, |(_, command)| command)
                .to_string()
        })
        .collect())
}

/// `text` with its `//` and `/* */` comments blanked out, line breaks kept.
fn strip_rust_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('/') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("//") {
            rest = &tail[tail.find('\n').unwrap_or(tail.len())..];
        } else if tail.starts_with("/*") {
            let end = tail.find("*/").map_or(tail.len(), |e| e + 2);
            out.extend(tail[..end].chars().filter(|&ch| ch == '\n'));
            rest = &tail[end..];
        } else {
            out.push('/');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// `read_handlers`, empty when there is no list.
pub fn handler_names(main_rs: &str) -> Vec<String> {
    read_handlers(main_rs).unwrap_or_default()
}

enum Site {
    Name(String),
    Unread,
}

/// Each `call(` or `call<...>(` in a face file, by byte offset: its string command name, or
/// `Unread` when the name is not a plain string literal. The `function call` definition is not
/// a call, and neither is `fn.call(this, ...)`; any other `x.call(` is read like `call(`.
fn call_sites(ts: &str) -> Vec<(usize, Site)> {
    let mut sites = Vec::new();

    for (start, _) in ts.match_indices("call") {
        let before = ts[..start].chars().next_back();
        if before.is_some_and(is_identifier_char) {
            continue;
        }
        let method = before == Some('.');
        let quiet = ts[..start].trim_end().ends_with("function");

        let mut pos = start + "call".len();
        if ts[pos..].starts_with('<') {
            let mut depth = 0usize;
            let mut generic_end = None;
            let mut prev = None;
            for (offset, ch) in ts[pos..].char_indices() {
                let arrow = prev == Some('=');
                prev = Some(ch);
                match ch {
                    '<' => depth += 1,
                    // `=>` in a function type is not a closing bracket.
                    '>' if arrow => {}
                    '>' => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        if depth == 0 {
                            generic_end = Some(pos + offset + ch.len_utf8());
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = generic_end else {
                if !quiet {
                    sites.push((start, Site::Unread));
                }
                continue;
            };
            pos = end;
        }

        let spaced = ts[pos..].trim_start();
        if !spaced.starts_with('(') {
            continue;
        }
        pos = ts.len() - spaced.len() + 1;
        while let Some(ch) = ts[pos..].chars().next() {
            if !ch.is_whitespace() {
                break;
            }
            pos += ch.len_utf8();
        }
        let name = ts[pos..]
            .strip_prefix('"')
            .and_then(|rest| rest.find('"').map(|end| rest[..end].to_string()));
        match name {
            Some(name) => sites.push((start, Site::Name(name))),
            None if method && this_first(&ts[pos..]) => {}
            None if !quiet => sites.push((start, Site::Unread)),
            None => {}
        }
    }

    sites
}

pub fn face_calls(ts: &str) -> Vec<String> {
    call_sites(ts)
        .into_iter()
        .filter_map(|(_, site)| match site {
            Site::Name(name) => Some(name),
            Site::Unread => None,
        })
        .collect()
}

/// The 1-based lines of the `call(` sites whose command name is not a string literal
/// (`call(cmd)`, a template string, an unclosed generic). `face_calls` cannot check them.
pub fn face_call_gaps(ts: &str) -> Vec<usize> {
    call_sites(ts)
        .into_iter()
        .filter(|(_, site)| matches!(site, Site::Unread))
        .map(|(start, _)| ts[..start].matches('\n').count() + 1)
        .collect()
}

/// Whether an argument list starts with the word `this` as its own argument (`this,` or `this)`).
fn this_first(args: &str) -> bool {
    args.strip_prefix("this")
        .is_some_and(|rest| matches!(rest.trim_start().chars().next(), Some(',' | ')')))
}

/// The 1-based lines that reach the core without a `call(` this test can read: `invoke` itself,
/// `call` imported under another name, or `call` used as a value (`const run = call`,
/// `f(call, ...)`, `{ call: run }`). Comments are skipped, and so is `call` in an import list.
/// Only the core wrappers (core.ts, dev-core.ts) may do any of these.
pub fn face_bypasses(ts: &str) -> Vec<usize> {
    let mut lines = Vec::new();
    let mut in_import = false;
    for (index, line) in ts.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with("/*") || code.starts_with('*') {
            continue;
        }
        let import = in_import || code.starts_with("import ");
        in_import = import && !code.contains(" from ") && !code.ends_with(';');
        let words = word_spans(line);
        let bypass = words.iter().enumerate().any(|(i, &(at, word))| match word {
            "invoke" => true,
            "call" if import => words.get(i + 1).is_some_and(|&(_, next)| next == "as"),
            "call" => !call_is_read(line, at, i.checked_sub(1).map(|j| words[j].1)),
            _ => false,
        });
        if bypass {
            lines.push(index + 1);
        }
    }
    lines
}

/// A `call` word that `call_sites` reads: `call(`, `call<..>(`, `x.call(`, or the definition.
fn call_is_read(line: &str, at: usize, previous: Option<&str>) -> bool {
    let after = line[at + "call".len()..].trim_start();
    previous == Some("function")
        || after.starts_with('(')
        || after.starts_with('<')
        || line[..at].ends_with('.')
}

/// Each identifier in `line` with its byte offset.
fn word_spans(line: &str) -> Vec<(usize, &str)> {
    let mut words = Vec::new();
    let mut from = None;
    for (i, ch) in line
        .char_indices()
        .chain(std::iter::once((line.len(), ' ')))
    {
        match (from, is_identifier_char(ch)) {
            (None, true) => from = Some(i),
            (Some(start), false) => {
                words.push((start, &line[start..i]));
                from = None;
            }
            _ => {}
        }
    }
    words
}

pub fn usage_has(usage: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }

    usage.match_indices(word).any(|(start, _)| {
        let end = start + word.len();
        let before = usage[..start].chars().next_back();
        let after = usage[end..].chars().next();
        let left_boundary = before.map_or(true, |ch| !is_usage_word_char(ch));
        let right_boundary = after.map_or(true, |ch| !is_usage_word_char(ch));
        let placeholder = before == Some('<') && after == Some('>');
        left_boundary && right_boundary && !placeholder
    })
}

/// The subcommands a usage line offers: each top-level alternative of its `<a | b ...>` group
/// (the first `<...>` outside `[...]` with a `|` in it), with whitespace and line-continuation
/// backslashes squeezed. A word that is only an argument (`drop <archive | restore>`) is not one.
pub fn usage_subcommands(usage: &str) -> Vec<String> {
    let mut depth = 0usize;
    let mut open = None;
    for (i, ch) in usage.char_indices() {
        match ch {
            '[' | '<' => {
                if depth == 0 && ch == '<' {
                    open = Some(i + 1);
                }
                depth += 1;
            }
            ']' | '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if let Some(from) = open.take() {
                        let alternatives = split_alternatives(&usage[from..i]);
                        if alternatives.len() > 1 {
                            return alternatives;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

fn split_alternatives(group: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut from = 0;
    for (i, ch) in group.char_indices() {
        match ch {
            '[' | '<' => depth += 1,
            ']' | '>' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                parts.push(&group[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&group[from..]);
    parts
        .into_iter()
        .map(|part| {
            part.replace('\\', " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Whether `subcommand` (one or more words) starts one of the usage line's subcommands.
fn offers(subcommands: &[String], subcommand: &str) -> bool {
    subcommands.iter().any(|alternative| {
        alternative == subcommand
            || alternative
                .strip_prefix(subcommand)
                .is_some_and(|rest| rest.starts_with(' '))
    })
}

pub fn problems(
    rows: &[Row<'_>],
    handlers: &[String],
    calls: &[String],
    usage: &str,
    mcp_tools: &[&str],
) -> Vec<String> {
    let mut issues = Vec::new();
    let subcommands = usage_subcommands(usage);

    for name in handlers {
        if !rows.iter().any(|row| row.tauri == Some(name.as_str())) {
            issues.push(format!("tauri command {name} has no row"));
        }
    }

    let mut seen_calls = HashSet::new();
    for name in calls {
        if seen_calls.insert(name.as_str()) && !handlers.iter().any(|handler| handler == name) {
            issues.push(format!("face calls {name}, which no tauri command handles"));
        }
    }

    if subcommands.is_empty()
        && rows
            .iter()
            .any(|row| row.tauri.is_some() && row.cli.is_some())
    {
        issues.push("the CLI usage has no <a | b> subcommand list".to_string());
    }

    for row in rows {
        let Some(tauri) = row.tauri else {
            continue;
        };

        if row.cli.is_none() && row.mcp.is_none() && row.ui_only.is_none() {
            issues.push(format!(
                "{tauri} has no CLI or MCP counterpart and no UI-only reason"
            ));
        }
        if row.ui_only.is_some_and(|reason| reason.trim().is_empty()) {
            issues.push(format!("{tauri} is UI-only with an empty reason"));
        }
        if let Some(cli) = row.cli {
            if !offers(&subcommands, cli) {
                issues.push(format!("{tauri}: CLI subcommand {cli} is missing"));
            }
        }
        if let Some(mcp) = row.mcp {
            if !mcp_tools.contains(&mcp) {
                issues.push(format!("{tauri}: MCP tool {mcp} is missing"));
            }
        }
        if !handlers.iter().any(|name| name == tauri) {
            issues.push(format!(
                "row {} names tauri command {tauri}, which is not registered",
                row.id
            ));
        }
    }

    issues
}

fn is_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_usage_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}
