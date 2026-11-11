//! Reading and writing the files in a pair's workspace.
//!
//! Both tools take a path relative to [`ToolContext::directory`] and refuse anything that
//! would reach outside it: an absolute path, a `..` segment, or a link that points
//! elsewhere. A read comes back numbered and capped, so one huge file cannot flood the
//! model's context; a write creates the parent directories it needs.

use std::fmt::Write as _;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::ToolContext;

/// Lines shown before a read stops, whatever the file holds.
const MAX_LINES: usize = 2000;
/// Characters of text returned before a read stops; the rest stays in the file.
const MAX_BYTES: usize = 50 * 1024;
/// A single line longer than this is cut in the middle: one minified line would otherwise
/// spend the whole byte budget on itself.
const MAX_LINE_CHARS: usize = 2000;
/// Larger text files are not read whole: the file, its decoded copy and its lines would take
/// several times its size in memory. `bash` can read part of one (`sed -n`, `tail`).
pub(crate) const MAX_READ: u64 = 10 * 1024 * 1024;
/// How many leading bytes decide whether a file is text.
const BINARY_SNIFF: usize = 8000;

/// Reads a file in the workspace, every line prefixed `N\t`.
///
/// A file that does not fit in [`MAX_LINES`] lines or [`MAX_BYTES`] characters is cut, and
/// the cut is named: the marker says which lines are shown out of how many, and where to
/// carry on from.
pub async fn read(ctx: &ToolContext<'_>, args: &Map<String, Value>) -> Result<String, String> {
    let path = text(args, "path")
        .ok_or_else(|| "read needs `path`".to_string())?
        .to_string();
    let offset = uint(args, "offset");
    let limit = uint(args, "limit");
    let directory = ctx.directory.to_path_buf();
    // The file work runs off the async worker: a FIFO, a socket or a slow mount
    // would otherwise hold that worker — and with it the turn, which is only
    // polled for cancellation between tool calls.
    match tokio::task::spawn_blocking(move || read_file(&directory, &path, offset, limit)).await {
        Ok(result) => result,
        Err(error) => Err(format!("the read could not run: {}", error)),
    }
}

/// The file at `path` inside `directory`, numbered and cut to the caps.
fn read_file(
    directory: &Path,
    path: &str,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<String, String> {
    let full = resolve(directory, path)?;
    if full.is_dir() {
        return Err(format!("`{path}` is a directory; use glob to list it"));
    }
    let too_big = fs::metadata(&full)
        .ok()
        .filter(|meta| meta.len() > MAX_READ);
    if let Some(meta) = too_big {
        return Err(format!(
            "`{path}` is {} MB, too large to read whole; search it or read part of it with bash",
            meta.len() / (1024 * 1024)
        ));
    }
    let bytes = fs::read(&full).map_err(|error| format!("could not read `{path}`: {error}"))?;
    if is_binary(&bytes) {
        return Err(format!(
            "`{path}` is a binary file ({} bytes); not shown",
            bytes.len()
        ));
    }
    let text = decode(bytes);
    if text.is_empty() {
        return Ok("(empty file)".to_string());
    }

    let lines = || text.split_terminator('\n');
    let total = lines().count();
    let offset = offset.unwrap_or(1).max(1) as usize;
    if offset > total {
        return Err(format!(
            "offset {offset} is beyond the end of `{path}` ({total} lines)"
        ));
    }
    let limit = limit.map_or(MAX_LINES, |n| (n as usize).clamp(1, MAX_LINES));

    let mut out = String::new();
    let mut entry = String::new();
    let mut last = offset - 1;
    let mut hit_bytes = false;
    for (index, line) in lines().enumerate().skip(offset - 1).take(limit) {
        entry.clear();
        if line.len() > MAX_LINE_CHARS {
            let _ = writeln!(
                entry,
                "{}\t{}… [line truncated: {} chars]",
                index + 1,
                truncate(line, MAX_LINE_CHARS),
                line.len()
            );
        } else {
            let _ = writeln!(entry, "{}\t{line}", index + 1);
        }
        if !out.is_empty() && out.len() + entry.len() > MAX_BYTES {
            hit_bytes = true;
            break;
        }
        out.push_str(&entry);
        last = index + 1;
    }
    if last < total {
        let why = if hit_bytes {
            format!(" ({}KB limit)", MAX_BYTES / 1024)
        } else {
            String::new()
        };
        out.push_str(&format!(
            "[Showing lines {offset}-{last} of {total}{why}. Use offset={} to continue.]",
            last + 1
        ));
    }
    Ok(out)
}

/// Writes a file in the workspace, creating it and its parent directories, or replacing
/// what was there. Returns one line naming what happened and how big the file now is.
pub async fn write(ctx: &ToolContext<'_>, args: &Map<String, Value>) -> Result<String, String> {
    let path = text(args, "path")
        .ok_or_else(|| "write needs `path`".to_string())?
        .to_string();
    let content = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "write needs `content`".to_string())?
        .to_string();
    let directory = ctx.directory.to_path_buf();
    // Creating the parent folders and renaming over the target are file work:
    // they run off the async worker, as a read does.
    match tokio::task::spawn_blocking(move || write_file(&directory, &path, &content)).await {
        Ok(result) => result,
        Err(error) => Err(format!("the write could not run: {}", error)),
    }
}

/// Writes `content` to `path` inside `directory`, making the folders it needs.
fn write_file(directory: &Path, path: &str, content: &str) -> Result<String, String> {
    let full = resolve(directory, path)?;
    if full.is_dir() {
        return Err(format!("`{path}` is a directory"));
    }
    let parent = full
        .parent()
        .ok_or_else(|| format!("`{path}` has no parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    // The folders just made may have followed a link, so the path is checked again.
    let full = resolve(directory, path)?;
    let existed = full.exists();
    replace_file(&full, content).map_err(|error| format!("could not write `{path}`: {error}"))?;
    let verb = if existed { "Overwrote" } else { "Created" };
    Ok(format!("{verb} {path} ({}).", count(content.len(), "byte")))
}

/// The half-written copy of `full`, beside it so the rename stays on one device.
fn temp_path(full: &Path) -> PathBuf {
    let mut name = full.as_os_str().to_os_string();
    name.push(".the-pair-tmp");
    PathBuf::from(name)
}

/// Replaces the file at `full` with `contents` without ever leaving it half-written.
///
/// The new text goes to `<path>.the-pair-tmp` in the same directory and is renamed
/// over the target, so a disk that fills up or a crash part-way through leaves the
/// old file whole rather than truncated. The target's permissions are carried over,
/// since a fresh temp file would otherwise take the process's default mode, and the
/// temp file is removed if anything fails.
pub(crate) fn replace_file(full: &Path, contents: &str) -> std::io::Result<()> {
    let temp = temp_path(full);
    let written = (|| {
        fs::write(&temp, contents)?;
        if let Ok(target) = fs::metadata(full) {
            fs::set_permissions(&temp, target.permissions())?;
        }
        fs::rename(&temp, full)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

/// The real path `path` names inside the workspace, or why it cannot be named there.
///
/// Absolute paths and `..` are refused before anything is touched; everything else is
/// resolved through the filesystem, so a link that leaves the workspace is caught too.
fn resolve(directory: &Path, path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(format!(
            "`{path}` is an absolute path; give one relative to the workspace"
        ));
    }
    if relative
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(format!(
            "`{path}` leaves the workspace; a path may not contain `..`"
        ));
    }
    let root = fs::canonicalize(directory).unwrap_or_else(|_| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(directory)
    });
    let resolved = real_path(&directory.join(relative));
    if !resolved.starts_with(&root) {
        return Err(format!(
            "`{path}` points outside the workspace, through a link"
        ));
    }
    Ok(resolved)
}

/// The path the filesystem really means: the target itself when it exists, otherwise its
/// nearest existing ancestor with the rest of the path put back, so a link on the way is
/// still seen before anything is written through it.
fn real_path(target: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(target) {
        return real;
    }
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = target.to_path_buf();
    while let Some(name) = cursor.file_name() {
        rest.push(name.to_os_string());
        match cursor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => cursor = parent.to_path_buf(),
            _ => break,
        }
        if let Ok(real) = fs::canonicalize(&cursor) {
            let mut resolved = real;
            for part in rest.iter().rev() {
                resolved.push(part);
            }
            return resolved;
        }
    }
    target.to_path_buf()
}

/// The file as read sees it: the byte-order mark gone, and CRLF shown as LF when every line
/// ends in CRLF, so line numbers stay meaningful on either kind of file.
fn decode(bytes: Vec<u8>) -> String {
    let raw = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    };
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let mut newlines = 0;
    let mut all_crlf = true;
    for (index, _) in raw.match_indices('\n') {
        newlines += 1;
        if index == 0 || raw.as_bytes()[index - 1] != b'\r' {
            all_crlf = false;
        }
    }
    if newlines > 0 && all_crlf {
        raw.replace("\r\n", "\n")
    } else {
        raw.to_string()
    }
}

/// A NUL byte anywhere near the start means the file is not text.
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0)
}

/// The first `max` bytes of `text`, cut on a character boundary.
fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// A tool argument that must be a string with something in it.
fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    match args.get(key).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

/// A tool argument holding a whole number, whether it arrived as a number or a string.
fn uint(args: &Map<String, Value>, key: &str) -> Option<u64> {
    match args.get(key) {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// `n` and `noun`, the noun plural unless `n` is 1: "1 byte", "12 bytes".
fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolPolicy;
    use serde_json::json;
    use std::ops::Deref;
    use uuid::Uuid;

    /// A scratch folder in the temporary folder, removed when it goes out of scope.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("the-pair-files-{name}-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("scratch folder");
            Self(path)
        }

        fn file(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent folder");
            }
            fs::write(&path, contents).expect("scratch file");
            path
        }
    }

    impl Deref for Scratch {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn args(pairs: &[(&str, Value)]) -> Map<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[tokio::test]
    async fn read_returns_the_contents_with_line_numbers() {
        let scratch = Scratch::new("read");
        scratch.file("notes/a.txt", "alpha\nbeta\n");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let out = read(&ctx, &args(&[("path", json!("notes/a.txt"))]))
            .await
            .expect("read");
        assert_eq!(out, "1\talpha\n2\tbeta\n");
    }

    #[tokio::test]
    async fn read_carries_on_from_an_offset_with_a_limit() {
        let scratch = Scratch::new("read-offset");
        scratch.file("a.txt", "a\nb\nc\n");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let out = read(
            &ctx,
            &args(&[
                ("path", json!("a.txt")),
                ("offset", json!(2)),
                ("limit", json!(1)),
            ]),
        )
        .await
        .expect("read");
        assert_eq!(
            out,
            "2\tb\n[Showing lines 2-2 of 3. Use offset=3 to continue.]"
        );
        let error = read(
            &ctx,
            &args(&[("path", json!("a.txt")), ("offset", json!(9))]),
        )
        .await
        .expect_err("an offset past the end");
        assert!(error.contains("beyond the end"), "{error}");
    }

    #[tokio::test]
    async fn read_marks_a_file_it_could_not_show_whole() {
        let scratch = Scratch::new("read-cap");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let many: String = (1..=2500).map(|n| format!("line {n}\n")).collect();
        scratch.file("many.txt", &many);
        let out = read(&ctx, &args(&[("path", json!("many.txt"))]))
            .await
            .expect("read");
        assert!(out.starts_with("1\tline 1\n"), "{}", &out[..20]);
        assert!(out.ends_with("[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]"));

        let wide = "x".repeat(600);
        let fat: String = (1..=100).map(|n| format!("{n} {wide}\n")).collect();
        scratch.file("fat.txt", &fat);
        let out = read(&ctx, &args(&[("path", json!("fat.txt"))]))
            .await
            .expect("read");
        assert!(out.contains("(50KB limit)"), "{}", &out[..80]);
        assert!(out.contains("Use offset="), "{}", &out[..80]);

        scratch.file("long.txt", &format!("{}tail\n", "y".repeat(2996)));
        let out = read(&ctx, &args(&[("path", json!("long.txt"))]))
            .await
            .expect("read");
        assert!(out.contains("[line truncated: 3000 chars]"), "{}", out);
    }

    #[tokio::test]
    async fn read_refuses_a_file_too_large_to_read_whole() {
        let scratch = Scratch::new("read-big");
        let big = scratch.file("big.log", "");
        std::fs::File::create(&big)
            .expect("scratch file")
            .set_len(MAX_READ + 1)
            .expect("a sparse file");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let error = read(&ctx, &args(&[("path", json!("big.log"))]))
            .await
            .expect_err("a file past the read limit");
        assert!(error.contains("too large to read whole"), "{error}");
        assert!(error.contains("10 MB"), "{error}");
    }

    #[tokio::test]
    async fn read_reports_an_empty_file() {
        let scratch = Scratch::new("read-empty");
        scratch.file("empty.txt", "");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        assert_eq!(
            read(&ctx, &args(&[("path", json!("empty.txt"))]))
                .await
                .expect("read"),
            "(empty file)"
        );
    }

    #[tokio::test]
    async fn read_refuses_a_directory_a_missing_file_and_binary_bytes() {
        let scratch = Scratch::new("read-refuse");
        fs::create_dir_all(scratch.join("dir")).expect("scratch folder");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let error = read(&ctx, &args(&[("path", json!("dir"))]))
            .await
            .expect_err("a directory");
        assert!(error.contains("is a directory"), "{error}");
        let error = read(&ctx, &args(&[("path", json!("nope.txt"))]))
            .await
            .expect_err("a missing file");
        assert!(error.contains("could not read"), "{error}");
        scratch.file("blob.bin", &String::from_utf8_lossy(&[0x00, 0x01, 0x02]));
        let error = read(&ctx, &args(&[("path", json!("blob.bin"))]))
            .await
            .expect_err("binary bytes");
        assert!(error.contains("binary file"), "{error}");
    }

    #[tokio::test]
    async fn read_refuses_paths_that_leave_the_workspace() {
        let scratch = Scratch::new("read-escape");
        scratch.file("a.txt", "inside\n");
        let outside = Scratch::new("read-outside");
        fs::write(outside.join("secret.txt"), "outside\n").expect("file outside the workspace");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };

        for path in [
            "/etc/hosts",
            "../secret.txt",
            "a/../../secret.txt",
            "notes/../a.txt",
        ] {
            let error = read(&ctx, &args(&[("path", json!(path))]))
                .await
                .expect_err(path);
            assert!(error.contains("workspace"), "{path} gave: {error}");
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&*outside, scratch.join("link")).expect("folder link");
            std::os::unix::fs::symlink(outside.join("secret.txt"), scratch.join("file.txt"))
                .expect("file link");
            for path in ["link/secret.txt", "file.txt"] {
                let error = read(&ctx, &args(&[("path", json!(path))]))
                    .await
                    .expect_err(path);
                assert!(
                    error.contains("outside the workspace"),
                    "{path} gave: {error}"
                );
            }
        }
    }

    #[tokio::test]
    async fn read_shows_crlf_and_a_bom_as_plain_lines() {
        let scratch = Scratch::new("read-crlf");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        scratch.file("crlf.txt", "\u{feff}one\r\ntwo\r\n");
        assert_eq!(
            read(&ctx, &args(&[("path", json!("crlf.txt"))]))
                .await
                .expect("read"),
            "1\tone\n2\ttwo\n"
        );
        scratch.file("mixed.txt", "one\r\ntwo\n");
        assert_eq!(
            read(&ctx, &args(&[("path", json!("mixed.txt"))]))
                .await
                .expect("read"),
            "1\tone\r\n2\ttwo\n"
        );
    }

    #[tokio::test]
    async fn write_creates_the_file_and_its_parents() {
        let scratch = Scratch::new("write-new");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let out = write(
            &ctx,
            &args(&[
                ("path", json!("src/deep/a.rs")),
                ("content", json!("fn main() {}\n")),
            ]),
        )
        .await
        .expect("write");
        assert_eq!(out, "Created src/deep/a.rs (13 bytes).");
        assert_eq!(
            fs::read_to_string(scratch.join("src/deep/a.rs")).expect("the written file"),
            "fn main() {}\n"
        );
    }

    #[tokio::test]
    async fn write_overwrites_what_was_there() {
        let scratch = Scratch::new("write-over");
        scratch.file("a.txt", "old\n");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let out = write(
            &ctx,
            &args(&[("path", json!("a.txt")), ("content", json!("new\n"))]),
        )
        .await
        .expect("write");
        assert_eq!(out, "Overwrote a.txt (4 bytes).");
        assert_eq!(
            fs::read_to_string(scratch.join("a.txt")).expect("the written file"),
            "new\n"
        );
    }

    #[tokio::test]
    async fn write_replaces_the_file_without_leaving_a_half_written_copy() {
        let scratch = Scratch::new("write-atomic");
        scratch.file("a.txt", "old\n");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        write(
            &ctx,
            &args(&[("path", json!("a.txt")), ("content", json!("new\n"))]),
        )
        .await
        .expect("write");
        assert_eq!(
            fs::read_to_string(scratch.join("a.txt")).expect("the file"),
            "new\n"
        );
        // Nothing is left beside the target, whatever happened.
        let leftovers: Vec<String> = fs::read_dir(&*scratch)
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
    }

    #[tokio::test]
    async fn a_write_that_cannot_finish_leaves_the_old_file_whole() {
        let scratch = Scratch::new("write-fail");
        scratch.file("a.txt", "old\n");
        // Something already occupies the temp name, so the copy cannot be
        // written and the target is never touched: an in-place write would have
        // truncated it before it ran out of room.
        fs::create_dir_all(scratch.join("a.txt.the-pair-tmp")).expect("a squatting folder");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let error = write(
            &ctx,
            &args(&[("path", json!("a.txt")), ("content", json!("new\n"))]),
        )
        .await
        .expect_err("the temp file could not be written");
        assert!(error.contains("could not write `a.txt`"), "{}", error);
        assert_eq!(
            fs::read_to_string(scratch.join("a.txt")).expect("the old file"),
            "old\n"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn write_keeps_the_permissions_the_file_already_had() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("write-mode");
        let file = scratch.file("a.txt", "old\n");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("mode");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        write(
            &ctx,
            &args(&[("path", json!("a.txt")), ("content", json!("new\n"))]),
        )
        .await
        .expect("write");
        assert_eq!(
            fs::metadata(&file).expect("the file").permissions().mode() & 0o777,
            0o600
        );
    }

    #[tokio::test]
    async fn file_work_does_not_hold_the_async_worker() {
        let scratch = Scratch::new("yield");
        scratch.file("a.txt", "alpha\n");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        // A read that ran on the worker would starve the heartbeat, and the two
        // together would never come back.
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let reading = args(&[("path", json!("a.txt"))]);
            let writing = args(&[("path", json!("b.txt")), ("content", json!("beta\n"))]);
            tokio::join!(read(&ctx, &reading), write(&ctx, &writing), beat())
        })
        .await
        .expect("the file work held the worker");
        assert_eq!(outcome.0.expect("read"), "1\talpha\n");
        assert!(outcome.1.is_ok(), "{:?}", outcome.1);
        assert!(outcome.2, "the heartbeat was starved");
    }

    /// Runs five times, as a task that shares the worker does.
    async fn beat() -> bool {
        for _ in 0..5 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        true
    }

    #[tokio::test]
    async fn write_needs_a_path_and_content() {
        let scratch = Scratch::new("write-args");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let error = write(&ctx, &args(&[("content", json!("x"))]))
            .await
            .expect_err("no path");
        assert!(error.contains("`path`"), "{error}");
        let error = write(&ctx, &args(&[("path", json!("a.txt"))]))
            .await
            .expect_err("no content");
        assert!(error.contains("`content`"), "{error}");
        let error = write(
            &ctx,
            &args(&[("path", json!("   ")), ("content", json!("x"))]),
        )
        .await
        .expect_err("a blank path");
        assert!(error.contains("`path`"), "{error}");
    }

    #[tokio::test]
    async fn write_refuses_paths_that_leave_the_workspace() {
        let scratch = Scratch::new("write-escape");
        let outside = Scratch::new("write-outside");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        for path in [
            "/tmp/the-pair-escape.txt",
            "../escape.txt",
            "a/../../escape.txt",
        ] {
            let error = write(
                &ctx,
                &args(&[("path", json!(path)), ("content", json!("x"))]),
            )
            .await
            .expect_err(path);
            assert!(error.contains("workspace"), "{path} gave: {error}");
        }
        assert!(!outside.join("escape.txt").exists());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&*outside, scratch.join("link")).expect("folder link");
            let error = write(
                &ctx,
                &args(&[("path", json!("link/escape.txt")), ("content", json!("x"))]),
            )
            .await
            .expect_err("a linked folder");
            assert!(error.contains("outside the workspace"), "{error}");
            assert!(!outside.join("escape.txt").exists());
        }
    }

    #[tokio::test]
    async fn write_refuses_a_directory() {
        let scratch = Scratch::new("write-dir");
        fs::create_dir_all(scratch.join("dir")).expect("scratch folder");
        let ctx = ToolContext {
            directory: &scratch,
            policy: ToolPolicy::Full,
        };
        let error = write(
            &ctx,
            &args(&[("path", json!("dir")), ("content", json!("x"))]),
        )
        .await
        .expect_err("a directory");
        assert!(error.contains("is a directory"), "{error}");
    }
}
