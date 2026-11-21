use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const BASELINE: &str = "tests/comment_baseline.txt";
const UPDATE_ENV: &str = "IDX_UPDATE_COMMENT_BASELINE";
const DERIVES_THAT_RENDER_DOCS: [&str; 5] =
    ["Parser", "Args", "Subcommand", "ValueEnum", "JsonSchema"];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Code,
    Doc,
}

#[derive(Default)]
struct Lexed {
    code: Vec<String>,
    comments: BTreeMap<usize, Kind>,
}

fn lex(source: &str) -> Lexed {
    let chars: Vec<char> = source.chars().collect();
    let mut lexed = Lexed::default();
    let mut line = 0;
    let mut code = String::new();
    let mut i = 0;
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let finish_line = |code: &mut String, lexed: &mut Lexed| {
        lexed.code.push(std::mem::take(code));
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            finish_line(&mut code, &mut lexed);
            line += 1;
            i += 1;
        } else if c == '/' && at(i + 1) == '/' {
            let doc = (at(i + 2) == '/' && at(i + 3) != '/') || at(i + 2) == '!';
            lexed
                .comments
                .insert(line, if doc { Kind::Doc } else { Kind::Code });
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let doc = (at(i + 2) == '*' && !matches!(at(i + 3), '*' | '/')) || at(i + 2) == '!';
            let kind = if doc { Kind::Doc } else { Kind::Code };
            let mut depth = 0;
            loop {
                if i >= chars.len() {
                    break;
                }
                lexed.comments.entry(line).or_insert(kind);
                if chars[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else if chars[i] == '\n' {
                    finish_line(&mut code, &mut lexed);
                    line += 1;
                    i += 1;
                } else {
                    i += 1;
                }
            }
        } else if let Some(hashes) = raw_string_start(&chars, i) {
            let close: String = std::iter::once('"')
                .chain(std::iter::repeat_n('#', hashes))
                .collect();
            i += chars[i..].iter().position(|&ch| ch == '"').unwrap_or(0) + 1;
            code.push_str("\"\"");
            while i < chars.len() && !chars[i..].starts_with(&close.chars().collect::<Vec<_>>()) {
                if chars[i] == '\n' {
                    finish_line(&mut code, &mut lexed);
                    line += 1;
                }
                i += 1;
            }
            i += close.chars().count();
        } else if c == '"' {
            code.push_str("\"\"");
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if at(i) == '\n' {
                    finish_line(&mut code, &mut lexed);
                    line += 1;
                }
                i += 1;
            }
            i += 1;
        } else if c == '\'' && (at(i + 1) == '\\' || (at(i + 1) != '\'' && at(i + 2) == '\'')) {
            code.push_str("' '");
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
        } else {
            code.push(c);
            i += 1;
        }
    }
    finish_line(&mut code, &mut lexed);
    lexed
}

fn raw_string_start(chars: &[char], i: usize) -> Option<usize> {
    let starts_token = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
    let after_prefix = match (chars.get(i), chars.get(i + 1)) {
        (Some('r'), _) if starts_token => i + 1,
        (Some('b' | 'c'), Some('r')) if starts_token => i + 2,
        _ => return None,
    };
    let hashes = chars[after_prefix..]
        .iter()
        .take_while(|&&ch| ch == '#')
        .count();
    (chars.get(after_prefix + hashes) == Some(&'"')).then_some(hashes)
}

fn renders_docs(code: &str) -> bool {
    code.contains("derive(")
        && DERIVES_THAT_RENDER_DOCS.iter().any(|name| {
            code.split(|c: char| !c.is_alphanumeric())
                .any(|word| word == *name)
        })
}

fn banned_lines(source: &str) -> usize {
    let lexed = lex(source);
    let mut rendered_doc_lines = std::collections::BTreeSet::new();
    let mut inside_rendering_item = vec![false];
    let mut derive_pending = false;
    for (line, code) in lexed.code.iter().enumerate() {
        if lexed.comments.get(&line) == Some(&Kind::Doc) && *inside_rendering_item.last().unwrap() {
            rendered_doc_lines.insert(line);
        }
        if renders_docs(code) {
            derive_pending = true;
        }
        for ch in code.chars() {
            match ch {
                '{' => {
                    let parent = *inside_rendering_item.last().unwrap();
                    inside_rendering_item.push(derive_pending || parent);
                    derive_pending = false;
                }
                '}' if inside_rendering_item.len() > 1 => {
                    inside_rendering_item.pop();
                }
                ';' => derive_pending = false,
                _ => {}
            }
        }
    }
    let mut line = 0;
    while line < lexed.code.len() {
        if lexed.comments.get(&line) != Some(&Kind::Doc) {
            line += 1;
            continue;
        }
        let start = line;
        while lexed.comments.get(&line) == Some(&Kind::Doc) {
            line += 1;
        }
        let mut item = line;
        while item < lexed.code.len() {
            let code = lexed.code[item].trim();
            if renders_docs(code) {
                rendered_doc_lines.extend(start..line);
                break;
            }
            if !(code.is_empty() || code.starts_with("#[")) || code.contains('{') {
                break;
            }
            item += 1;
        }
    }
    lexed
        .comments
        .keys()
        .filter(|line| !rendered_doc_lines.contains(line))
        .count()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() && !matches!(name.to_str(), Some("fixtures" | "golden" | "target")) {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

fn counts() -> BTreeMap<String, usize> {
    let root = workspace_root();
    let mut files = Vec::new();
    for krate in ["idx-core", "idx-cli", "idx-mcp"] {
        for dir in ["src", "tests"] {
            rust_files(&root.join("crates").join(krate).join(dir), &mut files);
        }
    }
    files
        .into_iter()
        .filter_map(|path| {
            let count = banned_lines(&fs::read_to_string(&path).unwrap());
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (count > 0).then_some((relative, count))
        })
        .collect()
}

fn read_baseline(path: &Path) -> BTreeMap<String, usize> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (count, file) = line.split_once(' ')?;
            Some((file.to_string(), count.parse().ok()?))
        })
        .collect()
}

#[test]
fn no_file_gains_comments_and_the_baseline_only_goes_down() {
    let baseline_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(BASELINE);
    let actual = counts();
    if std::env::var_os(UPDATE_ENV).is_some() {
        let body: String = actual
            .iter()
            .map(|(file, count)| format!("{count} {file}\n"))
            .collect();
        fs::write(&baseline_path, body).unwrap();
        return;
    }
    let baseline = read_baseline(&baseline_path);
    let mut gained = Vec::new();
    let mut stale = Vec::new();
    for file in actual.keys().chain(baseline.keys()) {
        let now = actual.get(file).copied().unwrap_or(0);
        let allowed = baseline.get(file).copied().unwrap_or(0);
        if now > allowed {
            gained.push(format!("{file}: {now} comment lines, {allowed} allowed"));
        } else if now < allowed {
            stale.push(format!(
                "{file}: {now} comment lines, baseline says {allowed}"
            ));
        }
    }
    gained.dedup();
    stale.dedup();
    assert!(
        gained.is_empty(),
        "comments are banned (see .claude/skills/delete-comment/SKILL.md); say it in code \
         instead: a name, a type, a test, an assertion message, or docs/ for architecture.\n{}",
        gained.join("\n")
    );
    assert!(
        stale.is_empty(),
        "comments were removed; lock that in with `{UPDATE_ENV}=1 cargo test -p idx-cli --test \
         comments`:\n{}",
        stale.join("\n")
    );
}

#[test]
fn comments_are_told_apart_from_strings_chars_and_lifetimes() {
    let source = r##"fn f<'a>(s: &'a str) -> char {
    let url = "https://example.com/*not a comment*/";
    let raw = r#"// not a comment "quoted""#;
    let quote = b'"';
    let slash = '/';
    // a comment
    let x = 1; /* a block
    comment */
    '\''
}
"##;
    let lexed = lex(source);
    assert_eq!(
        lexed.comments.keys().copied().collect::<Vec<_>>(),
        [5, 6, 7],
        "{:?}",
        lexed.code
    );
}

#[test]
fn doc_comments_count_unless_clap_or_schemars_render_them() {
    let rendered = r#"
/// The CLI's about text.
#[derive(Debug, Parser)]
struct Cli {
    /// Help for --verbose.
    #[arg(long)]
    verbose: bool,
}

#[derive(Clone, Copy)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
enum Provider {
    /// Shown as a possible value.
    Msn,
}

#[derive(Subcommand)]
enum Command {
    Login {
        /// Help for a struct-like variant's field.
        #[arg(long)]
        port: Option<u16>,
    },
}
"#;
    assert_eq!(banned_lines(rendered), 0);

    let prose = r#"
//! Module prose.

/// Library prose.
#[derive(Debug, Serialize)]
struct Quote {
    /// Field prose.
    price: i64,
}

fn f() {
    // A code comment.
}
"#;
    assert_eq!(banned_lines(prose), 4);
}
