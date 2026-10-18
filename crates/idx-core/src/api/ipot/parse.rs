//! Pure parsers for IPOT's broker summary fragment, tested against the
//! recorded pages in `tests/fixtures/ipot_brokersummary_*.html`.
//!
//! The fragment is a 9-column table: buyer code, lots, value, average, rank,
//! seller code, lots, value, average. Each code sits in a
//! `<span class="text-local|text-foreign|text-bumn">`; the footer holds
//! `T. Val`, `F. NVal`, `T.Lot` and `Avg`, and the date inputs echo the
//! range the source applied.

use chrono::NaiveDate;

use crate::broker::types::{BrokerFlow, BrokerKind};
use crate::error::IdxError;

/// What one page said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpotPage {
    /// `Invalid stock code!`: the source does not know the ticker.
    Invalid,
    Summary(BrokerSummaryRaw),
}

/// One parsed summary page, before the symbol and query are attached.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrokerSummaryRaw {
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub buyers: Vec<BrokerFlow>,
    pub sellers: Vec<BrokerFlow>,
    pub total_value: Option<i64>,
    pub total_lots: Option<i64>,
    pub average_price: Option<i64>,
    pub foreign_net_value: Option<i64>,
    pub rounded: bool,
}

/// A figure as published: `1.6 M` -> `(1_600_000, true)`, `687,067` ->
/// `(687_067, false)`, `-2.3 T` -> `(-2_300_000_000_000, true)`. Scaled in
/// decimal, so `1.6 M` is exact. Empty or unparsable -> `None`.
pub fn parse_figure(text: &str) -> Option<(i64, bool)> {
    let text = text.trim().replace(',', "");
    let (number, exponent) = match text.chars().last()?.to_ascii_uppercase() {
        'K' => (&text[..text.len() - 1], 3),
        'M' => (&text[..text.len() - 1], 6),
        'B' => (&text[..text.len() - 1], 9),
        'T' => (&text[..text.len() - 1], 12),
        _ => (text.as_str(), 0),
    };
    let number = number.trim();
    let (negative, digits) = match number.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, number),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (exponent > 0 && fraction.len() > exponent as usize)
    {
        return None;
    }
    let scale = 10_i64.checked_pow(exponent)?;
    let mut value = whole.parse::<i64>().ok()?.checked_mul(scale)?;
    if exponent == 0 {
        // A plain decimal: round to whole units.
        if fraction
            .as_bytes()
            .first()
            .is_some_and(|digit| *digit >= b'5')
        {
            value = value.checked_add(1)?;
        }
    } else if !fraction.is_empty() {
        let unit = 10_i64.checked_pow(exponent - fraction.len() as u32)?;
        value = value.checked_add(fraction.parse::<i64>().ok()?.checked_mul(unit)?)?;
    }
    Some((if negative { -value } else { value }, exponent > 0))
}

struct Cell {
    class: Option<String>,
    text: String,
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").trim().to_string()
}

fn cells(row: &str) -> Vec<Cell> {
    let mut out = Vec::new();
    let mut rest = row;
    while let Some(start) = rest.find("<td") {
        let after = &rest[start..];
        let Some(open_end) = after.find('>') else {
            break;
        };
        let inner_start = open_end + 1;
        let Some(close) = after[inner_start..].find("</td>") else {
            break;
        };
        let inner = &after[inner_start..inner_start + close];
        let class = inner.find("class=\"").and_then(|at| {
            let value = &inner[at + 7..];
            value.find('"').map(|end| value[..end].to_string())
        });
        out.push(Cell {
            class,
            text: strip_tags(inner),
        });
        rest = &after[inner_start + close + 5..];
    }
    out
}

fn broker_kind(class: Option<&str>) -> Option<BrokerKind> {
    match class?.strip_prefix("text-")? {
        "local" => Some(BrokerKind::Local),
        "foreign" => Some(BrokerKind::Foreign),
        "bumn" => Some(BrokerKind::StateOwned),
        _ => None,
    }
}

/// The `MM/DD/YYYY` value of the input with this id.
fn input_date(html: &str, id: &str) -> Option<NaiveDate> {
    let at = html.find(&format!("id=\"{id}\""))?;
    let tag_start = html[..at].rfind('<')?;
    let tag_end = at + html[at..].find('>')?;
    let tag = &html[tag_start..tag_end];
    let value = &tag[tag.find("value=\"")? + 7..];
    NaiveDate::parse_from_str(&value[..value.find('"')?], "%m/%d/%Y").ok()
}

/// Text after `label` up to the next tag, e.g. `6.6 T` for `T. Val :`.
fn footer<'a>(html: &'a str, label: &str) -> Option<&'a str> {
    let rest = &html[html.find(label)? + label.len()..];
    Some(rest[..rest.find('<')?].trim())
}

pub fn parse_broker_summary(html: &str) -> Result<IpotPage, IdxError> {
    if html.to_ascii_lowercase().contains("invalid stock code") {
        return Ok(IpotPage::Invalid);
    }
    let Some((body_start, body_end)) = html.find("<tbody>").and_then(|start| {
        html[start..]
            .find("</tbody>")
            .map(|len| (start, start + len))
    }) else {
        return Err(IdxError::ParseError(
            "ipot broker summary: no table in the page".to_string(),
        ));
    };
    let mut raw = BrokerSummaryRaw {
        from: input_date(html, "startDateBrokerSummary"),
        to: input_date(html, "endDateBrokerSummary"),
        ..BrokerSummaryRaw::default()
    };
    let mut rounded = false;
    let mut figure = |text: &str| {
        parse_figure(text).map(|(value, abbreviated)| {
            rounded |= abbreviated;
            value
        })
    };
    // A skipped row would shift every later broker up a rank, so a row of
    // another shape, a rank out of order, or a broker after a side's blank
    // rows rejects the page.
    let (mut buyers_ended, mut sellers_ended) = (false, false);
    for (index, row) in html[body_start..body_end].split("</tr>").enumerate() {
        let row = cells(row);
        if row.is_empty() {
            continue;
        }
        let rank = index as u32 + 1;
        if row.len() != 9 || row[4].text.parse::<u32>().ok() != Some(rank) {
            return Err(IdxError::ParseError(format!(
                "ipot broker summary: unexpected row {rank} ({} cells, rank {:?})",
                row.len(),
                row.get(4).map(|cell| cell.text.as_str())
            )));
        }
        for (side, at, ended) in [
            (&mut raw.buyers, 0, &mut buyers_ended),
            (&mut raw.sellers, 5, &mut sellers_ended),
        ] {
            let code = row[at].text.to_ascii_uppercase();
            if code.is_empty() {
                *ended = true;
                continue;
            }
            if *ended {
                return Err(IdxError::ParseError(format!(
                    "ipot broker summary: broker {code:?} after a blank row"
                )));
            }
            if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(IdxError::ParseError(format!(
                    "ipot broker summary: unexpected broker code {code:?}"
                )));
            }
            side.push(BrokerFlow {
                rank,
                code,
                name: None,
                kind: broker_kind(row[at].class.as_deref()),
                lots: figure(&row[at + 1].text),
                value: figure(&row[at + 2].text),
                average_price: figure(&row[at + 3].text),
            });
        }
    }
    let mut total = |label: &str| footer(html, label).and_then(&mut figure);
    raw.total_value = total("T. Val :");
    raw.foreign_net_value = total("F. NVal :");
    raw.total_lots = total("T.Lot :");
    // The page prints `Avg : 0` when nothing traded; there is no average then.
    raw.average_price = total("Avg :").filter(|_| raw.total_lots.is_some_and(|lots| lots > 0));
    raw.rounded = rounded;
    Ok(IpotPage::Summary(raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: &str = include_str!("../../../tests/fixtures/ipot_brokersummary_bbca_20261016.html");
    const RANGE: &str =
        include_str!("../../../tests/fixtures/ipot_brokersummary_bbca_20261012_20261016.html");
    const NO_TRADES: &str =
        include_str!("../../../tests/fixtures/ipot_brokersummary_bbca_20261017.html");
    const INVALID: &str =
        include_str!("../../../tests/fixtures/ipot_brokersummary_zzzz_invalid.html");

    fn summary(html: &str) -> BrokerSummaryRaw {
        match parse_broker_summary(html).unwrap() {
            IpotPage::Summary(raw) => raw,
            IpotPage::Invalid => panic!("expected a summary"),
        }
    }

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_parse_figure_scales_in_decimal() {
        assert_eq!(parse_figure("1.6 M"), Some((1_600_000, true)));
        assert_eq!(parse_figure("841.2 B"), Some((841_200_000_000, true)));
        assert_eq!(parse_figure("-2.3 T"), Some((-2_300_000_000_000, true)));
        assert_eq!(parse_figure("12 K"), Some((12_000, true)));
        assert_eq!(parse_figure("687,067 "), Some((687_067, false)));
        assert_eq!(parse_figure("5,191.5"), Some((5_192, false)));
        assert_eq!(parse_figure("0"), Some((0, false)));
        for bad in ["", " ", "n/a", "1.2.3 M", "M", "-", "1.6789 K"] {
            assert_eq!(parse_figure(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn test_parse_broker_summary_one_day() {
        let raw = summary(DAY);
        assert_eq!(
            (raw.from, raw.to),
            (Some(date(2026, 10, 16)), Some(date(2026, 10, 16)))
        );
        assert_eq!((raw.buyers.len(), raw.sellers.len()), (10, 10));

        let top = &raw.buyers[0];
        assert_eq!(
            (top.rank, top.code.as_str(), top.kind),
            (1, "MG", Some(BrokerKind::Local))
        );
        assert_eq!(
            (top.lots, top.value, top.average_price),
            (Some(1_600_000), Some(841_200_000_000), Some(5_191))
        );
        let seller = &raw.sellers[0];
        assert_eq!(
            (seller.code.as_str(), seller.kind, seller.value),
            ("CC", Some(BrokerKind::StateOwned), Some(2_200_000_000_000))
        );
        assert_eq!(raw.sellers[1].kind, Some(BrokerKind::Foreign));
        let last = &raw.buyers[9];
        assert_eq!((last.rank, last.code.as_str()), (10, "AK"));
        assert_eq!(last.lots, Some(398_474), "small figures are exact");

        assert_eq!(raw.total_value, Some(6_600_000_000_000));
        assert_eq!(raw.foreign_net_value, Some(-2_300_000_000_000));
        assert_eq!(raw.total_lots, Some(12_600_000));
        assert_eq!(raw.average_price, Some(5_228));
        assert!(raw.rounded);
    }

    #[test]
    fn test_parse_broker_summary_range_echoes_its_dates() {
        let raw = summary(RANGE);
        assert_eq!(
            (raw.from, raw.to),
            (Some(date(2026, 10, 12)), Some(date(2026, 10, 16)))
        );
        assert_eq!(raw.buyers.len(), 10);
        assert!(
            raw.total_value
                .is_some_and(|value| value > 6_600_000_000_000)
        );
    }

    #[test]
    fn test_parse_broker_summary_without_trades_is_empty() {
        let raw = summary(NO_TRADES);
        assert!(raw.buyers.is_empty() && raw.sellers.is_empty());
        assert_eq!((raw.total_value, raw.total_lots), (Some(0), Some(0)));
        assert_eq!(raw.average_price, None, "Avg : 0 means no average");
        assert!(!raw.rounded);
        assert_eq!(raw.to, Some(date(2026, 10, 17)));
    }

    #[test]
    fn test_parse_broker_summary_rejects_unknown_codes_and_markup() {
        assert_eq!(parse_broker_summary(INVALID).unwrap(), IpotPage::Invalid);
        assert!(matches!(
            parse_broker_summary("<html>maintenance</html>"),
            Err(IdxError::ParseError(_))
        ));
        let odd = DAY.replacen(">MG<", ">M G<", 1);
        assert!(matches!(
            parse_broker_summary(&odd),
            Err(IdxError::ParseError(_))
        ));
    }

    #[test]
    fn test_parse_broker_summary_rejects_rows_that_would_shift_ranks() {
        let first_cell = "<td align=\"center\"><span class=\"text-local\">MG</span></td>";
        let short_row = DAY.replacen(first_cell, "", 1);
        let skipped_rank = DAY.replacen(
            "<td align=\"center\">1</td>",
            "<td align=\"center\">2</td>",
            1,
        );
        let gap = DAY.replacen(">XL<", "><", 1);
        for page in [short_row, skipped_rank, gap, "</tbody><tbody>".to_string()] {
            assert!(
                matches!(parse_broker_summary(&page), Err(IdxError::ParseError(_))),
                "{}",
                &page[..page.len().min(80)]
            );
        }
    }
}
