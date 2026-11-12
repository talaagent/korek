//! Markdown helpers.

/// `s` as an inline code span, fenced with more backticks than it holds.
pub fn code(s: &str) -> String {
    let mut run = 0;
    let mut longest = 0;
    for c in s.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{s}{pad}{fence}")
}

/// Text fit for one table cell: paragraphs joined with `<br>`, pipes
/// escaped.
pub fn cell(s: &str) -> String {
    s.trim()
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("<br><br>")
        .replace('|', "\\|")
}

/// A table row.
pub fn row(out: &mut String, cells: &[String]) {
    out.push('|');
    for c in cells {
        out.push(' ');
        out.push_str(&cell(c));
        out.push_str(" |");
    }
    out.push('\n');
}

/// A table header and its separator.
pub fn header(out: &mut String, cells: &[String]) {
    row(out, cells);
    out.push('|');
    for _ in cells {
        out.push_str("---|");
    }
    out.push('\n');
}

/// A heading at `level` (clamped to 6) with an explicit anchor before it,
/// so links do not depend on the renderer's slug rules.
pub fn heading(out: &mut String, level: usize, anchor: Option<&str>, text: &str) {
    if let Some(a) = anchor {
        out.push_str("<a id=\"");
        out.push_str(a);
        out.push_str("\"></a>\n\n");
    }
    out.push_str(&"#".repeat(level.clamp(1, 6)));
    out.push(' ');
    out.push_str(text);
    out.push_str("\n\n");
}

/// A fenced block of `lang`, fenced with more backticks than it holds.
pub fn block(out: &mut String, lang: &str, body: &str) {
    let mut fence = "```".to_owned();
    while body.contains(&fence) {
        fence.push('`');
    }
    out.push_str(&fence);
    out.push_str(lang);
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');
    out.push_str(&fence);
    out.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(code("a"), "`a`");
        assert_eq!(code("a`b"), "``a`b``");
        assert_eq!(code("`a"), "`` `a ``");
        assert_eq!(cell("a | b\n\nc\nd"), "a \\| b<br><br>c d");
    }
}
