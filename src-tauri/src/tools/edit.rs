//! `edit`: replace one exact stretch of a file with new text.
//!
//! The old text must appear in the file exactly once. Zero matches means the
//! model is working from a stale read; several matches mean the stretch is not
//! unique enough to be safe. Both are readable errors that say which one it is,
//! so the model can widen the stretch or read the file again.
//!
//! The result shows the changed lines with their line numbers, so the model can
//! confirm the change landed where it meant it to.
//!
//! Every path is resolved against the pair's workspace and may never leave it:
//! absolute paths, `..` segments and symlinks that point outside are refused.
//!
//! A file over [`MAX_READ`] is refused rather than read whole: a replacement
//! decodes the file twice — once as bytes, once as the folded copy the
//! whitespace-insensitive match walks — so a huge file would cost several times
//! its size in memory. The new text is written beside the file and renamed over
//! it, so a write that runs out of room leaves the file whole.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::files::{replace_file, MAX_READ};
use super::ToolContext;

/// Unchanged lines further than this from a change fold into one `⋮`.
const CONTEXT: usize = 2;

/// How many changed lines the diff shows before it folds the rest away; a replacement of
/// thousands of lines would otherwise build a diff nobody scrolls.
const MAX_CHANGED_LINES: usize = 200;

/// How many bytes of a file are inspected for a NUL before calling it binary.
const BINARY_SNIFF: usize = 8000;

/// Replaces the single occurrence of `old` in the file at `path` with `new`.
///
/// `old` is matched exactly first, then again with trailing whitespace removed
/// and look-alike characters (smart quotes, Unicode dashes, special spaces)
/// folded, so a read that lost a trailing space still matches. Either way the
/// match must be unique.
pub async fn edit(ctx: &ToolContext<'_>, args: &Map<String, Value>) -> Result<String, String> {
    let (path, old, new) = match (
        args.get("path").and_then(Value::as_str),
        args.get("old_string").and_then(Value::as_str),
        args.get("new_string").and_then(Value::as_str),
    ) {
        (Some(path), Some(old), Some(new)) => (path, old, new),
        _ => return Err("edit needs `path`, `old_string` and `new_string`".to_string()),
    };
    let directory = ctx.directory.to_path_buf();
    let (path, old, new) = (path.to_string(), old.to_string(), new.to_string());
    // Reading and rewriting the file is file work, so it runs off the async
    // worker: a FIFO or a slow mount would otherwise hold the worker — and with
    // it the turn, which is only polled for cancellation between tool calls.
    match tokio::task::spawn_blocking(move || edit_file(&directory, &path, &old, &new)).await {
        Ok(result) => result,
        Err(error) => Err(format!("the edit could not run: {}", error)),
    }
}

/// The replacement itself: read the file, apply it, put it back.
fn edit_file(directory: &Path, path: &str, old: &str, new: &str) -> Result<String, String> {
    let full = resolve(directory, path)?;
    // The file is about to be decoded twice — once as bytes, once as the
    // normalized copy the folded match walks — so one over [`MAX_READ`] is
    // refused rather than allowed to cost several times its size.
    let too_big = fs::metadata(&full)
        .ok()
        .filter(|meta| meta.len() > MAX_READ);
    if let Some(meta) = too_big {
        return Err(format!(
            "`{path}` is {} MB, too large to edit whole; rewrite it with `write` or change part of it with bash",
            meta.len() / (1024 * 1024)
        ));
    }
    let bytes = match fs::read(&full) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(format!(
                "could not read `{}`: {}; use `write` to create a file",
                path, error
            ))
        }
    };
    if is_binary(&bytes) {
        return Err(format!(
            "`{}` is a binary file; edit only works on text",
            path
        ));
    }
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            return Err(format!(
                "`{}` is not valid UTF-8 (first bad byte at offset {}); edit would rewrite bytes it cannot show; use `write` or `bash` instead",
                path,
                error.utf8_error().valid_up_to()
            ))
        }
    };
    let file = TextFile::decode(text);
    let applied = match apply(&file.text, old, new, path) {
        Ok(applied) => applied,
        Err(message) => return Err(message),
    };
    // The new text lands beside the file and is renamed over it, so a write that
    // runs out of room leaves the file whole rather than truncated.
    match replace_file(&full, &file.encode(&applied.text)) {
        Ok(()) => Ok(format!(
            "edited `{}` (1 replacement):\n{}",
            path, applied.diff
        )),
        Err(error) => Err(format!("could not write `{}`: {}", path, error)),
    }
}

/// What one replacement produced: the whole new text, and the changed lines.
struct Applied {
    text: String,
    /// The changed lines with their numbers, for the model to check.
    diff: String,
}

/// Replaces the one occurrence of `old` in `text`. `path` is only for messages.
fn apply(text: &str, old: &str, new: &str, path: &str) -> Result<Applied, String> {
    let old = old.replace("\r\n", "\n");
    let new = new.replace("\r\n", "\n");
    if old.is_empty() {
        return Err("`old_string` must not be empty".to_string());
    }
    let found: Vec<(usize, usize)> = text
        .match_indices(&old)
        .map(|(start, matched)| (start, start + matched.len()))
        .collect();
    let found = if found.is_empty() {
        let haystack = Normalized::new(text);
        let needle = Normalized::new(&old).text;
        if needle.is_empty() {
            Vec::new()
        } else {
            haystack
                .text
                .match_indices(&needle)
                .map(|(start, matched)| (haystack.map[start], haystack.map[start + matched.len()]))
                .collect()
        }
    } else {
        found
    };
    let (start, end) = match found.len() {
        0 => {
            let hint = closest_hint(text, &old).unwrap_or_default();
            return Err(format!(
                "could not find `old_string` in `{}`; it must match the file exactly, including indentation and line breaks; read the file again if unsure.{}",
                path, hint
            ));
        }
        1 => found[0],
        n => {
            return Err(format!(
                "found {} matches for `old_string` in `{}`; include more surrounding lines to make it unique",
                n, path
            ))
        }
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&new);
    out.push_str(&text[end..]);
    if out == text {
        return Err(format!(
            "no changes made to `{}`: the replacement produced identical content",
            path
        ));
    }
    let region_start = context_start(text, start, CONTEXT);
    let region_end = context_end(text, end, CONTEXT);
    let before = &text[region_start..start];
    let after = &text[end..region_end];
    let mut old_region = String::with_capacity(region_end - region_start);
    old_region.push_str(before);
    old_region.push_str(&text[start..end]);
    old_region.push_str(after);
    let mut new_region = String::with_capacity(old_region.len());
    new_region.push_str(before);
    new_region.push_str(&new);
    new_region.push_str(after);
    let first_line = text[..region_start].matches('\n').count() + 1;
    let diff = line_diff(&old_region, &new_region, first_line, first_line);
    Ok(Applied { text: out, diff })
}

/// The offset where the line holding `offset` begins, then `back` more lines back.
fn context_start(text: &str, offset: usize, back: usize) -> usize {
    let mut start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    for _ in 0..back {
        if start == 0 {
            break;
        }
        start = text[..start - 1].rfind('\n').map_or(0, |at| at + 1);
    }
    start
}

/// The offset just past the line holding `offset`, then `forward` more lines on.
fn context_end(text: &str, offset: usize, forward: usize) -> usize {
    let mut end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at + 1);
    for _ in 0..forward {
        match text[end..].find('\n') {
            Some(at) => end += at + 1,
            None => {
                end = text.len();
                break;
            }
        }
    }
    end
}

/// The lines that changed between `old` and `new`. Each row is `-`, `+` or a
/// space, then the line number (from `old_line` or `new_line`), a tab and the
/// text; unchanged lines more than [`CONTEXT`] away from a change fold into one
/// `⋮`, and past [`MAX_CHANGED_LINES`] changed rows the rest folds away too.
fn line_diff(old: &str, new: &str, old_line: usize, new_line: usize) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let mut ops: Vec<(char, &str)> = Vec::new();
    if a.len() * b.len() > 250_000 {
        ops.extend(
            a.iter()
                .map(|line| ('-', *line))
                .chain(b.iter().map(|line| ('+', *line))),
        );
    } else {
        // lcs[i][j]: the longest common subsequence of a[i..] and b[j..].
        let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
        for i in (0..a.len()).rev() {
            for j in (0..b.len()).rev() {
                lcs[i][j] = if a[i] == b[j] {
                    lcs[i + 1][j + 1] + 1
                } else {
                    lcs[i + 1][j].max(lcs[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() || j < b.len() {
            if i < a.len() && j < b.len() && a[i] == b[j] {
                ops.push((' ', a[i]));
                (i, j) = (i + 1, j + 1);
            } else if j == b.len() || i < a.len() && lcs[i + 1][j] >= lcs[i][j + 1] {
                ops.push(('-', a[i]));
                i += 1;
            } else {
                ops.push(('+', b[j]));
                j += 1;
            }
        }
    }
    let near = |k: usize| {
        ops[k.saturating_sub(CONTEXT)..(k + CONTEXT + 1).min(ops.len())]
            .iter()
            .any(|(sign, _)| *sign != ' ')
    };
    let (mut out, mut folded) = (String::new(), false);
    let (mut old_no, mut new_no) = (old_line, new_line);
    let (mut changed, mut omitted) = (0, 0);
    for (k, (sign, text)) in ops.iter().enumerate() {
        let number = if *sign == '-' { old_no } else { new_no };
        let capped = changed >= MAX_CHANGED_LINES;
        if near(k) && !capped {
            out.push_str(&format!("{}{}\t{}\n", sign, number, text));
            folded = false;
            if *sign != ' ' {
                changed += 1;
            }
        } else if near(k) {
            if *sign != ' ' {
                omitted += 1;
            }
        } else if !folded {
            out.push_str("⋮\n");
            folded = true;
        }
        match sign {
            '-' => old_no += 1,
            '+' => new_no += 1,
            _ => (old_no, new_no) = (old_no + 1, new_no + 1),
        }
    }
    if omitted > 0 {
        out.push_str(&format!("⋮ [{} more changed lines not shown]\n", omitted));
    }
    out
}

/// The first divergent line of the closest block, or the closest line for a one-line edit.
/// An unrelated file gets no hint.
fn closest_hint(text: &str, old: &str) -> Option<String> {
    let start = nearest_line(text, old)?;
    let expected: Vec<&str> = old
        .lines()
        .skip_while(|line| line.trim().is_empty())
        .collect();
    if expected.len() < 2 {
        return Some(format!(
            " The closest line is line {}; read from there.",
            start
        ));
    }
    let lines: Vec<&str> = text.lines().collect();
    let distance = edit_distance(expected[0].trim(), lines[start - 1].trim());
    let (mut best, mut matched) = (start - 1, 0);
    for (index, line) in lines.iter().enumerate() {
        if edit_distance(expected[0].trim(), line.trim()) != distance {
            continue;
        }
        let prefix = expected
            .iter()
            .zip(&lines[index..])
            .take_while(|(want, actual)| want == actual)
            .count();
        if prefix > matched {
            (best, matched) = (index, prefix);
        }
    }
    if matched == expected.len() {
        return Some(format!(
            " The closest line is line {}; read from there.",
            best + 1
        ));
    }
    let line = best + matched + 1;
    let actual = lines.get(best + matched);
    let detail = match actual {
        Some(actual) => {
            let excerpt: String = actual.chars().take(160).collect();
            let suffix = if actual.chars().count() > 160 {
                "…"
            } else {
                ""
            };
            format!("the file has {:?}{}.", excerpt, suffix)
        }
        None => "the file ends there.".to_string(),
    };
    Some(format!(
        " The closest block starts at line {}; first differing line is line {}: {}",
        best + 1,
        line,
        detail
    ))
}

/// The 1-based line of `text` closest to `old`'s first non-empty line, trimmed on both sides.
/// `None` when nothing is within half the needle's length.
fn nearest_line(text: &str, old: &str) -> Option<usize> {
    let needle = old.lines().map(str::trim).find(|line| !line.is_empty())?;
    let limit = (needle.chars().count() / 2).max(1);
    text.lines()
        .enumerate()
        .map(|(i, line)| (i + 1, edit_distance(needle, line.trim())))
        .min_by_key(|(_, distance)| *distance)
        .filter(|(_, distance)| *distance <= limit)
        .map(|(line, _)| line)
}

/// Levenshtein distance between two strings, over characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if ca == *cb {
                diagonal
            } else {
                1 + diagonal.min(row[j]).min(above)
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Text with trailing whitespace removed from each line and look-alike characters folded,
/// plus a map from each byte of it back to a byte offset in the original.
struct Normalized {
    text: String,
    map: Vec<usize>,
}

impl Normalized {
    fn new(src: &str) -> Self {
        let mut text = String::with_capacity(src.len());
        let mut map = Vec::with_capacity(src.len() + 1);
        let mut pos = 0;
        for line in src.split_inclusive('\n') {
            let body = line.strip_suffix('\n').unwrap_or(line);
            for (offset, ch) in body.trim_end_matches(char::is_whitespace).char_indices() {
                text.push(fold(ch));
                map.resize(text.len(), pos + offset);
            }
            if body.len() < line.len() {
                text.push('\n');
                map.push(pos + body.len());
            }
            pos += line.len();
        }
        map.push(src.len());
        Self { text, map }
    }
}

fn fold(ch: char) -> char {
    match ch {
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
        other => other,
    }
}

/// A text file as read and edit see it: BOM removed, and CRLF shown as LF when every line
/// ends in CRLF, so the model works on one view and a write restores the original form.
struct TextFile {
    text: String,
    bom: bool,
    crlf: bool,
}

impl TextFile {
    fn decode(text: String) -> Self {
        let (bom, text) = match text.strip_prefix('\u{feff}') {
            Some(rest) => (true, rest.to_string()),
            None => (false, text),
        };
        let mut newlines = 0;
        let mut all_crlf = true;
        for (i, _) in text.match_indices('\n') {
            newlines += 1;
            if i == 0 || text.as_bytes()[i - 1] != b'\r' {
                all_crlf = false;
            }
        }
        let crlf = newlines > 0 && all_crlf;
        let text = if crlf {
            text.replace("\r\n", "\n")
        } else {
            text
        };
        Self { text, bom, crlf }
    }

    fn encode(&self, text: &str) -> String {
        let body = if self.crlf {
            text.replace('\n', "\r\n")
        } else {
            text.to_string()
        };
        if self.bom {
            format!("\u{feff}{}", body)
        } else {
            body
        }
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0)
}

/// Resolves `path` against the workspace, refusing anything that leaves it.
///
/// Absolute paths and `..` segments are rejected outright; the resolved path is
/// canonicalized, so a symlink pointing out of the workspace is caught too.
fn resolve(directory: &Path, path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("`path` must not be empty".to_string());
    }
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(format!(
            "`{}` must be a path relative to the workspace",
            path
        ));
    }
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                return Err(format!("`{}` must not contain `..` segments", path))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "`{}` must be a path relative to the workspace",
                    path
                ))
            }
            _ => {}
        }
    }
    let root = directory
        .canonicalize()
        .map_err(|error| format!("could not resolve the workspace directory: {}", error))?;
    let full = root.join(relative);
    let real = match full.canonicalize() {
        Ok(real) => real,
        // The file may not be there yet: its parent must be, and must stay inside.
        Err(_) => {
            let parent = full
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.clone());
            let parent = parent
                .canonicalize()
                .map_err(|error| format!("could not read `{}`: {}", path, error))?;
            if !parent.starts_with(&root) {
                return Err(format!("`{}` is outside the workspace", path));
            }
            full
        }
    };
    if !real.starts_with(&root) {
        return Err(format!("`{}` is outside the workspace", path));
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolPolicy;
    use serde_json::json;

    fn workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-edit-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("temp workspace");
        dir
    }

    fn context(dir: &Path) -> ToolContext<'_> {
        ToolContext {
            directory: dir,
            policy: ToolPolicy::Full,
        }
    }

    fn args(path: &str, old: &str, new: &str) -> Map<String, Value> {
        let mut args = Map::new();
        args.insert("path".to_string(), json!(path));
        args.insert("old_string".to_string(), json!(old));
        args.insert("new_string".to_string(), json!(new));
        args
    }

    #[tokio::test]
    async fn an_exact_replacement_rewrites_the_file_and_shows_the_change() {
        let dir = workspace();
        let file = dir.join("src/main.rs");
        fs::create_dir_all(file.parent().expect("src")).expect("src dir");
        fs::write(&file, "fn main() {\n    let x = 1;\n    let y = 2;\n}\n").expect("seed");

        let result = edit(
            &context(&dir),
            &args("src/main.rs", "    let x = 1;", "    let x = 42;"),
        )
        .await
        .expect("edit");

        assert_eq!(
            fs::read_to_string(&file).expect("read back"),
            "fn main() {\n    let x = 42;\n    let y = 2;\n}\n"
        );
        assert!(
            result.contains("edited `src/main.rs` (1 replacement)"),
            "{}",
            result
        );
        assert!(result.contains("-2\t    let x = 1;"), "{}", result);
        assert!(result.contains("+2\t    let x = 42;"), "{}", result);
        assert!(result.contains(" 1\tfn main() {"), "{}", result);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_missing_argument_is_a_readable_error() {
        let dir = workspace();
        fs::write(dir.join("a.txt"), "one\n").expect("seed");
        let mut missing_old = Map::new();
        missing_old.insert("path".to_string(), json!("a.txt"));
        missing_old.insert("new_string".to_string(), json!("two"));
        let error = edit(&context(&dir), &missing_old).await.unwrap_err();
        assert!(
            error.contains("`path`, `old_string` and `new_string`"),
            "{}",
            error
        );
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).expect("read back"),
            "one\n"
        );

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_absolute_path_is_refused() {
        let dir = workspace();
        let outside =
            std::env::temp_dir().join(format!("the-pair-edit-out-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside).expect("outside dir");
        let file = outside.join("secret.txt");
        fs::write(&file, "keep\n").expect("seed outside");

        let error = edit(
            &context(&dir),
            &args(file.to_str().expect("utf8"), "keep", "gone"),
        )
        .await
        .unwrap_err();
        assert!(error.contains("relative to the workspace"), "{}", error);
        assert_eq!(fs::read_to_string(&file).expect("untouched"), "keep\n");

        fs::remove_dir_all(&outside).expect("cleanup outside");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_parent_segment_is_refused() {
        let dir = workspace();
        let outside =
            std::env::temp_dir().join(format!("the-pair-edit-out-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside).expect("outside dir");
        let file = outside.join("secret.txt");
        fs::write(&file, "keep\n").expect("seed outside");

        let error = edit(&context(&dir), &args("../secret.txt", "keep", "gone"))
            .await
            .unwrap_err();
        assert!(error.contains("`..` segments"), "{}", error);
        assert_eq!(fs::read_to_string(&file).expect("untouched"), "keep\n");

        fs::remove_dir_all(&outside).expect("cleanup outside");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlink_out_of_the_workspace_is_refused() {
        let dir = workspace();
        let outside =
            std::env::temp_dir().join(format!("the-pair-edit-out-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside).expect("outside dir");
        let file = outside.join("secret.txt");
        fs::write(&file, "keep\n").expect("seed outside");
        std::os::unix::fs::symlink(&file, dir.join("link.txt")).expect("symlink");

        let error = edit(&context(&dir), &args("link.txt", "keep", "gone"))
            .await
            .unwrap_err();
        assert!(error.contains("outside the workspace"), "{}", error);
        assert_eq!(fs::read_to_string(&file).expect("untouched"), "keep\n");

        fs::remove_dir_all(&outside).expect("cleanup outside");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn zero_matches_is_a_readable_error() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "alpha\nbeta\n").expect("seed");

        let error = edit(&context(&dir), &args("a.txt", "gamma", "delta"))
            .await
            .unwrap_err();
        assert!(error.contains("could not find `old_string`"), "{}", error);
        assert!(error.contains("match the file exactly"), "{}", error);
        assert_eq!(
            fs::read_to_string(&file).expect("untouched"),
            "alpha\nbeta\n"
        );

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn several_matches_is_a_readable_error() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "x = 1\nx = 2\n").expect("seed");

        let error = edit(&context(&dir), &args("a.txt", "x = ", "y = "))
            .await
            .unwrap_err();
        assert!(error.contains("found 2 matches"), "{}", error);
        assert!(error.contains("make it unique"), "{}", error);
        assert_eq!(
            fs::read_to_string(&file).expect("untouched"),
            "x = 1\nx = 2\n"
        );

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_identical_replacement_changes_nothing() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "same\n").expect("seed");

        let error = edit(&context(&dir), &args("a.txt", "same", "same"))
            .await
            .unwrap_err();
        assert!(error.contains("identical content"), "{}", error);
        assert_eq!(fs::read_to_string(&file).expect("untouched"), "same\n");

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_huge_replacement_folds_its_diff() {
        let dir = workspace();
        let file = dir.join("a.txt");
        let before = format!("keep\n{}\n", "old\n".repeat(500));
        fs::write(&file, &before).expect("seed");

        let result = edit(
            &context(&dir),
            &args("a.txt", &"old\n".repeat(500), &"new\n".repeat(500)),
        )
        .await
        .expect("edit");

        assert_eq!(
            fs::read_to_string(&file).expect("read back"),
            format!("keep\n{}\n", "new\n".repeat(500))
        );
        assert!(
            result.contains("⋮ [804 more changed lines not shown]"),
            "{}",
            result
        );
        assert!(result.len() < 100_000, "{} bytes", result.len());

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_missing_match_points_at_the_closest_line() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "fn main() {\n    let x = 0;\n}\n").expect("seed");

        let error = edit(
            &context(&dir),
            &args("a.txt", "\tlet x = 0;", "    let x = 1;"),
        )
        .await
        .unwrap_err();
        assert!(error.contains("closest line is line 2"), "{}", error);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_file_that_does_not_exist_points_at_write() {
        let dir = workspace();
        let error = edit(&context(&dir), &args("nope.txt", "a", "b"))
            .await
            .unwrap_err();
        assert!(error.contains("could not read `nope.txt`"), "{}", error);
        assert!(error.contains("use `write`"), "{}", error);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_empty_old_string_is_refused() {
        let dir = workspace();
        fs::write(dir.join("a.txt"), "one\n").expect("seed");
        let error = edit(&context(&dir), &args("a.txt", "", "two"))
            .await
            .unwrap_err();
        assert!(
            error.contains("`old_string` must not be empty"),
            "{}",
            error
        );
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).expect("untouched"),
            "one\n"
        );

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_match_ignoring_trailing_space_and_smart_quotes_is_applied() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "let s = \u{201C}hi\u{201D};   \nnext\n").expect("seed");

        let result = edit(
            &context(&dir),
            &args("a.txt", "let s = \"hi\";\nnext", "let s = \"yo\";\nnext"),
        )
        .await
        .expect("edit");

        assert_eq!(
            fs::read_to_string(&file).expect("read back"),
            "let s = \"yo\";\nnext\n"
        );
        assert!(result.contains("+1\tlet s = \"yo\";"), "{}", result);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn crlf_and_bom_round_trip() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "\u{feff}a\r\nb\r\n").expect("seed");

        let result = edit(&context(&dir), &args("a.txt", "a\nb", "a\nc"))
            .await
            .expect("edit");

        assert_eq!(
            fs::read(&file).expect("read back"),
            "\u{feff}a\r\nc\r\n".as_bytes()
        );
        assert!(result.contains(" 1\ta"), "{}", result);
        assert!(result.contains("-2\tb"), "{}", result);
        assert!(result.contains("+2\tc"), "{}", result);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_file_too_large_to_edit_is_refused() {
        let dir = workspace();
        let file = dir.join("big.log");
        fs::File::create(&file)
            .expect("seed")
            .set_len(MAX_READ + 1)
            .expect("a sparse file");

        let error = edit(&context(&dir), &args("big.log", "a", "b"))
            .await
            .unwrap_err();
        assert!(error.contains("too large to edit"), "{}", error);
        assert!(error.contains("`write`"), "{}", error);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_edit_that_cannot_be_written_leaves_the_file_whole() {
        let dir = workspace();
        let file = dir.join("a.txt");
        fs::write(&file, "one\n").expect("seed");
        // The temp copy cannot be written, so the target is never touched: an
        // in-place write would have truncated it before it ran out of room.
        fs::create_dir_all(dir.join("a.txt.the-pair-tmp")).expect("a squatting folder");

        let error = edit(&context(&dir), &args("a.txt", "one", "two"))
            .await
            .unwrap_err();
        assert!(error.contains("could not write `a.txt`"), "{}", error);
        assert_eq!(fs::read_to_string(&file).expect("untouched"), "one\n");

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_edit_leaves_no_half_written_copy_behind() {
        let dir = workspace();
        fs::write(dir.join("a.txt"), "one\n").expect("seed");

        edit(&context(&dir), &args("a.txt", "one", "two"))
            .await
            .expect("edit");
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .expect("the workspace")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(leftovers, vec!["a.txt".to_string()], "{:?}", leftovers);

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn an_edit_does_not_hold_the_async_worker() {
        let dir = workspace();
        fs::write(dir.join("a.txt"), "one\n").expect("seed");
        // The read and the rewrite run off the worker, so a heartbeat sharing it
        // still runs; a blocking implementation would starve it.
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let ctx = context(&dir);
            let call = args("a.txt", "one", "two");
            tokio::join!(edit(&ctx, &call), beat())
        })
        .await
        .expect("the edit held the worker");
        assert!(outcome.0.is_ok(), "{:?}", outcome.0);
        assert!(outcome.1, "the heartbeat was starved");
        assert_eq!(
            fs::read_to_string(dir.join("a.txt")).expect("back"),
            "two\n"
        );

        fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Runs five times, as a task sharing the worker does.
    async fn beat() -> bool {
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        true
    }

    #[tokio::test]
    async fn a_binary_file_is_refused() {
        let dir = workspace();
        let file = dir.join("blob.bin");
        fs::write(&file, [0x00, 0x01, 0x02]).expect("seed");

        let error = edit(&context(&dir), &args("blob.bin", "a", "b"))
            .await
            .unwrap_err();
        assert!(error.contains("binary file"), "{}", error);
        assert_eq!(fs::read(&file).expect("untouched"), [0x00, 0x01, 0x02]);

        fs::remove_dir_all(&dir).expect("cleanup");
    }
}
