// Permission rules (plan §2 "Permissions", ported from 2market's D5): Claude Code's form, `Shell(git status:*)`,
// `Shell(ffmpeg -version)`, `Edit(views/**)`, matched against a tool call.
// Shell matching reads command text, so it is best-effort: a
// command it can't read as plain words never matches an allow rule.

use std::path::{Component, Path, PathBuf};

/// A rule's tool: `Shell`, or a file tool by its glob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pattern {
    /// `Shell(<prefix>:*)`: the command's words start with `prefix`.
    ShellPrefix(String),
    /// `Shell(<command>)`: exactly this command.
    ShellExact(String),
    /// `Read|Write|Edit(<glob>)`.
    File { tool: FileTool, glob: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileTool {
    Read,
    Write,
    Edit,
}

impl FileTool {
    pub fn of(tool: &str) -> Option<FileTool> {
        match tool {
            "read" => Some(FileTool::Read),
            "write" => Some(FileTool::Write),
            "edit" => Some(FileTool::Edit),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Option<Pattern> {
    let text = text.trim();
    let open = text.find('(')?;
    let inner = text.strip_suffix(')')?[open + 1..].trim();
    if inner.is_empty() {
        return None;
    }
    let tool = match &text[..open] {
        "Shell" | "Bash" => {
            return Some(match inner.strip_suffix(":*") {
                Some(p) if !p.trim().is_empty() => Pattern::ShellPrefix(squash(p)),
                Some(_) => return None,
                None => Pattern::ShellExact(squash(inner)),
            })
        }
        "Read" => FileTool::Read,
        "Write" => FileTool::Write,
        "Edit" => FileTool::Edit,
        _ => return None,
    };
    Some(Pattern::File { tool, glob: inner.to_string() })
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Pattern {
    /// Whether this shell rule names one segment of a command.
    pub fn matches_segment(&self, seg: &Segment) -> bool {
        match self {
            Pattern::ShellPrefix(p) => seg.text == *p || seg.text.strip_prefix(p.as_str()).is_some_and(|rest| rest.starts_with(' ')),
            Pattern::ShellExact(c) => seg.text == *c,
            Pattern::File { .. } => false,
        }
    }

    /// Whether this file rule covers `tool` on `path` (as `target` gives it).
    pub fn matches_file(&self, tool: FileTool, path: &Target) -> bool {
        let Pattern::File { tool: t, glob } = self else { return false };
        // A rule for Edit covers Write too, as Claude Code's does.
        let covers = *t == tool || (*t == FileTool::Edit && tool == FileTool::Write);
        // macOS and Windows folders ignore case: `.BUDDY/x` is `.buddy/x`.
        let fold = |x: &str| if cfg!(any(target_os = "macos", windows)) { x.to_lowercase() } else { x.to_string() };
        let glob = fold(glob);
        covers && (glob_match(&glob, &fold(&path.display)) || glob_match(&glob, &fold(&path.abs.to_string_lossy())))
    }
}

// ------------------------------------------------------------------ shell

/// One simple command of a shell line, with its words unquoted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub words: Vec<String>,
    /// The words joined by single spaces, for rule matching.
    pub text: String,
    /// Writes a file through a redirect (`>`, `>>`), other than to /dev/null.
    pub writes: bool,
}

/// A shell line split into segments at `&&`, `||`, `;`, `|`, `&`, newlines
/// and command substitutions. `complex` marks anything the split can't read
/// as plain words: substitutions, process substitution, unclosed quotes.
#[derive(Debug, Default)]
pub struct Line {
    pub segments: Vec<Segment>,
    pub complex: bool,
}

pub fn split(command: &str) -> Line {
    let mut line = Line::default();
    let mut words: Vec<String> = vec![];
    let mut word = String::new();
    let mut in_word = false;
    let mut writes = false;
    let mut redirect = false;
    let mut quote: Option<char> = None;
    let chars: Vec<char> = command.chars().collect();
    let mut i = 0;
    fn end_word(words: &mut Vec<String>, word: &mut String, in_word: &mut bool, redirect: &mut bool, writes: &mut bool) {
        if *in_word {
            if *redirect {
                if word != "/dev/null" && !word.starts_with('&') {
                    *writes = true;
                }
                *redirect = false;
            } else {
                words.push(std::mem::take(word));
            }
        }
        word.clear();
        *in_word = false;
    }
    let end_seg = |line: &mut Line, words: &mut Vec<String>, writes: &mut bool| {
        if !words.is_empty() || *writes {
            let text = words.join(" ");
            line.segments.push(Segment { words: std::mem::take(words), text, writes: *writes });
        }
        *writes = false;
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    word.push(c);
                }
            }
            Some(_) => match c {
                '"' => quote = None,
                '\\' if next.is_some() => {
                    word.push(next.unwrap());
                    i += 1;
                }
                '`' => line.complex = true,
                '$' if next == Some('(') => line.complex = true,
                _ => word.push(c),
            },
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '\\' if next.is_some() => {
                    if next != Some('\n') {
                        word.push(next.unwrap());
                        in_word = true;
                    }
                    i += 1;
                }
                ' ' | '\t' => end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes),
                '#' if !in_word => {
                    // A comment runs to the end of the line.
                    while i < chars.len() && chars[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                // `2>&1` and `>&2` never get here: the `>` arm takes their `&`.
                ';' | '\n' | '&' | '|' | '(' | ')' | '`' => {
                    if c == '`' || c == '(' {
                        line.complex = true;
                    }
                    end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes);
                    end_seg(&mut line, &mut words, &mut writes);
                    if (c == '&' || c == '|') && next == Some(c) {
                        i += 1;
                    }
                }
                '$' if next == Some('(') => {
                    line.complex = true;
                    end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes);
                    end_seg(&mut line, &mut words, &mut writes);
                    i += 1;
                }
                '<' | '>' if next == Some('(') => {
                    line.complex = true;
                    end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes);
                    end_seg(&mut line, &mut words, &mut writes);
                    i += 1;
                }
                '>' => {
                    // `2>` and `&>` start a redirect too; their digits aren't a word.
                    if in_word && word.chars().all(|ch| ch.is_ascii_digit() || ch == '&') {
                        word.clear();
                        in_word = false;
                    } else {
                        end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes);
                    }
                    if next == Some('>') {
                        i += 1;
                    }
                    if chars.get(i + 1) == Some(&'&') && chars.get(i + 2).is_some_and(|ch| ch.is_ascii_digit() || *ch == '-') {
                        // `>&2`: a descriptor.
                        i += 1;
                        while chars.get(i + 1).is_some_and(|ch| ch.is_ascii_digit() || *ch == '-') {
                            i += 1;
                        }
                    } else {
                        // `>& file` writes the file, as `>` does.
                        if chars.get(i + 1) == Some(&'&') {
                            i += 1;
                        }
                        redirect = true;
                    }
                }
                '<' => {
                    // A heredoc's body isn't commands; the split can't tell where it ends.
                    if next == Some('<') {
                        line.complex = true;
                    }
                    end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes)
                }
                // `$'…'` takes `\'` as a quote inside, which the split doesn't read.
                '$' if next == Some('\'') => {
                    line.complex = true;
                    word.push(c);
                    in_word = true;
                }
                _ => {
                    word.push(c);
                    in_word = true;
                }
            },
        }
        i += 1;
    }
    if quote.is_some() {
        line.complex = true;
    }
    if redirect && !in_word {
        // `cmd >` with nothing after it.
        writes = true;
    }
    end_word(&mut words, &mut word, &mut in_word, &mut redirect, &mut writes);
    end_seg(&mut line, &mut words, &mut writes);
    line
}

/// Commands that only read (the built-in allowlist).
const READ_ONLY: [&str; 25] = [
    "ls", "cat", "head", "tail", "wc", "pwd", "echo", "printf", "rg", "grep", "which", "file", "stat", "du", "df", "date", "uniq", "cut", "tr", "diff", "tree", "jq", "ffprobe", "basename", "dirname",
];
const GIT_READ: [&str; 6] = ["status", "diff", "log", "show", "rev-parse", "ls-files"];
const FIND_WRITES: [&str; 9] = ["-exec", "-execdir", "-delete", "-ok", "-okdir", "-fls", "-fprint", "-fprint0", "-fprintf"];

/// Whether a segment runs a built-in read-only command.
pub fn read_only(seg: &Segment) -> bool {
    if seg.writes {
        return false;
    }
    let w: Vec<&str> = seg.words.iter().map(String::as_str).collect();
    let Some(&cmd) = w.first() else { return true };
    let args = &w[1..];
    match cmd {
        "find" => !args.iter().any(|a| FIND_WRITES.contains(a)),
        "sort" => !args.iter().any(|a| *a == "-o" || a.starts_with("--output") || a.starts_with("--compress-program") || (a.starts_with('-') && !a.starts_with("--") && a.contains('o'))),
        // `--pre` and `--hostname-bin` run a program; `tree -o` and `uniq IN OUT` write a file.
        "rg" => !args.iter().any(|a| ["--pre", "--hostname-bin"].iter().any(|f| a == f || a.strip_prefix(f).is_some_and(|r| r.starts_with('=')))),
        "tree" => !args.iter().any(|a| *a == "-o" || a.starts_with("--output")),
        "uniq" => args.iter().filter(|a| !a.starts_with('-')).count() <= 1,
        "realpath" => true,
        "git" => match args.first() {
            Some(sub) if GIT_READ.contains(sub) => !args.iter().any(|a| a.starts_with("--output")),
            Some(&"branch") => args[1..].iter().all(|a| ["-a", "-r", "-v", "-vv", "--list", "--all", "--remotes", "--show-current"].contains(a)),
            _ => false,
        },
        // The `buddy` commands that only read: help and status.
        "buddy" => matches!(args, [] | ["--help"] | ["--help", "--json"] | ["help"] | ["status"] | ["status", "--json"]),
        c => READ_ONLY.contains(&c),
    }
}

/// Words that run the command after them.
const WRAPPERS: [&str; 9] = ["env", "nohup", "time", "command", "exec", "nice", "xargs", "builtin", "caffeinate"];
const PUBLISHERS: [&str; 7] = ["npm", "pnpm", "yarn", "bun", "cargo", "twine", "gem"];

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Full access (`full` mode) still asks before this segment: why, in the card's words. The list is the
/// commands that can't be taken back or reach outside the person's files: administrator rights, deleting
/// folders, erasing disks, rewriting git history, system settings, the keychain, other apps, publishing.
pub fn attention(seg: &Segment) -> Option<&'static str> {
    if seg.words.iter().any(|w| sensitive_path(w.split_once('=').map_or(w.as_str(), |(_, v)| v))) {
        return Some("it touches keys, passwords or startup files");
    }
    // the command word: after `VAR=x`, wrappers and their flags
    let mut w = seg.words.iter().map(String::as_str).skip_while(|w| w.contains('=') || w.starts_with('-') || WRAPPERS.contains(&base(w)));
    let cmd = base(w.next()?);
    let args: Vec<&str> = w.collect();
    let flag = |short: char, long: &str| args.iter().any(|a| *a == long || (a.starts_with('-') && !a.starts_with("--") && a.contains(short)));
    let sub = args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("");
    match cmd {
        "sudo" | "su" | "doas" => Some("it runs as administrator"),
        "rm" if flag('r', "--recursive") || flag('R', "--recursive") => Some("it deletes whole folders"),
        "find" if args.contains(&"-delete") => Some("it deletes files"),
        "dd" | "diskutil" | "fdisk" => Some("it can erase a disk"),
        c if c.starts_with("mkfs") || c.starts_with("newfs") => Some("it can erase a disk"),
        "shutdown" | "reboot" | "halt" => Some("it turns the computer off"),
        "launchctl" | "csrutil" | "spctl" | "nvram" | "systemsetup" | "dscl" | "crontab" => Some("it changes system settings"),
        "security" => Some("it reads or changes the keychain"),
        "osascript" => Some("it controls other apps"),
        "killall" | "pkill" => Some("it stops other apps"),
        "chmod" | "chown" if flag('R', "--recursive") => Some("it changes permissions on a whole folder"),
        "git" => match sub {
            "push" if flag('f', "--force") || args.iter().any(|a| a.starts_with("--force") || *a == "--delete" || *a == "--mirror" || a.starts_with('+')) => Some("it rewrites or deletes git history on the server"),
            "reset" if args.contains(&"--hard") => Some("it throws away uncommitted work"),
            "clean" => Some("it deletes untracked files"),
            _ => None,
        },
        c if PUBLISHERS.contains(&c) && (sub == "publish" || (c == "gem" && sub == "push") || (c == "twine" && sub == "upload")) => Some("it publishes a package"),
        _ => None,
    }
}

/// `attention` for a line the split can't read as plain words (`$(…)`, unclosed quotes): any risky command
/// name anywhere in it.
pub fn attention_raw(command: &str) -> Option<&'static str> {
    const NAMES: [&str; 22] = ["sudo", "su", "doas", "rm", "dd", "diskutil", "fdisk", "shutdown", "reboot", "halt", "launchctl", "csrutil", "spctl", "nvram", "systemsetup", "dscl", "crontab", "security", "osascript", "killall", "pkill", "eval"];
    let risky = command.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')).any(|w| NAMES.contains(&w) || w.starts_with("mkfs") || w.starts_with("newfs"));
    (risky || sensitive_path(command)).then_some("it runs a risky command inside another, which can’t be checked")
}

/// A path that holds keys, passwords or scripts the system runs by itself.
pub fn sensitive_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    const DIRS: [&str; 9] = [".ssh", ".aws", ".gnupg", ".kube", ".docker", "launchagents", "launchdaemons", "keychains", ".config/gh"];
    const FILES: [&str; 9] = [".netrc", ".npmrc", ".pypirc", ".zshrc", ".zshenv", ".zprofile", ".bashrc", ".bash_profile", ".profile"];
    let parts: Vec<&str> = p.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    DIRS.iter().any(|d| format!("/{p}/").contains(&format!("/{d}/")))
        || FILES.contains(&name)
        || name == ".env"
        || (name.starts_with(".env.") && !name.ends_with(".example") && !name.ends_with(".sample"))
        || ["/etc/", "/private/etc/", "/system/", "/library/"].iter().any(|r| p.starts_with(r))
}

/// Tools whose first argument is a subcommand, so a rule names both words.
const TWO_WORD: [&str; 14] = ["git", "npm", "pnpm", "yarn", "bun", "cargo", "docker", "gh", "gcloud", "buddy", "brew", "pip", "uv", "npx"];

/// The rule "Allow … from now on" would add for a segment.
pub fn suggest(seg: &Segment) -> Option<String> {
    let first = seg.words.first()?;
    // `VAR=x cmd` and paths as commands don't make a reusable rule.
    if first.contains('=') || first.contains('/') || first.is_empty() {
        return None;
    }
    let prefix = match seg.words.get(1) {
        Some(sub) if TWO_WORD.contains(&first.as_str()) && !sub.starts_with('-') => format!("{first} {sub}"),
        _ => first.clone(),
    };
    Some(format!("Shell({prefix}:*)"))
}

// ------------------------------------------------------------------ files

/// A file a call names, as rules see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub abs: PathBuf,
    /// Relative to the work folder when inside it (`notes/a.md`), else absolute.
    pub display: String,
    pub inside: bool,
}

/// The path pi-durable's file tools open for `path`: Unicode spaces made
/// plain, one leading `@` dropped, then `~`, `~/` and `file://` expanded
/// (its `normalizeToolPath` and `resolvePath`). Rules must judge that path.
fn tool_path(home: Option<&Path>, path: &str) -> PathBuf {
    let plain: String = path.chars().map(|c| if matches!(c, '\u{a0}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}') { ' ' } else { c }).collect();
    let p = plain.strip_prefix('@').unwrap_or(&plain);
    let home = || home.map(Path::to_path_buf).or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)).unwrap_or_default();
    if p == "~" {
        return home();
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| if cfg!(windows) { p.strip_prefix("~\\") } else { None }) {
        return home().join(rest);
    }
    if let Some(rest) = p.strip_prefix("file://") {
        let rest = rest.strip_prefix("localhost").filter(|r| r.starts_with('/')).unwrap_or(rest);
        if rest.starts_with('/') {
            if let Some(decoded) = percent_decode(rest) {
                return PathBuf::from(decoded);
            }
        }
    }
    PathBuf::from(p)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Resolve `path` against the work folder: `..` and symlinks are followed
/// through the nearest existing ancestor, so a path can't escape by name.
pub fn target(work_dir: &Path, home: Option<&Path>, path: &str) -> Target {
    let tool = tool_path(home, path);
    let p = tool.as_path();
    let joined = if p.is_absolute() { p.to_path_buf() } else { work_dir.join(p) };
    let mut lexical = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                lexical.pop();
            }
            Component::CurDir => {}
            c => lexical.push(c),
        }
    }
    // The nearest ancestor that exists, canonicalized, plus the rest.
    let mut base = lexical.clone();
    let mut rest = vec![];
    let abs = loop {
        if let Ok(c) = base.canonicalize() {
            let mut out = c;
            for r in rest.iter().rev() {
                out.push(r);
            }
            break out;
        }
        match (base.file_name().map(|n| n.to_os_string()), base.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                base = parent;
            }
            _ => break lexical.clone(),
        }
    };
    let root = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
    match abs.strip_prefix(&root) {
        Ok(rel) => Target { display: rel.to_string_lossy().replace('\\', "/"), abs, inside: true },
        Err(_) => Target { display: abs.to_string_lossy().into_owned(), abs, inside: false },
    }
}

/// `p` as compared: without case where the file system usually ignores it
/// (macOS, Windows), so `../HOST.JSON` is the same file as `../host.json`.
pub fn folded(p: &Path) -> PathBuf {
    if cfg!(any(target_os = "macos", windows)) {
        PathBuf::from(p.to_string_lossy().to_lowercase())
    } else {
        p.to_path_buf()
    }
}

/// `*` within a segment, `**` across segments, `?` one character.
pub fn glob_match(glob: &str, path: &str) -> bool {
    let g: Vec<&str> = glob.split('/').filter(|s| !s.is_empty()).collect();
    let p: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    fn segs(g: &[&str], p: &[&str]) -> bool {
        match g.first() {
            None => p.is_empty(),
            Some(&"**") => (0..=p.len()).any(|k| segs(&g[1..], &p[k..])),
            Some(s) => !p.is_empty() && seg(s.as_bytes(), p[0].as_bytes()) && segs(&g[1..], &p[1..]),
        }
    }
    fn seg(g: &[u8], s: &[u8]) -> bool {
        match g.first() {
            None => s.is_empty(),
            Some(b'*') => (0..=s.len()).any(|k| seg(&g[1..], &s[k..])),
            Some(b'?') => !s.is_empty() && seg(&g[1..], &s[1..]),
            Some(c) => s.first() == Some(c) && seg(&g[1..], &s[1..]),
        }
    }
    // An absolute glob only matches absolute paths, and the other way round.
    glob.starts_with('/') == path.starts_with('/') && segs(&g, &p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(cmd: &str) -> Vec<String> {
        split(cmd).segments.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn rules_parse_in_claude_codes_form() {
        assert_eq!(parse("Shell(git status:*)"), Some(Pattern::ShellPrefix("git status".into())));
        assert_eq!(parse("Shell(ffmpeg  -version)"), Some(Pattern::ShellExact("ffmpeg -version".into())));
        assert_eq!(parse("Edit(views/**)"), Some(Pattern::File { tool: FileTool::Edit, glob: "views/**".into() }));
        for bad in ["Shell()", "Shell(:*)", "Net(x)", "Shell(x", "Edit()"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_line_splits_into_segments_and_marks_what_it_cannot_read() {
        assert_eq!(texts("git status && ls -la | wc -l; echo 'a && b'"), ["git status", "ls -la", "wc -l", "echo a && b"]);
        let l = split("echo $(curl -s x)");
        assert!(l.complex);
        assert_eq!(l.segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["echo", "curl -s x"]);
        assert!(split("echo `id`").complex);
        assert!(split("echo 'open").complex);
        assert!(!split("ls 2>&1 >/dev/null").segments[0].writes);
        assert!(split("cat a > b.txt").segments[0].writes);
        assert!(split("echo x >> log").segments[0].writes);
        assert_eq!(texts("ffmpeg -i a.mp4 \\\n  out.png"), ["ffmpeg -i a.mp4 out.png"]);
        assert_eq!(texts("ls # && rm -rf /"), ["ls"]);
        // A quoted or escaped `>` is text: the `&` after it still separates.
        assert_eq!(texts("echo 'x>'&curl -s y"), ["echo x>", "curl -s y"]);
        assert_eq!(texts("echo x\\>&rm y"), ["echo x>", "rm y"]);
        assert!(split("echo x >& out").segments[0].writes);
        assert!(split("echo x >&out").segments[0].writes);
        assert!(!split("ls >&2").segments[0].writes);
        assert!(split("ls $'\\'' ; echo hidden #'").complex);
        assert!(split("cat <<EOF\nls '\nEOF\necho hidden #'").complex);
    }

    #[test]
    fn read_only_commands_are_the_built_in_list() {
        let ro = |c: &str| split(c).segments.iter().all(read_only);
        for c in ["ls -la", "cat a.txt", "rg -n x src", "git status", "git log --oneline -5", "find . -name '*.md'", "git branch", "buddy status --json", "buddy --help --json", "sort a.txt", "uniq -c a.txt", "tree -L 2"] {
            assert!(ro(c), "{c}");
        }
        for c in ["rm x", "find . -delete", "git push", "git branch -D x", "sort -o out a", "cat a > b", "ffmpeg -i a b", "env X=1 ls", "buddy say hi", "buddy mascot emote wave", "rg --pre sh x .", "rg --pre=sh x", "rg --hostname-bin=./x.sh --hyperlink-format=default x .", "rg --hostname-bin ./x.sh x", "tree -o out", "uniq a out", "find . -fls out", "sort --compress-program=sh a"] {
            assert!(!ro(c), "{c}");
        }
    }

    #[test]
    fn prefix_rules_match_on_a_word_boundary() {
        let p = parse("Shell(git status:*)").unwrap();
        let seg = |c: &str| split(c).segments.remove(0);
        assert!(p.matches_segment(&seg("git status")));
        assert!(p.matches_segment(&seg("git  status -s")));
        assert!(!p.matches_segment(&seg("git statusx")));
        assert!(!p.matches_segment(&seg("git log")));
        assert_eq!(suggest(&seg("ffmpeg -i a b")).as_deref(), Some("Shell(ffmpeg:*)"));
        assert_eq!(suggest(&seg("git commit -m x")).as_deref(), Some("Shell(git commit:*)"));
        assert_eq!(suggest(&seg("./run.sh")), None);
    }

    #[test]
    fn file_targets_resolve_inside_or_outside_the_work_folder() {
        let dir = std::env::temp_dir().join(format!("buddy-rules-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("views")).unwrap();
        let t = target(&dir, None, "views/new/a.html");
        assert!(t.inside);
        assert_eq!(t.display, "views/new/a.html");
        assert!(!target(&dir, None, "../outside.txt").inside);
        assert!(!target(&dir, None, "/etc/hosts").inside);
        assert_eq!(target(&dir, None, "views/../.buddy/agents/a/policy.json").display, ".buddy/agents/a/policy.json");
        let deny = parse("Write(.buddy/**)").unwrap();
        assert!(deny.matches_file(FileTool::Write, &target(&dir, None, "./.buddy/x")));
        // The file tools drop a leading `@` and expand `~` and `file://`: so does the check.
        assert!(deny.matches_file(FileTool::Write, &target(&dir, None, "@.buddy/agents/a/policy.json")));
        let home = target(&dir, None, "~/.zshrc");
        assert!(!home.inside);
        assert!(home.abs.ends_with(".zshrc"));
        assert!(!target(&dir, None, "@~/x").inside);
        let url = target(&dir, None, &format!("file://{}/views/a%20b.html", dir.display()));
        assert_eq!((url.inside, url.display.as_str()), (true, "views/a b.html"));
        assert!(!target(&dir, None, "file:///etc/hosts").inside);
        assert_eq!(target(&dir, None, "views/a\u{a0}b.html").display, "views/a b.html");
        #[cfg(target_os = "macos")]
        assert!(deny.matches_file(FileTool::Write, &target(&dir, None, ".BUDDY/x")));
        assert!(parse("Edit(views/**)").unwrap().matches_file(FileTool::Write, &target(&dir, None, "views/a.html")));
        assert!(!parse("Edit(views/*)").unwrap().matches_file(FileTool::Edit, &target(&dir, None, "views/a/b.html")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
