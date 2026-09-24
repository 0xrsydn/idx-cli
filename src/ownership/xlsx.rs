//! Parser for the IDX "Pemegang Saham di Atas 1%" XLSX workbook.
//!
//! Since June 2026 IDX publishes the monthly above-1% holder register as an
//! XLSX file on the "Data Kepemilikan Saham" page instead of a PDF
//! announcement. The workbook has one sheet: a disclaimer block, a `[PUBLIC]`
//! marker, then a fixed 12-column table. Only that exact header is accepted,
//! so the daily above-5% workbook and the investor-type / classification
//! workbooks published on the same page are rejected instead of mis-imported.

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use chrono::{Duration, NaiveDate};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::IdxError;
use crate::ownership::entities::{normalize_investor_type, normalize_locality, parse_ksei_date};
use crate::ownership::types::KseiHoldingDraft;

const EXPECTED_HEADER: [&str; 12] = [
    "DATE",
    "SHARE_CODE",
    "ISSUER_NAME",
    "INVESTOR_NAME",
    "INVESTOR_CLASSIFICATION",
    "LOCAL_FOREIGN",
    "NATIONALITY",
    "DOMICILE",
    "HOLDINGS_SCRIPLESS",
    "HOLDINGS_SCRIP",
    "TOTAL_HOLDING_SHARES",
    "PERCENTAGE",
];

/// Rows above this line are disclaimer text; the header must appear before it.
const MAX_HEADER_SEARCH_ROWS: usize = 50;

pub fn supports_xlsx_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("xlsx"))
}

pub fn parse_above1_xlsx_file(path: &Path) -> Result<Vec<KseiHoldingDraft>, IdxError> {
    let bytes = fs::read(path).map_err(|e| {
        IdxError::Io(format!(
            "failed to read ownership XLSX {}: {e}",
            path.display()
        ))
    })?;
    parse_above1_xlsx_bytes(&bytes)
}

pub fn parse_above1_xlsx_bytes(bytes: &[u8]) -> Result<Vec<KseiHoldingDraft>, IdxError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| IdxError::ParseError(format!("failed to open ownership XLSX: {e}")))?;

    let shared_strings = match read_zip_entry(&mut zip, "xl/sharedStrings.xml")? {
        Some(xml) => parse_shared_strings(&xml)?,
        None => Vec::new(),
    };
    let sheet_name = first_worksheet_name(&mut zip)?;
    let sheet_xml = read_zip_entry(&mut zip, &sheet_name)?
        .ok_or_else(|| IdxError::ParseError(format!("ownership XLSX is missing {sheet_name}")))?;

    let rows = parse_sheet_rows(&sheet_xml, &shared_strings)?;
    drafts_from_rows(&rows)
}

fn read_zip_entry(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<String>, IdxError> {
    let mut file = match zip.by_name(name) {
        Ok(file) => file,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => {
            return Err(IdxError::ParseError(format!(
                "failed to read ownership XLSX entry {name}: {e}"
            )));
        }
    };
    let mut output = String::new();
    file.read_to_string(&mut output).map_err(|e| {
        IdxError::ParseError(format!("failed to decode ownership XLSX entry {name}: {e}"))
    })?;
    Ok(Some(output))
}

fn first_worksheet_name(zip: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<String, IdxError> {
    let mut sheets: Vec<String> = zip
        .file_names()
        .filter(|name| name.starts_with("xl/worksheets/") && name.ends_with(".xml"))
        .filter(|name| !name["xl/worksheets/".len()..].contains('/'))
        .map(str::to_string)
        .collect();
    // sheet1.xml, sheet2.xml, ... sort by their numeric suffix.
    sheets.sort_by_key(|name| {
        name.trim_start_matches("xl/worksheets/sheet")
            .trim_end_matches(".xml")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });
    sheets
        .into_iter()
        .next()
        .ok_or_else(|| IdxError::ParseError("ownership XLSX has no worksheet".to_string()))
}

fn xml_error(context: &str, err: impl std::fmt::Display) -> IdxError {
    IdxError::ParseError(format!("failed to parse ownership XLSX {context}: {err}"))
}

/// Shared strings table: each `<si>` is one string, made of one `<t>` or
/// several rich-text runs `<r><t>`. Phonetic hints (`<rPh>`) are skipped.
fn parse_shared_strings(xml: &str) -> Result<Vec<String>, IdxError> {
    let mut reader = Reader::from_str(xml);
    let mut strings = Vec::new();
    let mut current: Option<String> = None;
    let mut in_text = false;
    let mut in_phonetic = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"si" => current = Some(String::new()),
                b"t" => in_text = true,
                b"rPh" => in_phonetic = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"si" => strings.push(String::new()),
            Ok(Event::Text(text)) if in_text && !in_phonetic => {
                if let Some(value) = current.as_mut() {
                    let unescaped = text
                        .unescape()
                        .map_err(|e| xml_error("shared strings", e))?;
                    value.push_str(&unescaped);
                }
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"si" => strings.push(current.take().unwrap_or_default()),
                b"t" => in_text = false,
                b"rPh" => in_phonetic = false,
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(xml_error("shared strings", e)),
            _ => {}
        }
    }

    Ok(strings)
}

/// One sheet row: column letters (`A`, `B`, ...) to cell text.
type SheetRow = HashMap<String, String>;

fn parse_sheet_rows(xml: &str, shared_strings: &[String]) -> Result<Vec<SheetRow>, IdxError> {
    let mut reader = Reader::from_str(xml);
    let mut rows = Vec::new();
    let mut row: Option<SheetRow> = None;
    let mut cell: Option<(String, String)> = None;
    let mut value = String::new();
    let mut in_value = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"row" => row = Some(SheetRow::new()),
                b"c" => {
                    cell = Some(cell_ref_and_type(&e)?);
                    value.clear();
                }
                b"v" | b"t" if cell.is_some() => in_value = true,
                _ => {}
            },
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"row" => rows.push(SheetRow::new()),
            Ok(Event::Text(text)) if in_value => {
                let unescaped = text.unescape().map_err(|e| xml_error("worksheet", e))?;
                value.push_str(&unescaped);
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"v" | b"t" => in_value = false,
                b"c" => {
                    if let (Some((column, cell_type)), Some(current_row)) =
                        (cell.take(), row.as_mut())
                    {
                        let text = if cell_type == "s" {
                            let index = value.trim().parse::<usize>().map_err(|e| {
                                xml_error("worksheet", format!("bad shared string index: {e}"))
                            })?;
                            shared_strings.get(index).cloned().ok_or_else(|| {
                                xml_error(
                                    "worksheet",
                                    format!("shared string {index} out of range"),
                                )
                            })?
                        } else {
                            value.clone()
                        };
                        current_row.insert(column, text);
                    }
                }
                b"row" => {
                    if let Some(current_row) = row.take() {
                        rows.push(current_row);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(xml_error("worksheet", e)),
            _ => {}
        }
    }

    Ok(rows)
}

fn cell_ref_and_type(element: &BytesStart<'_>) -> Result<(String, String), IdxError> {
    let mut column = String::new();
    let mut cell_type = String::new();
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|e| xml_error("worksheet", e))?;
        let raw = String::from_utf8_lossy(&attribute.value).to_string();
        match attribute.key.local_name().as_ref() {
            b"r" => column = raw.chars().take_while(char::is_ascii_alphabetic).collect(),
            b"t" => cell_type = raw,
            _ => {}
        }
    }
    if column.is_empty() {
        return Err(xml_error("worksheet", "cell without a reference"));
    }
    Ok((column, cell_type))
}

fn drafts_from_rows(rows: &[SheetRow]) -> Result<Vec<KseiHoldingDraft>, IdxError> {
    let (header_index, columns) = rows
        .iter()
        .take(MAX_HEADER_SEARCH_ROWS)
        .enumerate()
        .find_map(|(index, row)| header_columns(row).map(|columns| (index, columns)))
        .ok_or_else(|| {
            IdxError::ParseError(
                "ownership XLSX header did not match the above-1% holder register layout \
                 (DATE, SHARE_CODE, ... PERCENTAGE); the above-5% and investor-type \
                 workbooks are not supported"
                    .to_string(),
            )
        })?;

    let mut drafts = Vec::new();
    for (offset, row) in rows[header_index + 1..].iter().enumerate() {
        let line = header_index + offset + 2;
        let field = |name: &str| -> &str {
            columns
                .get(name)
                .and_then(|column| row.get(column))
                .map(|value| value.trim())
                .unwrap_or("")
        };

        let ticker_code = field("SHARE_CODE");
        let investor_name = field("INVESTOR_NAME");
        if ticker_code.is_empty() && investor_name.is_empty() {
            continue;
        }
        if ticker_code.is_empty() || investor_name.is_empty() {
            return Err(IdxError::ParseError(format!(
                "ownership XLSX row {line} is missing SHARE_CODE or INVESTOR_NAME"
            )));
        }

        drafts.push(KseiHoldingDraft {
            ticker_code: ticker_code.to_string(),
            issuer_name: optional(field("ISSUER_NAME")),
            raw_investor_name: investor_name.to_string(),
            investor_type: normalize_investor_type(field("INVESTOR_CLASSIFICATION")),
            locality: normalize_locality(field("LOCAL_FOREIGN")),
            nationality: optional(field("NATIONALITY")),
            domicile: optional(field("DOMICILE")),
            holdings_scripless: parse_shares(field("HOLDINGS_SCRIPLESS"), line, false)?,
            holdings_scrip: parse_shares(field("HOLDINGS_SCRIP"), line, false)?,
            total_shares: parse_shares(field("TOTAL_HOLDING_SHARES"), line, true)?,
            percentage_bps: parse_percentage_bps(field("PERCENTAGE"), line)?,
            report_date: parse_report_date(field("DATE"), line)?,
        });
    }

    if drafts.is_empty() {
        return Err(IdxError::ParseError(
            "ownership XLSX contained a header but no holder rows".to_string(),
        ));
    }

    Ok(drafts)
}

/// Map header labels to column letters if `row` is exactly the expected header.
fn header_columns(row: &SheetRow) -> Option<HashMap<&'static str, String>> {
    let mut found: HashMap<&'static str, String> = HashMap::new();
    for (column, text) in row {
        let label = text.trim().to_ascii_uppercase().replace(' ', "_");
        if label.is_empty() {
            continue;
        }
        let expected = EXPECTED_HEADER.iter().find(|name| **name == label)?;
        found.insert(expected, column.clone());
    }
    (found.len() == EXPECTED_HEADER.len()).then_some(found)
}

fn optional(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_number(raw: &str, line: usize, name: &str) -> Result<f64, IdxError> {
    raw.replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| {
            IdxError::ParseError(format!("ownership XLSX row {line}: invalid {name} '{raw}'"))
        })
}

fn parse_shares(raw: &str, line: usize, required: bool) -> Result<i64, IdxError> {
    if raw.is_empty() {
        return if required {
            Err(IdxError::ParseError(format!(
                "ownership XLSX row {line}: missing TOTAL_HOLDING_SHARES"
            )))
        } else {
            Ok(0)
        };
    }
    let value = parse_number(raw, line, "share count")?;
    if value < 0.0 || value.fract() != 0.0 || value > i64::MAX as f64 {
        return Err(IdxError::ParseError(format!(
            "ownership XLSX row {line}: share count '{raw}' is not a whole number"
        )));
    }
    Ok(value as i64)
}

/// `54.94` (percent) -> 5494 basis points. Excel stores values such as
/// `1.1499999999999999`, so round to the nearest basis point.
fn parse_percentage_bps(raw: &str, line: usize) -> Result<i64, IdxError> {
    let value = parse_number(raw, line, "PERCENTAGE")?;
    if !(0.0..=100.0).contains(&value) {
        return Err(IdxError::ParseError(format!(
            "ownership XLSX row {line}: PERCENTAGE '{raw}' is out of range"
        )));
    }
    Ok((value * 100.0).round() as i64)
}

/// DATE is an Excel serial day number (`46265` = 2026-08-31); accept the
/// KSEI text form (`31-Aug-2026`) as well in case the cell is stored as text.
fn parse_report_date(raw: &str, line: usize) -> Result<NaiveDate, IdxError> {
    if let Ok(serial) = raw.parse::<f64>() {
        let days = serial.trunc() as i64;
        let epoch = NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid Excel epoch");
        if (1..=2_958_465).contains(&days) {
            return Ok(epoch + Duration::days(days));
        }
    }
    parse_ksei_date(raw).map_err(|_| {
        IdxError::ParseError(format!("ownership XLSX row {line}: invalid DATE '{raw}'"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<KseiHoldingDraft> {
        parse_above1_xlsx_bytes(include_bytes!(
            "../../tests/fixtures/ksei_above1_20260831_excerpt.xlsx"
        ))
        .expect("above-1% XLSX excerpt parses")
    }

    #[test]
    fn parses_above1_xlsx_excerpt() {
        let drafts = fixture();
        assert!(
            drafts.len() >= 10,
            "expected excerpt rows, got {}",
            drafts.len()
        );
        assert!(
            drafts
                .iter()
                .all(|d| d.report_date == NaiveDate::from_ymd_opt(2026, 8, 31).unwrap())
        );

        let dwimuria = drafts
            .iter()
            .find(|d| {
                d.ticker_code == "BBCA" && d.raw_investor_name == "PT DWIMURIA INVESTAMA ANDALAN"
            })
            .expect("BBCA controlling holder present");
        assert_eq!(dwimuria.percentage_bps, 5494);
        assert_eq!(
            dwimuria.investor_type.as_ref().map(|t| t.0.as_str()),
            Some("CORPORATE")
        );
        assert_eq!(
            dwimuria.total_shares,
            dwimuria.holdings_scripless + dwimuria.holdings_scrip
        );
    }

    #[test]
    fn rounds_float_noise_in_percentages() {
        assert_eq!(parse_percentage_bps("1.1499999999999999", 1).unwrap(), 115);
        assert_eq!(parse_percentage_bps("41.1", 1).unwrap(), 4110);
        assert!(parse_percentage_bps("100.5", 1).is_err());
    }

    #[test]
    fn parses_excel_serial_and_text_dates() {
        let expected = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        assert_eq!(parse_report_date("46265", 1).unwrap(), expected);
        assert_eq!(parse_report_date("31-Aug-2026", 1).unwrap(), expected);
        assert!(parse_report_date("not a date", 1).is_err());
    }

    #[test]
    fn rejects_above5_workbook() {
        let err = parse_above1_xlsx_bytes(include_bytes!(
            "../../tests/fixtures/ksei_above5_20260923_excerpt.xlsx"
        ))
        .expect_err("above-5% workbook must be rejected");
        assert!(
            err.to_string().contains("above-1% holder register layout"),
            "{err}"
        );
    }

    #[test]
    fn rejects_non_xlsx_bytes() {
        assert!(parse_above1_xlsx_bytes(b"%PDF-1.7 not a workbook").is_err());
    }
}
