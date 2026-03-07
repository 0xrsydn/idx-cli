use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::IdxError;
use crate::ownership::types::KseiRawRow;

/// Character grid for a single PDF page: y-coord → column-index → sorted (x, char) pairs.
type PageGrid = HashMap<i32, HashMap<usize, Vec<(i32, char)>>>;

const Y_TOLERANCE: f32 = 0.8;

/// Inclusive-left, exclusive-right X ranges for each KSEI data column.
const COLUMN_BOUNDS: [(f32, f32); 12] = [
    (15.0, 52.0),   // date
    (52.0, 70.0),   // share_code
    (70.0, 167.0),  // issuer_name
    (167.0, 432.0), // investor_name
    (432.0, 463.0), // investor_type
    (463.0, 497.0), // local_foreign
    (497.0, 558.0), // nationality
    (558.0, 615.0), // domicile
    (615.0, 653.0), // holdings_scripless
    (653.0, 692.0), // holdings_scrip
    (692.0, 745.0), // total_holding_shares
    (745.0, 800.0), // percentage
];

/// Parse a KSEI ownership PDF into raw rows.
/// Shells out to `mutool` for XML extraction, then parses with quick-xml.
pub fn parse_ksei_pdf(path: &Path) -> Result<Vec<KseiRawRow>, IdxError> {
    check_mutool()?;

    let output = Command::new("mutool")
        .arg("convert")
        .arg("-F")
        .arg("stext")
        .arg("-o")
        .arg("-")
        .arg(path)
        .output()
        .map_err(|e| IdxError::PdfParseError(format!("failed to run mutool: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IdxError::PdfParseError(format!(
            "mutool convert failed: {}",
            stderr.trim()
        )));
    }

    let xml = String::from_utf8(output.stdout)
        .map_err(|e| IdxError::PdfParseError(format!("invalid utf-8 stext output: {e}")))?;

    parse_stext_xml(&xml)
}

/// Parse mutool stext XML output into raw rows.
/// Pure function — takes XML string, returns parsed rows.
pub fn parse_stext_xml(xml: &str) -> Result<Vec<KseiRawRow>, IdxError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut rows: Vec<KseiRawRow> = Vec::new();
    let mut current_page: Option<PageGrid> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"page" => {
                current_page = Some(HashMap::new());
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"char" => {
                if let Some(page) = current_page.as_mut() {
                    let mut x: Option<f32> = None;
                    let mut y: Option<f32> = None;
                    let mut c: Option<char> = None;

                    for attr_result in e.attributes().with_checks(false) {
                        let attr = attr_result.map_err(|err| {
                            IdxError::PdfParseError(format!("invalid XML attribute: {err}"))
                        })?;

                        match attr.key.as_ref() {
                            b"x" => {
                                let s = attr.decode_and_unescape_value(reader.decoder()).map_err(
                                    |err| {
                                        IdxError::PdfParseError(format!(
                                            "invalid XML x attribute: {err}"
                                        ))
                                    },
                                )?;
                                x = s.parse::<f32>().ok();
                            }
                            b"y" => {
                                let s = attr.decode_and_unescape_value(reader.decoder()).map_err(
                                    |err| {
                                        IdxError::PdfParseError(format!(
                                            "invalid XML y attribute: {err}"
                                        ))
                                    },
                                )?;
                                y = s.parse::<f32>().ok();
                            }
                            b"c" => {
                                let s = attr.decode_and_unescape_value(reader.decoder()).map_err(
                                    |err| {
                                        IdxError::PdfParseError(format!(
                                            "invalid XML char attribute: {err}"
                                        ))
                                    },
                                )?;
                                c = s.chars().next();
                            }
                            _ => {}
                        }
                    }

                    if let (Some(x_val), Some(y_val), Some(ch)) = (x, y, c)
                        && let Some(col_idx) = x_to_column(x_val)
                    {
                        let yb = y_bucket(y_val);
                        let xi = (x_val * 100.0).round() as i32;
                        page.entry(yb)
                            .or_default()
                            .entry(col_idx)
                            .or_default()
                            .push((xi, ch));
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"page" => {
                if let Some(page) = current_page.take() {
                    rows.extend(extract_rows_from_page(page));
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => {
                return Err(IdxError::PdfParseError(format!(
                    "failed to parse stext XML: {err}"
                )));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(rows)
}

/// Check if mutool is available in PATH.
pub fn check_mutool() -> Result<(), IdxError> {
    // mutool with no args prints usage to stderr and exits non-zero,
    // so we just check that the binary is found and executable.
    Command::new("mutool")
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| IdxError::PdfParseError(format!("mutool not found in PATH: {e}")))?;
    Ok(())
}

fn extract_rows_from_page(page: PageGrid) -> Vec<KseiRawRow> {
    let mut page_rows = Vec::new();

    let mut y_keys: Vec<i32> = page.keys().copied().collect();
    y_keys.sort_unstable();

    for y in y_keys {
        let mut row = KseiRawRow {
            date: String::new(),
            share_code: String::new(),
            issuer_name: String::new(),
            investor_name: String::new(),
            investor_type: String::new(),
            local_foreign: String::new(),
            nationality: String::new(),
            domicile: String::new(),
            holdings_scripless: String::new(),
            holdings_scrip: String::new(),
            total_holding_shares: String::new(),
            percentage: String::new(),
        };

        if let Some(col_map) = page.get(&y) {
            for (col_idx, chars) in col_map {
                let mut sorted = chars.clone();
                sorted.sort_by_key(|(x, _)| *x);
                let text = normalize_spaces(&sorted.iter().map(|(_, c)| c).collect::<String>());
                assign_column(&mut row, *col_idx, text);
            }
        }

        if is_data_row(&row) {
            page_rows.push(row);
        }
    }

    page_rows
}

fn assign_column(row: &mut KseiRawRow, col_idx: usize, value: String) {
    match col_idx {
        0 => row.date = value,
        1 => row.share_code = value,
        2 => row.issuer_name = value,
        3 => row.investor_name = value,
        4 => row.investor_type = value,
        5 => row.local_foreign = value,
        6 => row.nationality = value,
        7 => row.domicile = value,
        8 => row.holdings_scripless = value,
        9 => row.holdings_scrip = value,
        10 => row.total_holding_shares = value,
        11 => row.percentage = value,
        _ => {}
    }
}

fn x_to_column(x: f32) -> Option<usize> {
    COLUMN_BOUNDS
        .iter()
        .position(|(left, right)| x >= *left && x < *right)
}

fn y_bucket(y: f32) -> i32 {
    (y / Y_TOLERANCE).round() as i32
}

fn normalize_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;

    for ch in input.chars() {
        if ch == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }

    out.trim().to_string()
}

fn is_data_row(row: &KseiRawRow) -> bool {
    is_ksei_date(&row.date)
        && is_percentage_like(&row.percentage)
        && !row.share_code.trim().is_empty()
        && !row.investor_name.trim().is_empty()
}

fn is_ksei_date(s: &str) -> bool {
    if s.len() != 11 {
        return false;
    }

    let mut parts = s.split('-');
    let day = parts.next();
    let mon = parts.next();
    let year = parts.next();

    if parts.next().is_some() {
        return false;
    }

    match (day, mon, year) {
        (Some(d), Some(m), Some(y)) => {
            d.len() == 2
                && d.chars().all(|c| c.is_ascii_digit())
                && m.len() == 3
                && m.chars().all(|c| c.is_ascii_alphabetic())
                && y.len() == 4
                && y.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

fn is_percentage_like(s: &str) -> bool {
    let cleaned = s.trim();
    if cleaned.is_empty() {
        return false;
    }

    let mut parts = cleaned.split(',');
    let left = parts.next();
    let right = parts.next();

    if parts.next().is_some() {
        return false;
    }

    match (left, right) {
        (Some(l), Some(r)) => {
            !l.is_empty()
                && l.chars().all(|c| c.is_ascii_digit())
                && !r.is_empty()
                && r.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{check_mutool, parse_ksei_pdf, parse_stext_xml};

    #[test]
    fn test_parse_stext_xml_fixture_extracts_rows() {
        let fixture_path = Path::new("tests/fixtures/ksei_stext_sample.xml");
        let xml = fs::read_to_string(fixture_path).expect("failed to read stext fixture");

        let rows = parse_stext_xml(&xml).expect("failed to parse fixture XML");
        assert_eq!(rows.len(), 3);

        let first = &rows[0];
        assert_eq!(first.date, "27-Feb-2026");
        assert_eq!(first.share_code, "BBCA");
        assert_eq!(first.investor_name, "PT DWIMURIA INVESTAMA ANDALAN");
        assert_eq!(first.percentage, "54,94");
    }

    #[test]
    fn test_parse_ksei_pdf_real_file_row_count() {
        if check_mutool().is_err() {
            eprintln!("skipping mutool-dependent test: mutool not available");
            return;
        }

        let pdf_path =
            Path::new("/var/lib/openclaw/projects/idx-cli/research/ownership_202603.pdf");
        if !pdf_path.exists() {
            eprintln!("skipping mutool-dependent test: sample PDF not found");
            return;
        }

        let rows = parse_ksei_pdf(pdf_path).expect("failed to parse real KSEI PDF");
        assert!(
            rows.len() >= 7_200,
            "expected at least 7200 rows, got {}",
            rows.len()
        );
    }
}
