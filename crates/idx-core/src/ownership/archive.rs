use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use chrono::NaiveDate;

use crate::error::IdxError;
use crate::ownership::types::{InvestorTypeCode, KseiHoldingDraft, Locality};

const EXPECTED_HEADER: &[&str] = &[
    "Date",
    "Code",
    "Type",
    "Sec. Num",
    "Price",
    "Local IS",
    "Local CP",
    "Local PF",
    "Local IB",
    "Local ID",
    "Local MF",
    "Local SC",
    "Local FD",
    "Local OT",
    "Total",
    "Foreign IS",
    "Foreign CP",
    "Foreign PF",
    "Foreign IB",
    "Foreign ID",
    "Foreign MF",
    "Foreign SC",
    "Foreign FD",
    "Foreign OT",
    "Total",
];

const LOCAL_BUCKETS: &[(&str, usize)] = &[
    ("IS", 5),
    ("CP", 6),
    ("PF", 7),
    ("IB", 8),
    ("ID", 9),
    ("MF", 10),
    ("SC", 11),
    ("FD", 12),
    ("OT", 13),
];

const FOREIGN_BUCKETS: &[(&str, usize)] = &[
    ("IS", 15),
    ("CP", 16),
    ("PF", 17),
    ("IB", 18),
    ("ID", 19),
    ("MF", 20),
    ("SC", 21),
    ("FD", 22),
    ("OT", 23),
];

pub fn supports_local_archive_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| {
            let ext = value.trim().to_ascii_lowercase();
            ext == "txt" || ext == "zip"
        })
        .unwrap_or(false)
}

pub fn parse_balancepos_file(path: &Path) -> Result<Vec<KseiHoldingDraft>, IdxError> {
    let raw = match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("txt") => fs::read_to_string(path).map_err(|e| {
            IdxError::Io(format!(
                "failed to read KSEI archive TXT {}: {e}",
                path.display()
            ))
        })?,
        Some("zip") => extract_txt_from_zip(path)?,
        _ => {
            return Err(IdxError::InvalidInput(format!(
                "unsupported local ownership archive file {}; expected .txt or .zip",
                path.display()
            )));
        }
    };

    parse_balancepos_text(&raw)
}

pub fn parse_balancepos_text(raw: &str) -> Result<Vec<KseiHoldingDraft>, IdxError> {
    let mut lines = raw.lines().filter(|line| !line.trim().is_empty());
    let header = lines
        .next()
        .ok_or_else(|| IdxError::ParseError("empty KSEI archive TXT input".to_string()))?;
    validate_header(header)?;

    let mut drafts = Vec::new();
    for (line_number, line) in lines.enumerate() {
        let columns: Vec<&str> = line.split('|').map(str::trim).collect();
        if columns.len() != EXPECTED_HEADER.len() {
            return Err(IdxError::ParseError(format!(
                "invalid KSEI archive TXT row {}: expected {} columns, got {}",
                line_number + 2,
                EXPECTED_HEADER.len(),
                columns.len()
            )));
        }

        if !columns[2].eq_ignore_ascii_case("EQUITY") {
            continue;
        }

        let report_date = parse_archive_date(columns[0])?;
        let ticker_code = columns[1].trim().to_uppercase();
        let sec_num = parse_archive_number(columns[3], line_number + 2, "Sec. Num")?;
        if sec_num <= 0 {
            continue;
        }

        append_bucket_drafts(
            &mut drafts,
            &ticker_code,
            report_date,
            sec_num,
            columns.as_slice(),
            Locality::Local,
            LOCAL_BUCKETS,
        )?;
        append_bucket_drafts(
            &mut drafts,
            &ticker_code,
            report_date,
            sec_num,
            columns.as_slice(),
            Locality::Foreign,
            FOREIGN_BUCKETS,
        )?;
    }

    if drafts.is_empty() {
        return Err(IdxError::ParseError(
            "no importable EQUITY rows found in KSEI archive TXT".to_string(),
        ));
    }

    Ok(drafts)
}

fn extract_txt_from_zip(path: &Path) -> Result<String, IdxError> {
    let bytes = fs::read(path).map_err(|e| {
        IdxError::Io(format!(
            "failed to read KSEI archive ZIP {}: {e}",
            path.display()
        ))
    })?;
    let cursor = Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor)
        .map_err(|e| IdxError::ParseError(format!("failed to open KSEI archive ZIP: {e}")))?;

    for index in 0..zip.len() {
        let mut file = zip.by_index(index).map_err(|e| {
            IdxError::ParseError(format!("failed to read KSEI archive ZIP entry: {e}"))
        })?;
        if file.is_dir() {
            continue;
        }

        let name = file.name().to_ascii_lowercase();
        if !name.ends_with(".txt") {
            continue;
        }

        let mut output = String::new();
        file.read_to_string(&mut output).map_err(|e| {
            IdxError::ParseError(format!("failed to decode KSEI archive TXT entry: {e}"))
        })?;
        return Ok(output);
    }

    Err(IdxError::ParseError(
        "KSEI archive ZIP did not contain a TXT payload".to_string(),
    ))
}

fn validate_header(header: &str) -> Result<(), IdxError> {
    let columns: Vec<&str> = header.split('|').map(str::trim).collect();
    if columns != EXPECTED_HEADER {
        return Err(IdxError::ParseError(
            "KSEI archive TXT header did not match the expected balancepos layout".to_string(),
        ));
    }
    Ok(())
}

fn parse_archive_date(raw: &str) -> Result<NaiveDate, IdxError> {
    let trimmed = raw.trim();
    if trimmed.len() != 11 {
        return Err(IdxError::ParseError(format!(
            "invalid KSEI archive date `{trimmed}`"
        )));
    }

    let canonical = format!(
        "{}-{}-{}",
        &trimmed[..2],
        titlecase_month(&trimmed[3..6]),
        &trimmed[7..11]
    );

    NaiveDate::parse_from_str(&canonical, "%d-%b-%Y")
        .map_err(|e| IdxError::ParseError(format!("invalid KSEI archive date `{trimmed}`: {e}")))
}

fn titlecase_month(raw: &str) -> String {
    let upper = raw.trim().to_ascii_uppercase();
    let mut chars = upper.chars();
    match chars.next() {
        Some(first) => {
            let mut output = String::new();
            output.push(first.to_ascii_uppercase());
            output.push_str(&chars.as_str().to_ascii_lowercase());
            output
        }
        None => String::new(),
    }
}

fn parse_archive_number(raw: &str, line_number: usize, field: &str) -> Result<i64, IdxError> {
    raw.trim().parse::<i64>().map_err(|e| {
        IdxError::ParseError(format!(
            "invalid KSEI archive TXT value in row {line_number} field `{field}`: {e}"
        ))
    })
}

fn append_bucket_drafts(
    drafts: &mut Vec<KseiHoldingDraft>,
    ticker_code: &str,
    report_date: NaiveDate,
    sec_num: i64,
    columns: &[&str],
    locality: Locality,
    buckets: &[(&str, usize)],
) -> Result<(), IdxError> {
    for (investor_type, column_index) in buckets {
        let shares =
            parse_archive_number(columns[*column_index], 0, investor_type).map_err(|_| {
                IdxError::ParseError(format!(
                    "invalid KSEI archive TXT share count for {ticker_code} {investor_type}"
                ))
            })?;
        if shares <= 0 {
            continue;
        }

        drafts.push(KseiHoldingDraft {
            ticker_code: ticker_code.to_string(),
            issuer_name: None,
            raw_investor_name: synthetic_holder_name(locality, investor_type),
            investor_type: Some(InvestorTypeCode((*investor_type).to_string())),
            locality: Some(locality),
            nationality: None,
            domicile: None,
            holdings_scripless: shares,
            holdings_scrip: 0,
            total_shares: shares,
            percentage_bps: compute_percentage_bps(shares, sec_num),
            report_date,
        });
    }

    Ok(())
}

fn synthetic_holder_name(locality: Locality, investor_type: &str) -> String {
    let prefix = match locality {
        Locality::Local => "LOCAL",
        Locality::Foreign => "FOREIGN",
    };
    format!("KSEI AGGREGATE {prefix} {investor_type}")
}

fn compute_percentage_bps(shares: i64, sec_num: i64) -> i64 {
    let shares_i128 = i128::from(shares);
    let sec_num_i128 = i128::from(sec_num);
    let rounded = ((shares_i128 * 10_000) + (sec_num_i128 / 2)) / sec_num_i128;
    i64::try_from(rounded).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::parse_balancepos_text;
    use crate::ownership::entities::normalize_ksei_row;
    use crate::ownership::parser::parse_stext_xml;
    use crate::ownership::types::Locality;

    #[test]
    fn parses_balancepos_excerpt_into_bucket_holders() {
        let raw = include_str!("../../tests/fixtures/ksei_balancepos_20260227_excerpt.txt");
        let drafts = parse_balancepos_text(raw).expect("balancepos excerpt parses");

        let local_cp = drafts
            .iter()
            .find(|draft| {
                draft.ticker_code == "AADI"
                    && draft.investor_type.as_ref().map(|code| code.0.as_str()) == Some("CP")
                    && draft.locality == Some(Locality::Local)
            })
            .expect("AADI local CP bucket");

        assert_eq!(local_cp.raw_investor_name, "KSEI AGGREGATE LOCAL CP");
        assert_eq!(local_cp.total_shares, 5_035_745_466);
        assert_eq!(local_cp.percentage_bps, 6467);
    }

    #[test]
    fn balancepos_cross_check_contains_pdf_holder_bucket() {
        let pdf_rows = parse_stext_xml(include_str!(
            "../../tests/fixtures/ksei_above1_stext_excerpt.xml"
        ))
        .expect("pdf fixture rows");
        let pdf_drafts: Vec<_> = pdf_rows
            .iter()
            .map(normalize_ksei_row)
            .collect::<Result<_, _>>()
            .expect("normalized pdf drafts");
        let archive_drafts = parse_balancepos_text(include_str!(
            "../../tests/fixtures/ksei_balancepos_20260227_excerpt.txt"
        ))
        .expect("archive drafts");

        let pdf_local_cp = pdf_drafts
            .iter()
            .find(|draft| {
                draft.ticker_code == "AADI"
                    && draft.investor_type.as_ref().map(|code| code.0.as_str()) == Some("CP")
                    && draft.locality == Some(Locality::Local)
            })
            .expect("pdf local cp");
        let archive_local_cp = archive_drafts
            .iter()
            .find(|draft| {
                draft.ticker_code == "AADI"
                    && draft.investor_type.as_ref().map(|code| code.0.as_str()) == Some("CP")
                    && draft.locality == Some(Locality::Local)
            })
            .expect("archive local cp");

        assert_eq!(pdf_local_cp.report_date, archive_local_cp.report_date);
        assert!(pdf_local_cp.total_shares <= archive_local_cp.total_shares);
        assert!(pdf_local_cp.percentage_bps <= archive_local_cp.percentage_bps);
    }
}
