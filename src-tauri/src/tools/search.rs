//! Searching the workspace: `grep` over file contents, `glob` over paths.
//!
//! Both tools walk the workspace with `std::fs` — there is no `ignore` or
//! `globset` here — so this module also holds what those crates would have
//! given: a small pattern matcher, shared by both tools, and the walk itself.
//!
//! Every path argument goes through [`Workspace::resolve`], which resolves it
//! against the pair's workspace and refuses anything that leaves it: absolute
//! paths, `..` segments, and symlinks that point somewhere else. A tool never
//! reads or writes outside the workspace it was given.
//!
//! `grep`'s `pattern` is a *subset* of regular expressions, not the full Rust
//! `regex` syntax, and it is matched one line at a time:
//!
//! - literals, `.` (any character), `*`, `+`, `?`, `{n}`, `{n,}`, `{n,m}`
//! - `^` and `$` for the start and end of a line
//! - character classes: `[a-z]`, `[^abc]`, and the `\d`, `\w`, `\s` shorthands
//!   with their uppercase negations
//! - groups with alternation: `(cat|dog)`, `(ab)+`
//! - a backslash escapes the next character: `\.`, `\*`, `\\`
//!
//! A plain substring is always a valid pattern, so `alpha` and `fn main` work
//! exactly as written. Anything outside the subset — `\b`, `(?i)`, a pattern
//! spanning lines — is an error naming what is missing rather than a silently
//! different answer, and the model can retry with a simpler pattern.
//!
//! A repetition count is capped at [`MAX_REPEAT`]: the matcher compiles a count
//! into that many instructions before it reads a single byte, so a huge one is
//! refused rather than allowed to spend the app's memory.
//!
//! `glob`'s `pattern` is matched against paths relative to the directory it
//! searches: `*` and `?` stay inside one path component, `[...]` is a character
//! class (negated with `!`, as globs do), and a `**` component matches zero or
//! more whole components, so `src/**/*.rs` finds both `src/a.rs` and
//! `src/deep/b.rs`.
//!
//! Results are capped — [`MAX_MATCHES`] matches, each line cut at
//! [`MAX_MATCH_LINE`] characters, for `grep`, and [`MAX_FILES`] paths for
//! `glob` — and the cap is named in the result, so a truncated answer is never
//! mistaken for a complete one.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};

use super::ToolContext;

/// Matches `grep` reports before it stops and says so.
const MAX_MATCHES: usize = 100;
/// A matching line longer than this is cut in the middle.
const MAX_MATCH_LINE: usize = 500;
/// Files over this size are not searched; `bash` (`rg`, `sed -n`) can search them.
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// Paths `glob` reports before it stops and says so.
const MAX_FILES: usize = 1000;
/// How deep the walk descends before it stops and says so.
const MAX_DEPTH: usize = 32;
/// Steps the matcher may spend per search. A pattern that backtracks without
/// consuming input (`(a?)*`) stops the search instead of the agent, as does a
/// glob whose `**` components walk too many combinations.
const MAX_STEPS: usize = 50_000_000;
/// Backtracking alternatives kept at once, so one line cannot spend the memory
/// budget either.
const MAX_STACK: usize = 1_000_000;
/// The largest repetition count `{n}`, `{n,}` or `{n,m}` may name. A count is
/// compiled into that many instructions before a single byte is read, so a huge
/// one would spend the memory budget — `a{4294967295}` is ~137 GB of
/// instructions — and take the whole app down rather than answer.
const MAX_REPEAT: usize = 1000;
/// Directories the walk never enters: build output, dependencies, caches — and
/// `.worktrees`, where The Pair itself keeps other pairs' checkouts of this same
/// repository. Searching those would answer with every match several times
/// over, from copies the pair is not working in, and spend the result cap and
/// the pattern's step budget on them. A pair that lives in one of them is not
/// affected: its own worktree is the root of the walk, never an entry in it.
const NOISE_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".worktrees",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".cache",
    ".parcel-cache",
    ".turbo",
    "coverage",
    ".pytest_cache",
    ".mypy_cache",
];

/// Searches file contents for `pattern` and reports `path:line: text` for every
/// match, files in a stable order.
///
/// `pattern` is the regular-expression subset described in the module docs; a
/// plain substring always works. `path` narrows the search to one directory or
/// file inside the workspace and defaults to the whole workspace. Binary files,
/// files over 10 MB, and noise directories (`.git`, `node_modules`, `target`, …)
/// are skipped and named in a note. At most [`MAX_MATCHES`] matches come back,
/// each line cut at [`MAX_MATCH_LINE`] characters, with a note naming the
/// limit.
pub async fn grep(ctx: &ToolContext<'_>, args: &Map<String, Value>) -> Result<String, String> {
    let pattern = match args.get("pattern").and_then(Value::as_str) {
        Some(pattern) => pattern.to_string(),
        None => return Err("grep needs `pattern`.".to_string()),
    };
    let asked = args.get("path").and_then(Value::as_str).map(str::to_string);
    let directory = ctx.directory.to_path_buf();
    match tokio::task::spawn_blocking(move || {
        let workspace = Workspace::new(&directory)?;
        let root = search_root(&workspace, asked.as_deref())?;
        grep_tree(&workspace, &root, &pattern)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("the search could not run: {}", error)),
    }
}

/// Lists the files whose path matches `pattern`, relative to the directory
/// searched.
///
/// `pattern` is a glob as described in the module docs; it is relative, may not
/// contain `..`, and is matched against paths relative to `path` (the workspace
/// by default). Noise directories are skipped. At most [`MAX_FILES`] paths come
/// back, with a note naming the limit, and a pattern whose `**` components walk
/// too many combinations stops the search and says so.
pub async fn glob(ctx: &ToolContext<'_>, args: &Map<String, Value>) -> Result<String, String> {
    let pattern = match args.get("pattern").and_then(Value::as_str) {
        Some(pattern) => pattern.to_string(),
        None => return Err("glob needs `pattern`.".to_string()),
    };
    let asked = args.get("path").and_then(Value::as_str).map(str::to_string);
    let directory = ctx.directory.to_path_buf();
    match tokio::task::spawn_blocking(move || {
        let workspace = Workspace::new(&directory)?;
        let root = search_root(&workspace, asked.as_deref())?;
        glob_tree(&workspace, &root, &pattern)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("the search could not run: {}", error)),
    }
}

/// The directory a search starts from: `asked` inside the workspace, or the
/// workspace itself when the call named no path.
fn search_root(workspace: &Workspace, asked: Option<&str>) -> Result<PathBuf, String> {
    let root = match asked {
        Some(asked) => workspace.resolve(asked)?,
        None => workspace.root.clone(),
    };
    if !root.exists() {
        return Err(format!(
            "`{}` does not exist in the workspace",
            asked.unwrap_or(".")
        ));
    }
    Ok(root)
}

// ---------------------------------------------------------------------------
// The workspace, and the gate every path goes through
// ---------------------------------------------------------------------------

/// The pair's workspace, canonicalized once so containment is checked against
/// the real directory rather than the symlink the user picked.
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn new(directory: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(directory).map_err(|error| {
            format!(
                "the workspace {} cannot be read: {}",
                directory.display(),
                error
            )
        })?;
        Ok(Self { root })
    }

    /// Resolves `path` against the workspace, refusing anything that leaves it.
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let trimmed = path.trim();
        if trimmed.is_empty() || trimmed == "." {
            return Ok(self.root.clone());
        }
        if trimmed.starts_with('~') {
            return Err(format!("`{}` is not a path inside the workspace", path));
        }
        let relative = Path::new(trimmed);
        if relative.is_absolute() {
            return Err(format!(
                "`{}` must be relative to the workspace, not absolute",
                path
            ));
        }
        for component in relative.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(format!("`{}` is not a path inside the workspace", path)),
            }
        }
        self.confine(&self.root.join(relative), path)
    }

    /// The path as the model sees it: relative to the workspace when it is
    /// inside it, absolute otherwise.
    fn show(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(relative) => {
                let shown = relative.display().to_string();
                if shown.is_empty() {
                    ".".to_string()
                } else {
                    shown
                }
            }
            Err(_) => path.display().to_string(),
        }
    }

    /// `target` must resolve to something inside the workspace, symlinks
    /// followed: a link out of the workspace is refused before it is opened.
    fn confine(&self, target: &Path, asked: &str) -> Result<PathBuf, String> {
        if let Ok(metadata) = fs::symlink_metadata(target) {
            if metadata.file_type().is_symlink() {
                let resolved = target
                    .canonicalize()
                    .map_err(|_| format!("`{}` is a broken symlink", asked))?;
                if !resolved.starts_with(&self.root) {
                    return Err(format!(
                        "`{}` is a symlink that leaves the workspace",
                        asked
                    ));
                }
                return Ok(resolved);
            }
        }
        // The path may not exist yet, so walk up to the nearest ancestor that
        // does and put the missing components back on the end of it.
        let mut missing: Vec<OsString> = Vec::new();
        let mut cursor = target.to_path_buf();
        loop {
            match cursor.canonicalize() {
                Ok(resolved) => {
                    let mut full = resolved;
                    for name in missing.iter().rev() {
                        full.push(name);
                    }
                    if !full.starts_with(&self.root) {
                        return Err(format!("`{}` leaves the workspace", asked));
                    }
                    return Ok(full);
                }
                Err(_) => match cursor.file_name() {
                    Some(name) => {
                        missing.push(name.to_os_string());
                        if !cursor.pop() {
                            return Err(format!(
                                "`{}` cannot be resolved inside the workspace",
                                asked
                            ));
                        }
                    }
                    None => {
                        return Err(format!(
                            "`{}` cannot be resolved inside the workspace",
                            asked
                        ))
                    }
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The pattern matcher
// ---------------------------------------------------------------------------

/// What one piece of a pattern matches against a single character.
#[derive(Clone)]
enum Atom {
    Literal(char),
    Any,
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
}

/// One entry of a character class.
#[derive(Clone)]
enum ClassItem {
    Single(char),
    Range(char, char),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}

/// One piece of a pattern: `^`, `$`, or a body repeated `min` to `max` times.
enum Node {
    Start,
    End,
    Piece {
        body: Body,
        min: usize,
        max: Option<usize>,
    },
}

/// What a piece is made of: one atom, or a group's alternation branches.
enum Body {
    Atom(Atom),
    /// One branch is the plain case; several are alternation.
    Branches(Vec<Vec<Node>>),
}

/// One step of the compiled pattern, run as a backtracking program.
enum Inst {
    Char(Atom),
    Split(usize, usize),
    Jmp(usize),
    Start,
    End,
    Match,
}

/// Compiles `pattern` into a program. `glob` reads `*` and `?` as glob wildcards
/// rather than quantifiers.
fn compile(pattern: &str, glob: bool) -> Result<Vec<Inst>, String> {
    if pattern.is_empty() {
        return Err("the pattern must not be empty".to_string());
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut at = 0usize;
    let branches = alternation(&chars, &mut at, 0, glob)?;
    if at < chars.len() {
        return Err("the pattern has an unmatched `)`".to_string());
    }
    let nodes = vec![Node::Piece {
        body: Body::Branches(branches),
        min: 1,
        max: Some(1),
    }];
    let mut program = Vec::new();
    emit(&nodes, &mut program);
    program.push(Inst::Match);
    Ok(program)
}

fn emit(nodes: &[Node], program: &mut Vec<Inst>) {
    for node in nodes {
        match node {
            Node::Start => program.push(Inst::Start),
            Node::End => program.push(Inst::End),
            Node::Piece { body, min, max } => emit_repeat(body, *min, *max, program),
        }
    }
}

/// `min` copies of `body`, then a loop when there is no upper bound and optional
/// copies when there is one.
fn emit_repeat(body: &Body, min: usize, max: Option<usize>, program: &mut Vec<Inst>) {
    for _ in 0..min {
        emit_body(body, program);
    }
    match max {
        None => {
            let start = program.len();
            program.push(Inst::Split(0, 0));
            emit_body(body, program);
            program.push(Inst::Jmp(start));
            let end = program.len();
            program[start] = Inst::Split(start + 1, end);
        }
        Some(max) => {
            let mut splits = Vec::new();
            for _ in min..max {
                let at = program.len();
                program.push(Inst::Split(0, 0));
                emit_body(body, program);
                splits.push(at);
            }
            let end = program.len();
            for at in splits {
                program[at] = Inst::Split(at + 1, end);
            }
        }
    }
}

fn emit_body(body: &Body, program: &mut Vec<Inst>) {
    match body {
        Body::Atom(atom) => program.push(Inst::Char(atom.clone())),
        Body::Branches(branches) => {
            if branches.len() == 1 {
                emit(&branches[0], program);
                return;
            }
            let mut jumps = Vec::new();
            for branch in &branches[..branches.len() - 1] {
                let at = program.len();
                program.push(Inst::Split(0, 0));
                emit(branch, program);
                jumps.push(program.len());
                program.push(Inst::Jmp(0));
                let next = program.len();
                program[at] = Inst::Split(at + 1, next);
            }
            emit(&branches[branches.len() - 1], program);
            let end = program.len();
            for at in jumps {
                program[at] = Inst::Jmp(end);
            }
        }
    }
}

/// Runs `program` against `text` from `from`, spending from `steps`; false when
/// the budget runs out, which the caller reports as a pattern that is too slow.
/// `to_end` makes a match cover the rest of `text`, as a glob component must.
fn run(program: &[Inst], text: &[char], from: usize, steps: &mut usize, to_end: bool) -> bool {
    let mut alternatives: Vec<(usize, usize)> = Vec::new();
    let mut pc = 0usize;
    let mut pos = from;
    loop {
        if *steps == 0 || alternatives.len() >= MAX_STACK {
            return false;
        }
        *steps -= 1;
        match &program[pc] {
            Inst::Match => {
                if !to_end || pos == text.len() {
                    return true;
                }
            }
            Inst::Start if pos == 0 => pc += 1,
            Inst::End if pos == text.len() => pc += 1,
            Inst::Char(atom) if pos < text.len() && atom_matches(atom, text[pos]) => {
                pc += 1;
                pos += 1;
            }
            Inst::Jmp(target) => pc = *target,
            Inst::Split(first, second) => {
                alternatives.push((*second, pos));
                pc = *first;
            }
            _ => match alternatives.pop() {
                Some((next_pc, next_pos)) => {
                    pc = next_pc;
                    pos = next_pos;
                }
                None => return false,
            },
        }
    }
}

/// Whether the pattern matches anywhere in `text`.
///
/// A program that must start with one character — every plain substring, and
/// most expressions — is only tried where that character occurs, which is what
/// keeps searching a large tree cheap.
fn search(program: &[Inst], text: &str, steps: &mut usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    match program.first() {
        // A character cannot match nothing, so no start position is missed by
        // stopping one short of the end.
        Some(Inst::Char(atom)) => {
            for start in 0..chars.len() {
                if !atom_matches(atom, chars[start]) {
                    continue;
                }
                if run(program, &chars, start, steps, false) {
                    return true;
                }
                if *steps == 0 {
                    return false;
                }
            }
            false
        }
        // Anything else may match nothing at all, including at the very end.
        _ => {
            for start in 0..=chars.len() {
                if run(program, &chars, start, steps, false) {
                    return true;
                }
                if *steps == 0 {
                    return false;
                }
            }
            false
        }
    }
}

/// Whether the pattern matches `text` completely, as a glob component must.
fn full_match(program: &[Inst], text: &str, steps: &mut usize) -> bool {
    let chars: Vec<char> = text.chars().collect();
    run(program, &chars, 0, steps, true)
}

fn atom_matches(atom: &Atom, ch: char) -> bool {
    match atom {
        Atom::Literal(expected) => *expected == ch,
        Atom::Any => true,
        Atom::Class { negated, items } => {
            let hit = items.iter().any(|item| class_item_matches(item, ch));
            if *negated {
                !hit
            } else {
                hit
            }
        }
    }
}

fn class_item_matches(item: &ClassItem, ch: char) -> bool {
    match item {
        ClassItem::Single(expected) => *expected == ch,
        ClassItem::Range(start, end) => *start <= ch && ch <= *end,
        ClassItem::Digit => ch.is_ascii_digit(),
        ClassItem::NotDigit => !ch.is_ascii_digit(),
        ClassItem::Word => ch.is_alphanumeric() || ch == '_',
        ClassItem::NotWord => !(ch.is_alphanumeric() || ch == '_'),
        ClassItem::Space => ch.is_whitespace(),
        ClassItem::NotSpace => !ch.is_whitespace(),
    }
}

/// `a|b|c` at this level; stops at `)` when `depth` says we are inside a group.
fn alternation(
    chars: &[char],
    at: &mut usize,
    depth: usize,
    glob: bool,
) -> Result<Vec<Vec<Node>>, String> {
    let mut branches: Vec<Vec<Node>> = vec![Vec::new()];
    while *at < chars.len() {
        let ch = chars[*at];
        if ch == '|' {
            *at += 1;
            branches.push(Vec::new());
            continue;
        }
        if ch == ')' {
            if depth == 0 {
                return Err("the pattern has an unmatched `)`".to_string());
            }
            break;
        }
        let node = piece(chars, at, depth, glob)?;
        match branches.last_mut() {
            Some(branch) => branch.push(node),
            None => return Err("the pattern could not be read".to_string()),
        }
    }
    Ok(branches)
}

/// One piece: an anchor, a literal, a class, a group or a wildcard, with its
/// quantifier.
fn piece(chars: &[char], at: &mut usize, depth: usize, glob: bool) -> Result<Node, String> {
    let ch = chars[*at];
    if !glob && ch == '^' {
        *at += 1;
        return Ok(Node::Start);
    }
    if !glob && ch == '$' {
        *at += 1;
        return Ok(Node::End);
    }
    let body = if !glob && ch == '(' {
        *at += 1;
        let branches = alternation(chars, at, depth + 1, glob)?;
        if chars.get(*at) != Some(&')') {
            return Err("the pattern has an unclosed `(`".to_string());
        }
        *at += 1;
        Body::Branches(branches)
    } else if ch == '[' {
        *at += 1;
        Body::Atom(class(chars, at, glob)?)
    } else if ch == '\\' {
        *at += 1;
        Body::Atom(escape(chars, at)?)
    } else if glob && ch == '*' {
        // `**` inside one component behaves like `*`; a whole `**` component is
        // the path matcher's business.
        while chars.get(*at) == Some(&'*') {
            *at += 1;
        }
        return Ok(Node::Piece {
            body: Body::Atom(Atom::Any),
            min: 0,
            max: None,
        });
    } else if glob && ch == '?' {
        *at += 1;
        return Ok(Node::Piece {
            body: Body::Atom(Atom::Any),
            min: 1,
            max: Some(1),
        });
    } else if !glob && ch == '.' {
        *at += 1;
        Body::Atom(Atom::Any)
    } else if !glob && (ch == '*' || ch == '+' || ch == '?') {
        return Err(format!("the pattern has nothing for `{}` to repeat", ch));
    } else {
        *at += 1;
        Body::Atom(Atom::Literal(ch))
    };
    quantified(body, chars, at, glob)
}

/// Reads the quantifier after a piece, if there is one.
fn quantified(body: Body, chars: &[char], at: &mut usize, glob: bool) -> Result<Node, String> {
    let (min, max) = match chars.get(*at) {
        Some('*') if !glob => {
            *at += 1;
            (0, None)
        }
        Some('+') if !glob => {
            *at += 1;
            (1, None)
        }
        Some('?') if !glob => {
            *at += 1;
            (0, Some(1))
        }
        Some('{') if !glob => match bounds(chars, at)? {
            Some(found) => found,
            None => {
                return Ok(Node::Piece {
                    body,
                    min: 1,
                    max: Some(1),
                })
            }
        },
        _ => {
            return Ok(Node::Piece {
                body,
                min: 1,
                max: Some(1),
            })
        }
    };
    // A lazy marker changes nothing: the matcher only answers whether a match
    // exists, not which one it prefers.
    if chars.get(*at) == Some(&'?') {
        *at += 1;
    }
    Ok(Node::Piece { body, min, max })
}

/// `{n}`, `{n,}` or `{n,m}`. `Ok(None)` means the braces were not a quantifier
/// and the `{` is an ordinary character; an error means they looked like one
/// but cannot be.
fn bounds(chars: &[char], at: &mut usize) -> Result<Option<(usize, Option<usize>)>, String> {
    let mut index = *at + 1;
    let mut low = String::new();
    while chars.get(index).is_some_and(|ch| ch.is_ascii_digit()) {
        low.push(chars[index]);
        index += 1;
    }
    if low.is_empty() {
        return Ok(None);
    }
    let min: usize = low
        .parse()
        .map_err(|_| "the pattern has a repetition count that is too large".to_string())?;
    let max =
        if chars.get(index) == Some(&',') {
            index += 1;
            let mut high = String::new();
            while chars.get(index).is_some_and(|ch| ch.is_ascii_digit()) {
                high.push(chars[index]);
                index += 1;
            }
            if high.is_empty() {
                None
            } else {
                Some(high.parse().map_err(|_| {
                    "the pattern has a repetition count that is too large".to_string()
                })?)
            }
        } else {
            Some(min)
        };
    if chars.get(index) != Some(&'}') {
        return Ok(None);
    }
    // The braces really are a quantifier, so the count is about to be compiled
    // into that many instructions: refuse a count that cannot be afforded before
    // it is emitted.
    if min > MAX_REPEAT || max.is_some_and(|max| max > MAX_REPEAT) {
        return Err(format!(
            "the pattern has a repetition count above {}, which this matcher does not allow",
            MAX_REPEAT
        ));
    }
    if let Some(max) = max {
        if max < min {
            return Err("the pattern has a repetition quantifier that is out of order".to_string());
        }
    }
    *at = index + 1;
    Ok(Some((min, max)))
}

/// `[...]` after the opening bracket: ranges, negation, and the shorthands.
/// A regex class is negated with `^`, a glob class with `!`.
fn class(chars: &[char], at: &mut usize, glob: bool) -> Result<Atom, String> {
    let mut negated = false;
    match chars.get(*at) {
        Some('^') if !glob => {
            negated = true;
            *at += 1;
        }
        Some('!') if glob => {
            negated = true;
            *at += 1;
        }
        _ => {}
    }
    let mut items: Vec<ClassItem> = Vec::new();
    let mut first = true;
    loop {
        let ch = match chars.get(*at) {
            Some(']') if !first => {
                *at += 1;
                break;
            }
            Some(ch) => *ch,
            None => return Err("the pattern has an unclosed `[`".to_string()),
        };
        first = false;
        if ch == '\\' {
            *at += 1;
            let esc = match chars.get(*at) {
                Some(esc) => *esc,
                None => return Err("the pattern ends with a dangling `\\`".to_string()),
            };
            *at += 1;
            items.push(class_item(esc)?);
            continue;
        }
        // `a-z` is a range; a `-` last or just before `]` is a literal.
        if chars.get(*at + 1) == Some(&'-') {
            if let Some(&end) = chars.get(*at + 2) {
                if end != ']' {
                    if end < ch {
                        return Err(format!("the pattern has a reversed range `{}-{}`", ch, end));
                    }
                    items.push(ClassItem::Range(ch, end));
                    *at += 3;
                    continue;
                }
            }
        }
        items.push(ClassItem::Single(ch));
        *at += 1;
    }
    if items.is_empty() {
        return Err("the pattern has an empty `[]` class".to_string());
    }
    Ok(Atom::Class { negated, items })
}

/// `\d`, `\w`, `\s` and their uppercase negations, as a class item.
fn class_item(esc: char) -> Result<ClassItem, String> {
    Ok(match esc {
        'd' => ClassItem::Digit,
        'D' => ClassItem::NotDigit,
        'w' => ClassItem::Word,
        'W' => ClassItem::NotWord,
        's' => ClassItem::Space,
        'S' => ClassItem::NotSpace,
        'n' => ClassItem::Single('\n'),
        't' => ClassItem::Single('\t'),
        'r' => ClassItem::Single('\r'),
        other if other.is_ascii_punctuation() => ClassItem::Single(other),
        other => {
            return Err(format!(
                "the pattern uses `\\{}`, which this matcher does not support",
                other
            ))
        }
    })
}

/// A backslash escape outside a class: a shorthand, a control character, or the
/// escaped character itself.
fn escape(chars: &[char], at: &mut usize) -> Result<Atom, String> {
    let esc = match chars.get(*at) {
        Some(esc) => *esc,
        None => return Err("the pattern ends with a dangling `\\`".to_string()),
    };
    *at += 1;
    Ok(match esc {
        'd' | 'D' | 'w' | 'W' | 's' | 'S' => Atom::Class {
            negated: false,
            items: vec![class_item(esc)?],
        },
        'n' => Atom::Literal('\n'),
        't' => Atom::Literal('\t'),
        'r' => Atom::Literal('\r'),
        other if other.is_ascii_punctuation() => Atom::Literal(other),
        other => {
            return Err(format!(
                "the pattern uses `\\{}`, which this matcher does not support",
                other
            ))
        }
    })
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// What the walk found: every file under the root in a stable order, plus what
/// it could not finish.
struct Walk {
    files: Vec<PathBuf>,
    problems: Problems,
    /// The walk stopped descending at [`MAX_DEPTH`].
    depth_limited: bool,
}

/// The files under `root`, noise directories left out. A root that is a file is
/// that one file, so `grep` can be pointed at a single path.
fn walk(root: &Path) -> Walk {
    let mut found = Walk {
        files: Vec::new(),
        problems: Problems::default(),
        depth_limited: false,
    };
    if root.is_file() {
        found.files.push(root.to_path_buf());
        return found;
    }
    descend(root, 0, &mut found);
    found
}

fn descend(directory: &Path, depth: usize, found: &mut Walk) {
    if depth >= MAX_DEPTH {
        found.depth_limited = true;
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            found.problems.record(directory, error);
            return;
        }
    };
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                found.problems.record(directory, error);
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                found.problems.record(&entry.path(), error);
                continue;
            }
        };
        // Symlinks are not followed: a link out of the workspace is not searched.
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if is_noise(&entry.file_name()) {
                continue;
            }
            directories.push(entry.path());
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
    files.sort();
    directories.sort();
    found.files.append(&mut files);
    for directory in directories {
        descend(&directory, depth + 1, found);
    }
}

/// Build output, dependencies and caches: never searched, never listed.
fn is_noise(name: &OsStr) -> bool {
    NOISE_DIRS.iter().any(|noise| name == OsStr::new(noise))
}

/// Files the walk could not read, kept short so the note stays readable.
#[derive(Default)]
struct Problems {
    count: usize,
    examples: Vec<String>,
}

impl Problems {
    fn record(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.count += 1;
        if self.examples.len() < 3 {
            self.examples.push(format!("{}: {}", path.display(), error));
        }
    }

    fn note(&self) -> Option<String> {
        if self.count == 0 {
            return None;
        }
        Some(format!(
            "Search incomplete: {} read error{}; representative paths: {}.",
            self.count,
            if self.count == 1 { "" } else { "s" },
            self.examples.join("; ")
        ))
    }
}

/// Files `grep` did not open because they were over [`MAX_FILE_SIZE`]; the note
/// points at bash, which can search part of one.
#[derive(Default)]
struct Skipped {
    count: usize,
    examples: Vec<String>,
}

impl Skipped {
    fn record(&mut self, workspace: &Workspace, path: &Path) {
        self.count += 1;
        if self.examples.len() < 3 {
            self.examples.push(workspace.show(path));
        }
    }

    fn note(&self) -> Option<String> {
        if self.count == 0 {
            return None;
        }
        Some(format!(
            "Skipped {} file{} over {} MB: {}{} — search them with bash (`rg`/`grep`).",
            self.count,
            if self.count == 1 { "" } else { "s" },
            MAX_FILE_SIZE / (1024 * 1024),
            self.examples.join(", "),
            if self.count > self.examples.len() {
                " …"
            } else {
                ""
            }
        ))
    }
}

// ---------------------------------------------------------------------------
// grep
// ---------------------------------------------------------------------------

fn grep_tree(workspace: &Workspace, root: &Path, pattern: &str) -> Result<String, String> {
    let program = compile(pattern, false)?;
    let mut steps = MAX_STEPS;
    let found = walk(root);
    let mut problems = found.problems;
    let mut out = String::new();
    let mut matches = 0usize;
    let mut skipped = Skipped::default();
    let mut notes: Vec<String> = Vec::new();
    let mut stop: Option<String> = None;
    'files: for path in &found.files {
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                problems.record(path, error);
                continue;
            }
        };
        if metadata.len() > MAX_FILE_SIZE {
            skipped.record(workspace, path);
            continue;
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) => {
                problems.record(path, error);
                continue;
            }
        };
        let mut reader = BufReader::new(file);
        let head = match reader.fill_buf() {
            Ok(head) => head,
            Err(error) => {
                problems.record(path, error);
                continue;
            }
        };
        if is_binary(head) {
            continue;
        }
        let shown = workspace.show(path);
        let mut buffer = Vec::new();
        let mut number = 0usize;
        loop {
            let line = match next_line(&mut reader, &mut buffer) {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    problems.record(path, error);
                    break;
                }
            };
            number += 1;
            if !search(&program, &line, &mut steps) {
                if steps == 0 {
                    stop = Some(
                        "the pattern was too slow to run on this tree; simplify it or narrow the path"
                            .to_string(),
                    );
                    break 'files;
                }
                continue;
            }
            if matches == MAX_MATCHES {
                stop = Some(format!(
                    "{} matches limit reached; narrow the pattern or the path.",
                    MAX_MATCHES
                ));
                break 'files;
            }
            matches += 1;
            out.push_str(&format!(
                "{}:{}: {}\n",
                shown,
                number,
                cut(&line, MAX_MATCH_LINE)
            ));
        }
    }
    if found.depth_limited {
        notes.push(format!(
            "Search incomplete: the walk stopped descending at depth {}; narrow the path.",
            MAX_DEPTH
        ));
    }
    if let Some(stop) = stop {
        notes.push(stop);
    }
    if let Some(note) = skipped.note() {
        notes.push(note);
    }
    if let Some(note) = problems.note() {
        notes.push(note);
    }
    if matches == 0 {
        let mut text = "No matches found.".to_string();
        if !notes.is_empty() {
            text.push_str(&format!("\n[{}]", notes.join(" ")));
        }
        return Ok(text);
    }
    out = out.trim_end().to_string();
    if !notes.is_empty() {
        out.push_str(&format!("\n[{}]", notes.join(" ")));
    }
    Ok(out)
}

/// A file whose first 8 KB hold a NUL byte is treated as binary and skipped.
fn is_binary(head: &[u8]) -> bool {
    head[..head.len().min(8000)].contains(&0)
}

/// One line without its newline, decoded lossily the way `str::lines()` would,
/// so a file with a stray invalid byte still yields its other lines.
fn next_line<'a>(
    reader: &mut impl BufRead,
    buffer: &'a mut Vec<u8>,
) -> std::io::Result<Option<Cow<'a, str>>> {
    buffer.clear();
    if reader.read_until(b'\n', buffer)? == 0 {
        return Ok(None);
    }
    if buffer.last() == Some(&b'\n') {
        buffer.pop();
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
    }
    Ok(Some(String::from_utf8_lossy(buffer)))
}

/// The first `limit` characters, cut on a character boundary, with a marker
/// when something was cut.
fn cut(text: &str, limit: usize) -> String {
    let mut out: String = text.chars().take(limit).collect();
    if text.chars().count() > limit {
        out.push('…');
        out.push_str(" [truncated]");
    }
    out
}

// ---------------------------------------------------------------------------
// glob
// ---------------------------------------------------------------------------

/// One component of a glob pattern: whole components at once, or a program
/// matched against one component.
enum Segment {
    AnyDepth,
    Pattern(Vec<Inst>),
}

/// Splits a glob pattern into components and compiles each one. The pattern is
/// relative and may not climb out of the directory being searched.
fn segments(pattern: &str) -> Result<Vec<Segment>, String> {
    if pattern.starts_with('~') || Path::new(pattern).is_absolute() {
        return Err("the pattern must be relative to the searched directory".to_string());
    }
    let mut segments = Vec::new();
    for raw in pattern.split('/') {
        if raw == "." {
            continue;
        }
        if raw.is_empty() {
            return Err("the pattern has an empty component; use `**` for any depth".to_string());
        }
        if raw == ".." {
            return Err("the pattern may not contain `..`".to_string());
        }
        if raw == "**" {
            segments.push(Segment::AnyDepth);
            continue;
        }
        segments.push(Segment::Pattern(compile(raw, true)?));
    }
    Ok(segments)
}

fn glob_tree(workspace: &Workspace, root: &Path, pattern: &str) -> Result<String, String> {
    let segments = segments(pattern)?;
    let found = walk(root);
    let mut steps = MAX_STEPS;
    let mut listed: Vec<String> = Vec::new();
    let mut more = false;
    let mut stop: Option<String> = None;
    for path in &found.files {
        let relative = match path.strip_prefix(root) {
            Ok(relative) => relative,
            Err(_) => continue,
        };
        if !matches_segments(&segments, relative, &mut steps) {
            if steps == 0 {
                stop = Some(
                    "the pattern was too slow to run on this tree; simplify it or narrow the path"
                        .to_string(),
                );
                break;
            }
            continue;
        }
        if listed.len() == MAX_FILES {
            more = true;
            break;
        }
        listed.push(workspace.show(path));
    }
    let mut notes: Vec<String> = Vec::new();
    if found.depth_limited {
        notes.push(format!(
            "Search incomplete: the walk stopped descending at depth {}; narrow the path.",
            MAX_DEPTH
        ));
    }
    if let Some(stop) = stop {
        notes.push(stop);
    }
    if let Some(note) = found.problems.note() {
        notes.push(note);
    }
    if listed.is_empty() {
        let shown = workspace.show(root);
        let mut text = if shown == "." {
            format!("No files match `{}` in the workspace.", pattern)
        } else {
            format!("No files match `{}` under {}.", pattern, shown)
        };
        if !notes.is_empty() {
            text.push_str(&format!("\n[{}]", notes.join(" ")));
        }
        return Ok(text);
    }
    let mut out = listed.join("\n");
    if more {
        out.push_str(&format!(
            "\n[{} results limit reached; narrow the pattern or the path.]",
            MAX_FILES
        ));
    }
    if !notes.is_empty() {
        out.push_str(&format!("\n[{}]", notes.join(" ")));
    }
    Ok(out)
}

fn matches_segments(segments: &[Segment], relative: &Path, steps: &mut usize) -> bool {
    let parts: Vec<&OsStr> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part),
            _ => None,
        })
        .collect();
    match_parts(segments, &parts, steps)
}

fn match_parts(segments: &[Segment], parts: &[&OsStr], steps: &mut usize) -> bool {
    // Every node of the recursion spends budget, not only the leaf that runs a
    // program: a pattern with many `**` components walks a tree of
    // combinations, and only the matcher's own steps used to bound it.
    if *steps == 0 {
        return false;
    }
    *steps -= 1;
    match segments.split_first() {
        None => parts.is_empty(),
        Some((Segment::AnyDepth, rest)) => {
            if rest.is_empty() {
                // A trailing `**` is "everything below here", not "here".
                return !parts.is_empty();
            }
            (0..=parts.len()).any(|skip| match_parts(rest, &parts[skip..], steps))
        }
        Some((Segment::Pattern(program), rest)) => {
            if parts.is_empty() {
                return false;
            }
            let component = parts[0].to_string_lossy();
            full_match(program, &component, steps) && match_parts(rest, &parts[1..], steps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolContext, ToolPolicy};
    use serde_json::json;

    /// A scratch workspace under the temporary directory, removed when the test
    /// that owns it ends.
    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("the-pair-{}-{}", name, uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("scratch directory");
            Self { path }
        }

        fn file(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.path.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory");
            }
            fs::write(&path, contents).expect("scratch file");
            path
        }

        fn ctx(&self) -> ToolContext<'_> {
            ToolContext {
                directory: &self.path,
                policy: ToolPolicy::Full,
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn call(arguments: Value) -> Map<String, Value> {
        arguments.as_object().expect("a JSON object").clone()
    }

    async fn grep_one(scratch: &Scratch, pattern: &str) -> String {
        grep(&scratch.ctx(), &call(json!({"pattern": pattern})))
            .await
            .expect("grep")
    }

    async fn glob_one(scratch: &Scratch, pattern: &str) -> String {
        glob(&scratch.ctx(), &call(json!({"pattern": pattern})))
            .await
            .expect("glob")
    }

    #[tokio::test]
    async fn grep_reports_matches_with_paths_and_line_numbers() {
        let scratch = Scratch::new("grep-hit");
        scratch.file("src/a.rs", "fn alpha() {}\nfn beta() {}\n");
        scratch.file("notes.txt", "alpha notes\nnothing here\n");
        assert_eq!(
            grep_one(&scratch, "alpha").await,
            "notes.txt:1: alpha notes\nsrc/a.rs:1: fn alpha() {}"
        );
    }

    #[tokio::test]
    async fn grep_says_so_when_nothing_matches() {
        let scratch = Scratch::new("grep-miss");
        scratch.file("a.txt", "hello\nworld\n");
        assert_eq!(grep_one(&scratch, "needle").await, "No matches found.");
    }

    #[tokio::test]
    async fn grep_reads_carriage_return_line_endings() {
        let scratch = Scratch::new("grep-crlf");
        scratch.file("a.txt", "alpha one\r\nbeta\r\n");
        assert_eq!(grep_one(&scratch, "beta").await, "a.txt:2: beta");
    }

    #[tokio::test]
    async fn grep_keeps_its_output_bounded() {
        let scratch = Scratch::new("grep-bounded");
        let line = format!("needle {}\n", "y".repeat(5000));
        scratch.file("a.txt", &line.repeat(200));
        let out = grep_one(&scratch, "needle").await;
        assert_eq!(out.lines().count(), MAX_MATCHES + 1);
        assert!(out.lines().next().unwrap().len() <= MAX_MATCH_LINE + 32);
        assert!(
            out.ends_with("[100 matches limit reached; narrow the pattern or the path.]"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn a_search_path_that_is_not_there_is_an_error() {
        let scratch = Scratch::new("grep-missing");
        scratch.file("a.txt", "alpha\n");
        let error = grep(
            &scratch.ctx(),
            &call(json!({"pattern": "alpha", "path": "nope"})),
        )
        .await
        .unwrap_err();
        assert!(error.contains("does not exist"), "{}", error);
        let error = glob(
            &scratch.ctx(),
            &call(json!({"pattern": "*", "path": "nope"})),
        )
        .await
        .unwrap_err();
        assert!(error.contains("does not exist"), "{}", error);
    }

    #[test]
    fn a_pattern_it_cannot_read_is_an_error() {
        for pattern in ["a\\", "[z-a]", "a{99999999999999999999}", "a)", "(a", "[]"] {
            let error = match compile(pattern, false) {
                Ok(_) => panic!("{} compiled", pattern),
                Err(error) => error,
            };
            assert!(error.contains("pattern"), "{} -> {}", pattern, error);
        }
    }

    #[test]
    fn a_repetition_count_too_large_to_compile_is_refused() {
        // `a{100000000}` parses, so nothing rejects it until the compiler has
        // already emitted 100M instructions — 4 GB — and `a{4294967295}` would
        // ask for ~137 GB and abort the app. The count is refused instead.
        for pattern in [
            "a{100000000}",
            "a{4294967295}",
            "a{0,4294967295}",
            "a{1000000,}",
        ] {
            let started = std::time::Instant::now();
            let error = match compile(pattern, false) {
                Ok(_) => panic!("{} compiled", pattern),
                Err(error) => error,
            };
            assert!(error.contains("pattern"), "{} -> {}", pattern, error);
            assert!(
                error.contains(&MAX_REPEAT.to_string()),
                "{} -> {}",
                pattern,
                error
            );
            assert!(
                started.elapsed() < std::time::Duration::from_secs(1),
                "{} was compiled rather than refused",
                pattern
            );
        }
        // A count within the cap still compiles, and still matches.
        assert!(compile(&format!("a{{{}}}", MAX_REPEAT), false).is_ok());
        assert!(compile(&format!("a{{{},}}", MAX_REPEAT + 1), false).is_err());
    }

    #[test]
    fn a_glob_pattern_is_split_into_components() {
        let parts = segments("src/**/*.rs").expect("pattern");
        assert_eq!(parts.len(), 3);
        assert!(matches!(parts[1], Segment::AnyDepth));
        assert!(matches!(parts[0], Segment::Pattern(_)));
        for pattern in ["", "a//b", "..", "/etc", "~/x", "a/../b"] {
            assert!(segments(pattern).is_err(), "{} was allowed", pattern);
        }
    }

    #[test]
    fn a_workspace_that_cannot_be_read_is_an_error() {
        let missing = std::env::temp_dir().join(format!("the-pair-gone-{}", uuid::Uuid::new_v4()));
        let error = match Workspace::new(&missing) {
            Ok(_) => panic!("a missing workspace resolved"),
            Err(error) => error,
        };
        assert!(error.contains("cannot be read"), "{}", error);
    }

    #[tokio::test]
    async fn grep_needs_a_pattern() {
        let scratch = Scratch::new("grep-nopattern");
        scratch.file("a.txt", "alpha\n");
        let error = grep(&scratch.ctx(), &call(json!({"path": "."})))
            .await
            .unwrap_err();
        assert!(error.contains("needs `pattern`"), "{}", error);
    }

    #[tokio::test]
    async fn grep_refuses_paths_that_leave_the_workspace() {
        let scratch = Scratch::new("grep-escape");
        scratch.file("a.txt", "alpha\n");
        for asked in ["../outside", "/etc", "src/../../outside", "~/secrets"] {
            let error = grep(
                &scratch.ctx(),
                &call(json!({"pattern": "alpha", "path": asked})),
            )
            .await
            .unwrap_err();
            assert!(error.contains("workspace"), "{} -> {}", asked, error);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn grep_refuses_a_symlink_that_points_out_of_the_workspace() {
        let scratch = Scratch::new("grep-link");
        let outside = Scratch::new("grep-link-out");
        outside.file("secret.txt", "alpha\n");
        scratch.file("a.txt", "alpha\n");
        std::os::unix::fs::symlink(&outside.path, scratch.path.join("out")).expect("symlink");
        let error = grep(
            &scratch.ctx(),
            &call(json!({"pattern": "alpha", "path": "out"})),
        )
        .await
        .unwrap_err();
        assert!(error.contains("leaves the workspace"), "{}", error);
        assert!(!grep_one(&scratch, "alpha").await.contains("secret.txt"));
    }

    #[tokio::test]
    async fn grep_stops_at_the_match_limit_and_says_so() {
        let scratch = Scratch::new("grep-limit");
        let body: String = (0..150).map(|n| format!("needle {}\n", n)).collect();
        scratch.file("big.txt", &body);
        let out = grep_one(&scratch, "needle").await;
        assert_eq!(
            out.lines()
                .filter(|line| line.starts_with("big.txt:"))
                .count(),
            MAX_MATCHES
        );
        assert!(
            out.contains("[100 matches limit reached; narrow the pattern or the path.]"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn grep_skips_noise_directories_and_binary_files() {
        let scratch = Scratch::new("grep-noise");
        scratch.file("src/a.rs", "alpha\n");
        scratch.file(".git/config", "alpha\n");
        scratch.file("node_modules/pkg/index.js", "alpha\n");
        scratch.file("target/debug/output", "alpha\n");
        scratch.file("blob.bin", "alpha\0hidden\n");
        assert_eq!(grep_one(&scratch, "alpha").await, "src/a.rs:1: alpha");
    }

    #[tokio::test]
    async fn other_pairs_worktrees_are_not_searched() {
        // An in-place pair shares its repository with two worktree pairs.
        let scratch = Scratch::new("worktrees");
        scratch.file("src/a.rs", "alpha\n");
        scratch.file(".worktrees/pair-1/src/a.rs", "alpha\n");
        scratch.file(".worktrees/pair-2/src/a.rs", "alpha stale\n");
        assert_eq!(grep_one(&scratch, "alpha").await, "src/a.rs:1: alpha");
        assert_eq!(glob_one(&scratch, "**/*.rs").await, "src/a.rs");

        // A worktree pair's own checkout is the root it searches from, so being
        // under `.worktrees` does not hide its files from itself.
        let inside = scratch.path.join(".worktrees/pair-2");
        let ctx = ToolContext {
            directory: &inside,
            policy: ToolPolicy::Full,
        };
        let out = grep(&ctx, &call(json!({"pattern": "alpha"})))
            .await
            .expect("grep");
        assert_eq!(out, "src/a.rs:1: alpha stale");
    }

    #[tokio::test]
    async fn grep_searches_one_file_when_path_points_at_it() {
        let scratch = Scratch::new("grep-onefile");
        scratch.file("a.txt", "one\ntwo alpha\n");
        scratch.file("b.txt", "alpha\n");
        let out = grep(
            &scratch.ctx(),
            &call(json!({"pattern": "alpha", "path": "b.txt"})),
        )
        .await
        .expect("grep");
        assert_eq!(out, "b.txt:1: alpha");
    }

    #[tokio::test]
    async fn grep_understands_the_regex_subset() {
        let scratch = Scratch::new("grep-regex");
        scratch.file("a.rs", "fn alpha()\nlet x = 42;\n// TODO: fix\n");
        assert_eq!(
            grep_one(&scratch, r"^fn \w+\(\)").await,
            "a.rs:1: fn alpha()"
        );
        assert_eq!(
            grep_one(&scratch, r"let x = \d+;").await,
            "a.rs:2: let x = 42;"
        );
        assert_eq!(
            grep_one(&scratch, "TODO|FIXME").await,
            "a.rs:3: // TODO: fix"
        );
        assert_eq!(grep_one(&scratch, r"^let").await, "a.rs:2: let x = 42;");
        assert_eq!(
            grep_one(&scratch, r"^fn \w+\(\)$").await,
            "a.rs:1: fn alpha()"
        );
        assert_eq!(grep_one(&scratch, "alpha").await, "a.rs:1: fn alpha()");
        assert_eq!(grep_one(&scratch, r"^$").await, "No matches found.");
    }

    #[tokio::test]
    async fn grep_explains_a_pattern_it_cannot_read() {
        let scratch = Scratch::new("grep-badpattern");
        scratch.file("a.txt", "alpha\n");
        for pattern in ["alpha(", "[abc", "*alpha", r"\bword\b", "a{2,1}"] {
            let error = grep(&scratch.ctx(), &call(json!({"pattern": pattern})))
                .await
                .unwrap_err();
            assert!(error.contains("pattern"), "{} -> {}", pattern, error);
        }
    }

    #[tokio::test]
    async fn grep_cuts_a_very_long_line() {
        let scratch = Scratch::new("grep-longline");
        scratch.file("a.txt", &format!("needle {}\n", "x".repeat(900)));
        let out = grep_one(&scratch, "needle").await;
        assert!(out.starts_with("a.txt:1: needle xxx"), "{}", out);
        assert!(out.contains("… [truncated]"), "{}", out);
        assert!(out.len() < MAX_MATCH_LINE + 64, "{}", out.len());
    }

    #[tokio::test]
    async fn grep_skips_files_that_are_too_large_and_says_so() {
        let scratch = Scratch::new("grep-large");
        scratch.file("a.txt", "needle\n");
        let big = scratch.file("big.log", "");
        fs::OpenOptions::new()
            .write(true)
            .open(&big)
            .expect("big file")
            .set_len(MAX_FILE_SIZE + 1)
            .expect("size");
        let out = grep_one(&scratch, "needle").await;
        assert_eq!(
            out,
            "a.txt:1: needle\n[Skipped 1 file over 10 MB: big.log — search them with bash (`rg`/`grep`).]"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn grep_reports_directories_it_could_not_read() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new("grep-denied");
        scratch.file("locked/secret.txt", "alpha\n");
        scratch.file("open.txt", "alpha\n");
        let locked = scratch.path.join("locked");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock");
        let out = grep_one(&scratch, "alpha").await;
        // Root ignores the mode, so ask the filesystem rather than assuming.
        let enforced = fs::read_dir(&locked).is_err();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlock");
        if !enforced {
            // Running as root, where the mode is not enforced: nothing to report.
            assert!(!out.contains("Search incomplete"), "{}", out);
        } else {
            assert!(out.contains("open.txt:1: alpha"), "{}", out);
            assert!(out.contains("Search incomplete: 1 read error"), "{}", out);
            assert!(!out.contains("secret.txt"), "{}", out);
        }
    }

    #[tokio::test]
    async fn glob_matches_double_star_patterns() {
        let scratch = Scratch::new("glob-star");
        scratch.file("src/a.rs", "");
        scratch.file("src/deep/b.rs", "");
        scratch.file("src/keep.md", "");
        scratch.file("notes.txt", "");
        assert_eq!(
            glob_one(&scratch, "**/*.rs").await,
            "src/a.rs\nsrc/deep/b.rs"
        );
        assert_eq!(
            glob_one(&scratch, "src/**").await,
            "src/a.rs\nsrc/keep.md\nsrc/deep/b.rs"
        );
        assert_eq!(glob_one(&scratch, "src/*.rs").await, "src/a.rs");
        assert_eq!(glob_one(&scratch, "**/*.md").await, "src/keep.md");
        assert_eq!(
            glob_one(&scratch, "*.rs").await,
            "No files match `*.rs` in the workspace."
        );
    }

    #[tokio::test]
    async fn glob_matches_question_marks_and_character_classes() {
        let scratch = Scratch::new("glob-class");
        scratch.file("a1.txt", "");
        scratch.file("a22.txt", "");
        scratch.file("b1.txt", "");
        assert_eq!(glob_one(&scratch, "a?.txt").await, "a1.txt");
        assert_eq!(glob_one(&scratch, "[ab]1.txt").await, "a1.txt\nb1.txt");
        assert_eq!(glob_one(&scratch, "a[0-9]*.txt").await, "a1.txt\na22.txt");
        assert_eq!(glob_one(&scratch, "[!a]*.txt").await, "b1.txt");
    }

    #[tokio::test]
    async fn glob_needs_a_pattern() {
        let scratch = Scratch::new("glob-nopattern");
        scratch.file("a.txt", "");
        let error = glob(&scratch.ctx(), &call(json!({"path": "."})))
            .await
            .unwrap_err();
        assert!(error.contains("needs `pattern`"), "{}", error);
    }

    #[tokio::test]
    async fn glob_refuses_patterns_and_paths_that_leave_the_workspace() {
        let scratch = Scratch::new("glob-escape");
        scratch.file("src/a.rs", "");
        for pattern in ["/etc/*", "../*", "src/../../x", "~/x"] {
            let error = glob(&scratch.ctx(), &call(json!({"pattern": pattern})))
                .await
                .unwrap_err();
            assert!(
                error.contains("pattern") || error.contains("relative"),
                "{}",
                pattern
            );
        }
        for asked in ["../outside", "/etc"] {
            let error = glob(
                &scratch.ctx(),
                &call(json!({"pattern": "*", "path": asked})),
            )
            .await
            .unwrap_err();
            assert!(error.contains("workspace"), "{} -> {}", asked, error);
        }
    }

    #[tokio::test]
    async fn glob_skips_noise_directories() {
        let scratch = Scratch::new("glob-noise");
        scratch.file("src/a.rs", "");
        scratch.file(".git/HEAD", "");
        scratch.file("node_modules/pkg/index.js", "");
        assert_eq!(glob_one(&scratch, "**/*").await, "src/a.rs");
    }

    #[tokio::test]
    async fn glob_stops_at_the_results_limit_and_says_so() {
        let scratch = Scratch::new("glob-limit");
        for n in 0..(MAX_FILES + 1) {
            scratch.file(&format!("f{:04}.txt", n), "");
        }
        let out = glob_one(&scratch, "*.txt").await;
        assert_eq!(out.lines().count(), MAX_FILES + 1);
        assert!(
            out.contains("[1000 results limit reached; narrow the pattern or the path.]"),
            "{}",
            out
        );
    }

    #[tokio::test]
    async fn the_walk_stops_at_maximum_depth() {
        let scratch = Scratch::new("depth");
        let mut deep = String::new();
        for level in 0..(MAX_DEPTH + 8) {
            deep.push_str(&format!("d{}/", level));
        }
        scratch.file(&format!("{}leaf.txt", deep), "needle\n");
        scratch.file("shallow.txt", "needle\n");
        let out = grep_one(&scratch, "needle").await;
        assert_eq!(
            out,
            "shallow.txt:1: needle\n[Search incomplete: the walk stopped descending at depth 32; narrow the path.]"
        );
        let out = glob_one(&scratch, "**/leaf.txt").await;
        assert!(
            out.starts_with("No files match `**/leaf.txt` in the workspace."),
            "{}",
            out
        );
        assert!(out.contains("stopped descending at depth 32"), "{}", out);
    }

    #[tokio::test]
    async fn a_glob_whose_star_components_explode_stops_instead_of_hanging() {
        let scratch = Scratch::new("glob-explode");
        let mut deep = String::new();
        for level in 0..(MAX_DEPTH - 1) {
            deep.push_str(&format!("d{}/", level));
        }
        scratch.file(&format!("{}leaf.txt", deep), "");
        // Every `**` multiplies the ways the pattern can be laid over the path,
        // so the recursion is exponential in the number of components: ten of
        // them over a 32-component path is ~1e10 nodes. Only the budget stops
        // it, because the matcher's own steps are spent at the leaves alone.
        let pattern = format!("{}missing.txt", "**/".repeat(10));
        let started = std::time::Instant::now();
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            glob_one(&scratch, &pattern),
        )
        .await
        .expect("the pattern walk never came back");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the pattern took {:.1}s",
            started.elapsed().as_secs_f64()
        );
        assert!(out.contains("too slow to run"), "{}", out);
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused_before_anything_is_read() {
        let scratch = Scratch::new("resolve");
        let workspace = Workspace::new(&scratch.path).expect("workspace");
        assert_eq!(workspace.resolve(".").expect("root"), workspace.root);
        assert_eq!(
            workspace.resolve("src/a.rs").expect("inside"),
            workspace.root.join("src/a.rs")
        );
        assert_eq!(
            workspace.resolve("./a.txt").expect("inside"),
            workspace.root.join("a.txt")
        );
        for asked in ["..", "src/../outside", "/etc/passwd", "~/x", "a/../../b"] {
            assert!(workspace.resolve(asked).is_err(), "{} was allowed", asked);
        }
        assert_eq!(workspace.show(&workspace.root.join("src/a.rs")), "src/a.rs");
        assert_eq!(workspace.show(&workspace.root), ".");
    }

    #[test]
    fn the_pattern_subset_matches_what_it_should() {
        let matches = |pattern: &str, text: &str| {
            let program = compile(pattern, false).expect(pattern);
            let mut steps = MAX_STEPS;
            search(&program, text, &mut steps)
        };
        assert!(matches("alpha", "fn alpha() {}"));
        assert!(!matches("beta", "fn alpha() {}"));
        assert!(matches(r"^\w+ \w+$", "fn alpha"));
        assert!(!matches(r"^\w+ \w+$", "fn alpha() {}"));
        assert!(matches(r"a{2,3}", "aaa"));
        assert!(!matches(r"a{4}", "aaa"));
        assert!(matches(r"colou?r", "color"));
        assert!(matches(r"colou?r", "colour"));
        assert!(matches(r"(cat|dog)s", "two dogs"));
        assert!(!matches(r"(cat|dog)s", "two birds"));
        assert!(matches(r"[^0-9]+", "abc"));
        assert!(!matches(r"^[0-9]+$", "abc"));
        assert!(matches(r"\.", "a.b"));
        assert!(!matches(r"^\.$", "ab"));
        assert!(matches(r"^\s*$", "   "));
        assert!(!matches(r"^\s*$", " x "));
        // A pattern that can match nothing still matches at the end of a line.
        assert!(matches(r"\d*$", "abc"));
        assert!(matches(r"a*$", "bca"));
        assert!(matches(r"x*", "abc"));
        // A component glob is a full match, not a search: `*` covers everything.
        let program = compile("*.rs", true).expect("pattern");
        let mut steps = MAX_STEPS;
        assert!(full_match(&program, "a.rs", &mut steps));
        assert!(!full_match(&program, "a.rs.bak", &mut steps));
        assert!(!full_match(&program, "src/a.rs", &mut steps));
    }

    #[test]
    fn a_pattern_that_loops_without_consuming_stops_instead_of_hanging() {
        let program = compile(r"(a?)*b", false).expect("pattern");
        let mut steps = MAX_STEPS;
        assert!(!search(
            &program,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaac",
            &mut steps
        ));
        assert_eq!(steps, 0);
    }
}
