use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::IdxError;
use crate::ownership::types::KseiRawRow;

const Y_TOLERANCE: f32 = 0.8;
const HEADER_MATCH_MIN: usize = 4;

const HEADER_LABELS: &[&str] = &[
    "DATE",
    "SHARECODE",
    "ISSUERNAME",
    "INVESTORNAME",
    "INVESTORTYPE",
    "LOCALFOREIGN",
    "NATIONALITY",
    "DOMICILE",
    "HOLDINGSSCRIPLESS",
    "HOLDINGSSCRIP",
    "TOTALHOLDINGSHARES",
    "PERCENTAGE",
];

#[derive(Debug, Clone)]
struct PageLine {
    x: f32,
    y: f32,
    text: String,
}

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
    let mut current_page: Option<Vec<PageLine>> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"page" => {
                current_page = Some(Vec::new());
            }
            Ok(Event::Start(e)) if e.name().as_ref() == b"line" => {
                if let Some(page) = current_page.as_mut()
                    && let Some(line) = parse_line_attrs(&reader, &e)?
                {
                    page.push(line);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"page" => {
                if let Some(page) = current_page.take() {
                    rows.extend(extract_rows_from_page(&page));
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

fn parse_line_attrs(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<Option<PageLine>, IdxError> {
    let mut bbox: Option<String> = None;
    let mut text: Option<String> = None;

    for attr_result in event.attributes().with_checks(false) {
        let attr = attr_result
            .map_err(|err| IdxError::PdfParseError(format!("invalid XML attribute: {err}")))?;

        match attr.key.as_ref() {
            b"bbox" => {
                bbox = Some(
                    attr.decode_and_unescape_value(reader.decoder())
                        .map_err(|err| {
                            IdxError::PdfParseError(format!("invalid XML bbox attribute: {err}"))
                        })?
                        .to_string(),
                );
            }
            b"text" => {
                text = Some(
                    attr.decode_and_unescape_value(reader.decoder())
                        .map_err(|err| {
                            IdxError::PdfParseError(format!("invalid XML text attribute: {err}"))
                        })?
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    let Some(text) = text.map(|value| normalize_spaces(&value)) else {
        return Ok(None);
    };
    if text.is_empty() {
        return Ok(None);
    }

    let Some(bbox) = bbox else {
        return Ok(None);
    };
    let mut parts = bbox.split_whitespace();
    let x = parts.next().and_then(|value| value.parse::<f32>().ok());
    let y = parts.next().and_then(|value| value.parse::<f32>().ok());

    match (x, y) {
        (Some(x), Some(y)) => Ok(Some(PageLine { x, y, text })),
        _ => Ok(None),
    }
}

fn extract_rows_from_page(page: &[PageLine]) -> Vec<KseiRawRow> {
    let mut rows_by_y: HashMap<i32, Vec<PageLine>> = HashMap::new();
    for line in page {
        rows_by_y
            .entry(y_bucket(line.y))
            .or_default()
            .push(line.clone());
    }

    let mut y_keys: Vec<i32> = rows_by_y.keys().copied().collect();
    y_keys.sort_unstable();

    let mut rows = Vec::new();
    for y in y_keys {
        let Some(lines) = rows_by_y.get(&y) else {
            continue;
        };

        let mut sorted = lines.clone();
        sorted.sort_by(|left, right| left.x.total_cmp(&right.x));

        let texts: Vec<String> = sorted.into_iter().map(|line| line.text).collect();
        if is_header_row(&texts) {
            continue;
        }

        let row = parse_row_segments(&texts);
        if is_data_row(&row) {
            rows.push(row);
        }
    }

    rows
}

fn is_header_row(texts: &[String]) -> bool {
    texts
        .iter()
        .filter(|text| HEADER_LABELS.contains(&normalize_header_label(text).as_str()))
        .count()
        >= HEADER_MATCH_MIN
}

fn parse_row_segments(texts: &[String]) -> KseiRawRow {
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

    let mut remaining: Vec<String> = texts
        .iter()
        .map(|text| normalize_spaces(text))
        .filter(|text| !text.is_empty())
        .collect();
    if remaining.is_empty() {
        return row;
    }

    if let Some((date, share_code)) = split_date_and_share(&remaining[0]) {
        row.date = date;
        row.share_code = share_code;
        let _ = remaining.remove(0);
    }

    if row.date.is_empty() {
        return row;
    }

    if row.share_code.is_empty()
        && remaining
            .first()
            .is_some_and(|segment| is_share_code_like(segment))
    {
        row.share_code = remaining.remove(0);
    }

    if remaining
        .last()
        .is_some_and(|segment| is_percentage_like(segment))
    {
        row.percentage = remaining.pop().unwrap_or_default();
    }
    if remaining
        .last()
        .is_some_and(|segment| is_id_number_like(segment))
    {
        row.total_holding_shares = remaining.pop().unwrap_or_default();
    }
    if remaining
        .last()
        .is_some_and(|segment| is_id_number_like(segment))
    {
        row.holdings_scrip = remaining.pop().unwrap_or_default();
    }
    if remaining
        .last()
        .is_some_and(|segment| is_id_number_like(segment))
    {
        row.holdings_scripless = remaining.pop().unwrap_or_default();
    }

    let mut geo_fields = Vec::new();
    while remaining.len() > 2 {
        let Some(candidate) = remaining.last().cloned() else {
            break;
        };

        if row.local_foreign.is_empty() && is_locality_marker(&candidate) {
            row.local_foreign = remaining.pop().unwrap_or_default();
            continue;
        }

        if row.investor_type.is_empty() && is_investor_type_marker(&candidate) {
            row.investor_type = remaining.pop().unwrap_or_default();
            continue;
        }

        if geo_fields.len() < 2 {
            geo_fields.push(remaining.pop().unwrap_or_default());
            continue;
        }

        break;
    }

    geo_fields.reverse();
    if let Some(first) = geo_fields.first() {
        row.nationality = first.clone();
    }
    if geo_fields.len() > 1 {
        row.domicile = geo_fields[1..].join(" ");
    }

    if let Some(first) = remaining.first() {
        row.issuer_name = first.clone();
    }
    if remaining.len() > 1 {
        row.investor_name = remaining[1..].join(" ");
    }

    row
}

fn split_date_and_share(segment: &str) -> Option<(String, String)> {
    let trimmed = segment.trim();
    if trimmed.len() < 11 {
        return None;
    }

    let date = trimmed.get(..11)?.to_string();
    if !is_ksei_date(&date) {
        return None;
    }

    let share_code = trimmed.get(11..).unwrap_or_default().trim().to_string();
    Some((date, share_code))
}

fn is_share_code_like(value: &str) -> bool {
    let trimmed = value.trim();
    (3..=8).contains(&trimmed.len())
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn is_id_number_like(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed != "-"
        && trimmed.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
}

fn is_investor_type_marker(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.len() <= 4 && trimmed.chars().all(|ch| ch.is_ascii_uppercase())
}

fn is_locality_marker(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "L" | "F" | "A" | "D" | "LOCAL" | "FOREIGN" | "ASING" | "DOMESTIC"
    )
}

fn normalize_header_label(text: &str) -> String {
    let mut normalized = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_uppercase());
        }
    }
    normalized
}

fn y_bucket(y: f32) -> i32 {
    (y / Y_TOLERANCE).round() as i32
}

fn normalize_spaces(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;

    for ch in input.chars() {
        if ch.is_whitespace() {
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
    use std::path::Path;

    use super::{check_mutool, parse_ksei_pdf, parse_stext_xml};

    #[test]
    fn test_parse_stext_xml_live_like_lines_extract_rows() {
        let xml = build_live_like_stext_xml();

        let rows = parse_stext_xml(&xml).expect("failed to parse live-like fixture XML");
        assert_eq!(rows.len(), 3);

        let first = &rows[0];
        assert_eq!(first.date, "27-Feb-2026");
        assert_eq!(first.share_code, "AADI");
        assert_eq!(first.issuer_name, "ADARO ANDALAN INDONESIA Tbk");
        assert_eq!(first.investor_name, "ADARO STRATEGIC INVESTMENTS");
        assert_eq!(first.investor_type, "CP");
        assert_eq!(first.local_foreign, "D");
        assert_eq!(first.nationality, "INDONESIA");
        assert_eq!(first.holdings_scripless, "3.200.142.830");
        assert_eq!(first.holdings_scrip, "0");
        assert_eq!(first.total_holding_shares, "3.200.142.830");
        assert_eq!(first.percentage, "66,18");

        let last = &rows[2];
        assert_eq!(last.share_code, "BBRI");
        assert_eq!(last.investor_name, "PT NUSANTARA CAPITAL");
        assert_eq!(last.investor_type, "ID");
        assert_eq!(last.local_foreign, "A");
        assert_eq!(last.nationality, "SINGAPORE");
        assert_eq!(last.percentage, "15,00");
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

    fn build_live_like_stext_xml() -> String {
        let mut xml = String::from(r#"<?xml version="1.0"?><document>"#);

        append_page(
            &mut xml,
            "page1",
            &[
                (31.56, 86.33, "DATE"),
                (56.64, 86.33, "SHARE_CODE"),
                (121.22, 86.33, "ISSUER_NAME"),
                (267.17, 86.33, "INVESTOR_NAME"),
                (390.89, 86.33, "INVESTOR_TYPE"),
                (434.11, 86.33, "LOCAL_FOREIGN"),
                (475.87, 86.33, "NATIONALITY"),
                (525.19, 86.33, "DOMICILE"),
                (574.63, 86.33, "HOLDINGS_SCRIPLESS"),
                (629.98, 86.33, "HOLDINGS_SCRIP"),
                (680.02, 86.33, "TOTAL_HOLDING_SHARES"),
                (741.22, 86.33, "PERCENTAGE"),
                (28.68, 91.01, "27-Feb-2026 AADI"),
                (85.10, 91.01, "ADARO ANDALAN INDONESIA Tbk"),
                (179.30, 91.01, "ADARO STRATEGIC INVESTMENTS"),
                (381.41, 91.01, "CP"),
                (424.75, 91.01, "D"),
                (504.07, 91.01, "INDONESIA"),
                (597.58, 91.01, "3.200.142.830"),
                (630.10, 91.01, "0"),
                (680.12, 91.01, "3.200.142.830"),
                (741.30, 91.01, "66,18"),
                (28.68, 96.01, "27-Feb-2026 AADI"),
                (85.10, 96.01, "ADARO ANDALAN INDONESIA Tbk"),
                (179.30, 96.01, "PUBLIC"),
                (381.41, 96.01, "OT"),
                (424.75, 96.01, "A"),
                (504.07, 96.01, "SINGAPORE"),
                (597.58, 96.01, "500.000.000"),
                (630.10, 96.01, "0"),
                (680.12, 96.01, "500.000.000"),
                (741.30, 96.01, "10,34"),
            ],
        );

        append_page(
            &mut xml,
            "page2",
            &[
                (28.68, 20.00, "27-Feb-2026 BBRI"),
                (85.10, 20.00, "BANK RAKYAT INDONESIA Tbk"),
                (179.30, 20.00, "PT NUSANTARA CAPITAL"),
                (381.41, 20.00, "ID"),
                (424.75, 20.00, "A"),
                (504.07, 20.00, "SINGAPORE"),
                (597.58, 20.00, "1.250.000.000"),
                (630.10, 20.00, "0"),
                (680.12, 20.00, "1.250.000.000"),
                (741.30, 20.00, "15,00"),
            ],
        );

        xml.push_str("</document>");
        xml
    }

    fn append_page(xml: &mut String, id: &str, lines: &[(f32, f32, &str)]) {
        xml.push_str(&format!(r#"<page id="{id}" width="792" height="612">"#));
        for (x, y, text) in lines {
            append_line(xml, *x, *y, text);
        }
        xml.push_str("</page>");
    }

    fn append_line(xml: &mut String, x: f32, y: f32, text: &str) {
        let width = x + (text.len() as f32 * 2.0);
        let height = y + 3.48;
        xml.push_str(&format!(
            r#"<line bbox="{x:.2} {y:.2} {width:.2} {height:.2}" text="{}"></line>"#,
            escape_xml_attr(text)
        ));
    }

    fn escape_xml_attr(text: &str) -> String {
        text.chars()
            .map(|ch| match ch {
                '&' => "&amp;".to_string(),
                '<' => "&lt;".to_string(),
                '>' => "&gt;".to_string(),
                '"' => "&quot;".to_string(),
                '\'' => "&apos;".to_string(),
                other => other.to_string(),
            })
            .collect::<String>()
    }
}
