//! Guards the design-system rules a type checker cannot see.
//!
//! In a screen (`src/pages`, the shell and the root):
//!
//! 1. `kit-only`       no GPUI Kit type named directly: controls come from `motodesk_ui`;
//! 2. `colour-literal` no colour written by hand: colours come from the theme (`cx.ui().colors`);
//! 3. `square`         no rounded corner: every corner in Motodesk is square.
//!
//! Escape hatch: put `ui-rules-allow: <why>` in a comment on the offending line or the line
//! above it. Use it for output that is genuinely not themed (a printed document), not to dodge
//! the rule.

use std::path::{Path, PathBuf};

const RULES: [(&str, &[&str]); 3] = [
    ("kit-only", &["gpui_component", "gpui_kit::", "gpui_base"]),
    ("colour-literal", &["rgb(", "rgba(", "hsla(", "hsl(", "Hsla {", "Rgba {"]),
    ("square", &[".rounded", "radius"]),
];
const ALLOW: &str = "ui-rules-allow:";

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The part of a line that is code: what comes before a `//` comment.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

fn violations(source: &str) -> Vec<(usize, &'static str, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let allowed = line.contains(ALLOW) || (i > 0 && lines[i - 1].contains(ALLOW));
        if allowed {
            continue;
        }
        let code = code_of(line);
        for (rule, needles) in RULES {
            if needles.iter().any(|needle| code.contains(needle)) {
                found.push((i + 1, rule, line.trim().to_string()));
            }
        }
    }
    found
}

#[test]
fn the_screens_follow_the_design_system() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("shell.rs"), src.join("app.rs")];
    rust_files(&src.join("pages"), &mut files);
    assert!(files.len() > 2, "the screens were not found under {}", src.display());

    let mut report = String::new();
    for file in files {
        let source = std::fs::read_to_string(&file).unwrap_or_default();
        for (line, rule, text) in violations(&source) {
            report.push_str(&format!("{}:{line}  [{rule}]  {text}\n", file.strip_prefix(&src).unwrap_or(&file).display()));
        }
    }
    assert!(report.is_empty(), "design-system rules broken:\n{report}");
}

#[test]
fn the_rules_catch_what_they_are_for_and_honour_the_escape_hatch() {
    let found = violations(
        "use gpui_component::button::Button;\n\
         div().bg(rgb(0xff0000)).rounded_md()\n\
         // a comment may say rgb( or rounded without harm\n\
         // ui-rules-allow: printed paper is white whatever the theme\n\
         let paper = rgb(0xffffff);\n\
         let fine = cx.ui().colors.accent;\n",
    );
    let rules: Vec<(usize, &str)> = found.iter().map(|(line, rule, _)| (*line, *rule)).collect();
    assert_eq!(rules, vec![(1, "kit-only"), (2, "colour-literal"), (2, "square")]);
}
