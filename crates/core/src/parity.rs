use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row<'a> {
    pub id: &'a str,
    pub tauri: Option<&'a str>,
    pub cli: Option<&'a str>,
    pub mcp: Option<&'a str>,
    pub ui_only: Option<&'a str>,
}

pub fn handler_names(main_rs: &str) -> Vec<String> {
    let Some(start) = main_rs.find("generate_handler![") else {
        return Vec::new();
    };
    let body_start = start + "generate_handler![".len();
    let rest = &main_rs[body_start..];
    let Some(end) = rest.find(']') else {
        return Vec::new();
    };

    rest[..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            name.rsplit_once("::")
                .map_or(name, |(_, command)| command)
                .to_string()
        })
        .collect()
}

pub fn face_calls(ts: &str) -> Vec<String> {
    let mut names = Vec::new();

    for (start, _) in ts.match_indices("call") {
        if ts[..start]
            .chars()
            .next_back()
            .is_some_and(is_identifier_char)
        {
            continue;
        }

        let mut pos = start + "call".len();
        if ts[pos..].starts_with('<') {
            let mut depth = 0usize;
            let mut generic_end = None;
            for (offset, ch) in ts[pos..].char_indices() {
                match ch {
                    '<' => depth += 1,
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
                continue;
            };
            pos = end;
        }

        if !ts[pos..].starts_with('(') {
            continue;
        }
        pos += 1;
        while let Some(ch) = ts[pos..].chars().next() {
            if !ch.is_whitespace() {
                break;
            }
            pos += ch.len_utf8();
        }
        if !ts[pos..].starts_with('"') {
            continue;
        }
        pos += 1;
        let Some(end) = ts[pos..].find('"') else {
            continue;
        };
        names.push(ts[pos..pos + end].to_string());
    }

    names
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

pub fn problems(
    rows: &[Row<'_>],
    handlers: &[String],
    calls: &[String],
    usage: &str,
    mcp_tools: &[&str],
) -> Vec<String> {
    let mut issues = Vec::new();

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
            if !usage_has(usage, cli) {
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
