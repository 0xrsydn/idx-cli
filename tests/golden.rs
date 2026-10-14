//! Golden output tests: byte-for-byte parity of CLI stdout, stderr, and exit
//! code for every shipped command (see "Parity and Verification" in
//! docs/DESIGN.md).
//!
//! Cases live in `tests/golden/cases.toml`; expected output in
//! `tests/golden/<case>.golden`. Regenerate only for an intentional, reviewed
//! output change, and commit the diff with the reason:
//!
//!     UPDATE_GOLDEN=1 cargo test --test golden

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;

const IDX: &str = env!("CARGO_BIN_EXE_idx");
const REPO: &str = env!("CARGO_MANIFEST_DIR");

#[derive(Deserialize)]
struct CaseFile {
    case: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    args: Vec<String>,
    /// Output formats to expand the case into (`-o <format>` is prepended and
    /// `.<format>` appended to the name). Empty means run the args as given.
    #[serde(default = "default_formats")]
    formats: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Seed the fixture ownership DB before running.
    #[serde(default)]
    ownership: bool,
    /// Commands run first in the same environment; their output is ignored.
    #[serde(default)]
    before: Vec<Vec<String>>,
}

fn default_formats() -> Vec<String> {
    vec!["table".into(), "json".into()]
}

fn golden_dir() -> PathBuf {
    Path::new(REPO).join("tests").join("golden")
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("idx-golden-{}", std::process::id()))
}

/// Runs idx with an isolated home under `root` and only the given IDX_* env.
fn idx(root: &Path, args: &[String], env: &BTreeMap<String, String>) -> std::process::Output {
    let mut cmd = Command::new(IDX);
    cmd.current_dir(REPO).args(args);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("IDX_") || key == "NO_COLOR" {
            cmd.env_remove(key);
        }
    }
    cmd.env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("IDX_USE_MOCK_PROVIDER", "1")
        .envs(env);
    cmd.output().expect("run idx")
}

/// Fixture ownership DB: the above-1% XLSX (2026-08-31) plus two KSEI archive
/// months (2026-01-30, 2026-02-27), with wall-clock columns pinned.
fn seeded_ownership_db() -> &'static Path {
    static DB: OnceLock<PathBuf> = OnceLock::new();
    DB.get_or_init(|| {
        let root = work_dir().join("seed");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create seed dir");
        // The importer derives the release from the IDX file name.
        let xlsx = root.join("peng-2026-08-00017-satu-persen.xlsx");
        fs::copy(
            Path::new(REPO).join("tests/fixtures/ksei_above1_20260831_excerpt.xlsx"),
            &xlsx,
        )
        .expect("copy xlsx fixture");
        let inputs = [
            xlsx,
            Path::new(REPO).join("tests/fixtures/ksei_balancepos_20260130_excerpt.txt"),
            Path::new(REPO).join("tests/fixtures/ksei_balancepos_20260227_excerpt.txt"),
        ];
        for input in inputs {
            let args = ["ownership", "import", "--file", input.to_str().unwrap()]
                .map(String::from)
                .to_vec();
            let out = idx(&root, &args, &BTreeMap::new());
            assert!(out.status.success(), "seed import failed: {out:?}");
        }
        let db = root.join("data/idx/ownership.db");
        let conn = rusqlite::Connection::open(&db).expect("open seed db");
        conn.execute_batch(
            "UPDATE ownership_releases SET imported_at = 0;
             UPDATE entities SET created_at = 0, updated_at = 0;
             UPDATE bing_holdings SET fetched_at = 0;",
        )
        .expect("pin timestamps");
        db
    })
}

fn render(root: &Path, out: &std::process::Output) -> String {
    let normalize = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .replace(root.to_str().unwrap(), "<ROOT>")
            .replace(REPO, "<REPO>")
    };
    format!(
        "exit: {}\n--- stdout\n{}--- stderr\n{}",
        out.status.code().map_or("signal".into(), |c| c.to_string()),
        normalize(&out.stdout),
        normalize(&out.stderr),
    )
}

#[test]
fn cli_output_matches_golden_files() {
    let raw = fs::read_to_string(golden_dir().join("cases.toml")).expect("read cases.toml");
    let cases: CaseFile = toml::from_str(&raw).expect("parse cases.toml");
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut seen = BTreeSet::new();
    let mut failures = Vec::new();

    for case in &cases.case {
        let runs: Vec<(String, Vec<String>)> = if case.formats.is_empty() {
            vec![(case.name.clone(), case.args.clone())]
        } else {
            case.formats
                .iter()
                .map(|format| {
                    let mut args = vec!["-o".to_string(), format.clone()];
                    args.extend(case.args.iter().cloned());
                    (format!("{}.{format}", case.name), args)
                })
                .collect()
        };

        for (name, args) in runs {
            assert!(seen.insert(name.clone()), "duplicate golden case {name}");
            let root = work_dir().join(&name);
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("create case dir");
            if case.ownership {
                let target = root.join("data/idx/ownership.db");
                fs::create_dir_all(target.parent().unwrap()).expect("create data dir");
                fs::copy(seeded_ownership_db(), &target).expect("copy seeded db");
            }
            for before in &case.before {
                idx(&root, before, &case.env);
            }

            let actual = render(&root, &idx(&root, &args, &case.env));
            let path = golden_dir().join(format!("{name}.golden"));
            if update {
                fs::write(&path, &actual).expect("write golden");
            } else {
                match fs::read_to_string(&path) {
                    Ok(expected) if expected == actual => {}
                    Ok(expected) => failures.push(format!(
                        "{name}: output differs\n--- expected\n{expected}\n--- actual\n{actual}"
                    )),
                    Err(_) => failures.push(format!("{name}: missing {}", path.display())),
                }
            }
        }
    }

    for entry in fs::read_dir(golden_dir()).expect("read golden dir") {
        let path = entry.expect("dir entry").path();
        let Some(stem) = path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_suffix(".golden")
        else {
            continue;
        };
        if !seen.contains(stem) {
            if update {
                fs::remove_file(&path).expect("remove orphan golden");
            } else {
                failures.push(format!("{stem}: golden file has no case in cases.toml"));
            }
        }
    }

    let _ = fs::remove_dir_all(work_dir());
    assert!(
        failures.is_empty(),
        "{} golden mismatch(es); if the change is intentional, rerun with UPDATE_GOLDEN=1 \
         and commit the diff with the reason\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
