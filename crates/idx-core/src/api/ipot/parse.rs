//! Pure parsers for IPOT's broker summary fragment, tested against the
//! recorded pages in `tests/fixtures/ipot_brokersummary_*.html`.
//!
//! The fragment is a 9-column table: buyer code, lots, value, average, rank,
//! seller code, lots, value, average. Each code sits in a
//! `<span class="text-local|text-foreign|text-bumn">`; the footer holds
//! `T. Val`, `F. NVal`, `T.Lot` and `Avg`, and the date inputs echo the
//! range the source applied. The page is parsed with html5ever, so markup
//! reads as a browser builds it.

use chrono::NaiveDate;
use scraper::{ElementRef, Html, Node, Selector};

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

fn broker_kind(class: Option<&str>) -> Option<BrokerKind> {
    match class?.strip_prefix("text-")? {
        "local" => Some(BrokerKind::Local),
        "foreign" => Some(BrokerKind::Foreign),
        "bumn" => Some(BrokerKind::StateOwned),
        _ => None,
    }
}

/// Text with `&nbsp;` as a space, trimmed.
fn clean(text: &str) -> String {
    text.replace('\u{a0}', " ").trim().to_string()
}

fn selector(css: &str) -> Selector {
    Selector::parse(css).expect("a valid selector")
}

/// The one element matching `css` within `scope`; `None` for none or more.
fn only<'a>(scope: ElementRef<'a>, css: &str) -> Option<ElementRef<'a>> {
    let selector = selector(css);
    let mut found = scope.select(&selector);
    match (found.next(), found.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

fn parse_error(what: &str) -> IdxError {
    IdxError::ParseError(format!("ipot broker summary: {what}"))
}

/// Whether `element` has only attributes named in `allowed`.
fn only_attrs(element: ElementRef, allowed: &[&str]) -> bool {
    element
        .value()
        .attrs()
        .all(|(name, _)| allowed.contains(&name))
}

/// The text of an element whose children are text (and comments) only;
/// `None` when it holds other elements.
fn leaf_text(element: ElementRef) -> Option<String> {
    let mut text = String::new();
    for child in element.children() {
        match child.value() {
            Node::Text(part) => text.push_str(part),
            Node::Comment(_) => {}
            _ => return None,
        }
    }
    Some(text)
}

/// A body cell as IPOT writes it: `<td align>` holding text, or one
/// `<span class="text-...">` holding text. Its text and the span's class;
/// `None` for any other shape, which could show other text than it holds.
fn cell(td: ElementRef) -> Option<(String, Option<String>)> {
    if !only_attrs(td, &["align"]) {
        return None;
    }
    let mut text = String::new();
    let mut class = None;
    for child in td.children() {
        match child.value() {
            Node::Text(part) => text.push_str(part),
            Node::Comment(_) => {}
            Node::Element(element) if element.name() == "span" && class.is_none() => {
                let span = ElementRef::wrap(child)?;
                let value = span.attr("class").filter(|value| {
                    value.starts_with("text-") && value.split_whitespace().count() == 1
                })?;
                if !only_attrs(span, &["class"]) {
                    return None;
                }
                text.push_str(&leaf_text(span)?);
                class = Some(value.to_string());
            }
            _ => return None,
        }
    }
    Some((clean(&text), class))
}

/// Whether `element` sits in plain divs only, as IPOT's table and date
/// inputs do; inside anything else (details, dialog, noscript, object,
/// select, ...) it may not show.
fn in_divs(element: ElementRef) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .all(|ancestor| matches!(ancestor.value().name(), "div" | "html"))
}

/// The `MM/DD/YYYY` value of the one input with this id, in plain divs.
fn input_date(page: &Html, id: &str) -> Option<NaiveDate> {
    let input =
        only(page.root_element(), &format!("input#{id}")).filter(|input| in_divs(*input))?;
    NaiveDate::parse_from_str(input.attr("value")?.trim(), "%m/%d/%Y").ok()
}

/// Whether anything in the page is hidden by markup alone (`hidden`, an
/// inline `display`/`visibility` style) or inert (`template`). Styling from
/// the site's own stylesheet cannot be seen here.
fn hides_content(page: &Html) -> bool {
    page.root_element().descendent_elements().any(|element| {
        let style = element
            .attr("style")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .replace(' ', "");
        element.value().name() == "template"
            || element.attr("hidden").is_some()
            || style.contains("display:none")
            || style.contains("visibility:hidden")
    })
}

/// The page is read as a browser builds it from the fragment (html5ever),
/// so markup it would not show as table cells or footer text, such as
/// comments, attributes or other elements' content, is never read as such.
pub fn parse_broker_summary(html: &str) -> Result<IpotPage, IdxError> {
    let page = Html::parse_fragment(html);
    let shown: String = page.root_element().text().collect();
    if shown.to_ascii_lowercase().contains("invalid stock code") {
        return Ok(IpotPage::Invalid);
    }
    if hides_content(&page) {
        return Err(parse_error("hidden or inert markup in the page"));
    }
    let table = only(page.root_element(), "table")
        .ok_or_else(|| parse_error("no single table in the page"))?;
    if !in_divs(table) {
        return Err(parse_error("the table is not in plain page markup"));
    }
    let body = only(table, "tbody").ok_or_else(|| parse_error("no single table body"))?;
    let mut raw = BrokerSummaryRaw {
        from: input_date(&page, "startDateBrokerSummary"),
        to: input_date(&page, "endDateBrokerSummary"),
        ..BrokerSummaryRaw::default()
    };
    let mut rounded = false;
    let mut figure = |text: &str| {
        parse_figure(text).map(|(value, abbreviated)| {
            rounded |= abbreviated;
            value
        })
    };
    // Rows must be whole and ranked in order: a skipped or merged row would
    // shift brokers between ranks. A broker after a side's blank rows
    // rejects the page too.
    let (mut buyers_ended, mut sellers_ended) = (false, false);
    for (index, row) in body.child_elements().enumerate() {
        let rank = index as u32 + 1;
        let tds: Vec<ElementRef> = row.child_elements().collect();
        let cells: Option<Vec<(String, Option<String>)>> =
            if row.value().name() == "tr" && !row.value().attrs().any(|_| true) && tds.len() == 9 {
                tds.iter()
                    .map(|td| (td.value().name() == "td").then(|| cell(*td)).flatten())
                    .collect()
            } else {
                None
            };
        let Some(cells) = cells.filter(|cells| cells[4].0.parse::<u32>().ok() == Some(rank)) else {
            return Err(parse_error(&format!("unexpected row {rank}")));
        };
        for (side, at, ended) in [
            (&mut raw.buyers, 0, &mut buyers_ended),
            (&mut raw.sellers, 5, &mut sellers_ended),
        ] {
            let code = cells[at].0.to_ascii_uppercase();
            if code.is_empty() {
                *ended = true;
                continue;
            }
            if *ended {
                return Err(parse_error(&format!("broker {code:?} after a blank row")));
            }
            if code.len() != 2 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(parse_error(&format!("unexpected broker code {code:?}")));
            }
            side.push(BrokerFlow {
                rank,
                code,
                name: None,
                kind: broker_kind(cells[at].1.as_deref()),
                lots: figure(&cells[at + 1].0),
                value: figure(&cells[at + 2].0),
                average_price: figure(&cells[at + 3].0),
            });
        }
    }
    // The footer's figures, e.g. `T. Val : 6.6 T`, each in its own span
    // holding text only, each label once.
    let mut footer = Vec::new();
    for span in table.select(&selector("tfoot span")) {
        let text = leaf_text(span)
            .filter(|_| only_attrs(span, &["class"]))
            .ok_or_else(|| parse_error("unexpected footer markup"))?;
        footer.push(clean(&text));
    }
    for label in ["T. Val :", "F. NVal :", "T.Lot :", "Avg :"] {
        if footer.iter().filter(|text| text.starts_with(label)).count() > 1 {
            return Err(parse_error(&format!("{label} twice in the footer")));
        }
    }
    let mut total = |label: &str| {
        footer
            .iter()
            .find_map(|text| text.strip_prefix(label))
            .and_then(&mut figure)
    };
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

    /// The BBCA October 16 page with `edit` applied around its rows: `rows`
    /// holds where each `</tr>` of the body starts.
    fn mutated(edit: impl Fn(&str, &[usize]) -> String) -> String {
        let body = DAY.find("<tbody>").unwrap();
        let rows: Vec<usize> = DAY[body..]
            .match_indices("</tr>")
            .map(|(at, _)| body + at)
            .collect();
        edit(DAY, &rows)
    }

    /// Rank 5's row (`<tr>` to `</tr>`), and its cells alone.
    fn rank5(page: &str, rows: &[usize]) -> (usize, usize, usize) {
        let start = page[..rows[4]].rfind("<tr>").unwrap();
        (start, start + "<tr>".len(), rows[4])
    }

    #[test]
    fn test_parse_broker_summary_rejects_rows_that_do_not_line_up() {
        let first_cell = "<td align=\"center\"><span class=\"text-local\">MG</span></td>";
        let lots = "<span class=\"text-up\">828,163 </span>";
        let hide_rank5 = |wrap: fn(&str) -> String| {
            mutated(|page, rows| {
                let (start, cells, end) = rank5(page, rows);
                page[..start].to_string() + &wrap(&page[cells..end]) + &page[end + 5..]
            })
        };
        let pages = [
            ("a missing cell", DAY.replacen(first_cell, "", 1)),
            (
                "a rank out of order",
                DAY.replacen(
                    "<td align=\"center\">1</td>",
                    "<td align=\"center\">2</td>",
                    1,
                ),
            ),
            ("a broker after a blank row", DAY.replacen(">XL<", "><", 1)),
            (
                "a duplicate rank-5 row",
                mutated(|page, rows| {
                    let (start, _, end) = rank5(page, rows);
                    page[..start].to_string()
                        + &page[start..end + 5].to_uppercase()
                        + &page[start..]
                }),
            ),
            (
                "a table body ended early",
                mutated(|page, rows| {
                    page[..rows[3] + 5].to_string() + "</TbOdY>" + &page[rows[3] + 5..]
                }),
            ),
            (
                "cells in a comment",
                hide_rank5(|cells| format!("<tr><!--{cells}--></tr>")),
            ),
            (
                "cells in an attribute",
                hide_rank5(|cells| format!("<tr data-x='{cells}'></tr>")),
            ),
            (
                "cells after a quoted '>'",
                hide_rank5(|cells| format!("<tr data-x='>{cells}'></tr>")),
            ),
            (
                "a row ended mid-cell",
                DAY.replacen(lots, &format!("</tr >{lots}"), 1),
            ),
            (
                "a cell under another tag",
                DAY.replacen(
                    "<td align=\"center\">5</td>",
                    "<td-x align=\"center\">5</td>",
                    1,
                ),
            ),
            (
                "the table in a comment",
                DAY.replacen("<tbody>", "<!--<tbody>", 1)
                    .replacen("</tbody>", "</tbody>-->", 1),
            ),
            (
                "a table inside the table",
                DAY.replacen("<tbody>", "<table></table><tbody>", 1),
            ),
            (
                "the body in svg",
                DAY.replacen("<tbody>", "<svg><tbody>", 1).replacen(
                    "</tbody>",
                    "</tbody></svg>",
                    1,
                ),
            ),
            (
                "the table in a select",
                DAY.replacen("<table", "<select><table", 1).replacen(
                    "</table>",
                    "</table></select>",
                    1,
                ),
            ),
            (
                "the table in closed details",
                DAY.replacen("<table", "<details><table", 1).replacen(
                    "</table>",
                    "</table></details>",
                    1,
                ),
            ),
            ("no table", "</tbody><tbody>".to_string()),
            // Text a browser would not show, or not as one figure or code.
            (
                "a script inside a figure",
                DAY.replacen(">828,163 <", ">8<script>2</script>28,163 <", 1),
            ),
            (
                "hidden text inside a figure",
                DAY.replacen(">828,163 <", ">8<span hidden>2</span>8,163 <", 1),
            ),
            (
                "a hidden footer total",
                DAY.replacen(
                    "<span class=\"fsize12 ml5 mr5\">T. Val",
                    "<span hidden>T. Val : 1</span><span class=\"fsize12 ml5 mr5\">T. Val",
                    1,
                ),
            ),
            (
                "the table in a template",
                DAY.replacen("<table", "<template><table", 1).replacen(
                    "</table>",
                    "</table></template>",
                    1,
                ),
            ),
            (
                "another span before the code",
                DAY.replacen(
                    "<span class=\"text-local\">MG</span>",
                    "<span class=\"text-foreign\"></span><span class=\"text-local\">MG</span>",
                    1,
                ),
            ),
            (
                "nested footer spans",
                DAY.replacen(
                    "T. Val : 6.6 T",
                    "<span>x<span>T. Val : 6.6 T</span></span>",
                    1,
                ),
            ),
        ];
        for (what, page) in pages {
            assert!(
                matches!(parse_broker_summary(&page), Err(IdxError::ParseError(_))),
                "{what}"
            );
        }
    }

    #[test]
    fn test_parse_broker_summary_reads_what_a_browser_shows() {
        let original = summary(DAY);
        // Markup a browser repairs or decodes to the same table.
        let same = [
            (
                "rank 1 without cell ends",
                mutated(|page, rows| {
                    let start = page[..rows[0]].rfind("<tr>").unwrap();
                    page[..start].to_string()
                        + &page[start..rows[0]].replace("</td>", "")
                        + &page[rows[0]..]
                }),
            ),
            (
                "rank 9 left open, rank 10 without cell ends",
                mutated(|page, rows| {
                    page[..rows[8]].to_string()
                        + &page[rows[8] + 5..rows[9]].replace("</td>", "")
                        + &page[rows[9]..]
                }),
            ),
            (
                "a character reference",
                DAY.replacen(">828,163 <", ">8&#50;8,163 <", 1),
            ),
            (
                "an abrupt comment",
                mutated(|page, rows| {
                    page[..rows[7] + 5].to_string() + "<!-->" + &page[rows[7] + 5..]
                }),
            ),
            (
                "a decoy attribute before the date",
                DAY.replacen(
                    "id=\"startDateBrokerSummary\"",
                    "id=\"startDateBrokerSummary\" data-value=\"01/01/2001\"",
                    1,
                ),
            ),
            (
                "a decoy total outside the footer",
                DAY.replacen(
                    "</tbody>",
                    "</tbody><div data-note=\"T. Val : 1 <x>\"></div>",
                    1,
                ),
            ),
            (
                "a script after the table",
                DAY.replacen("</tbody>", "</tbody><script></script>", 1),
            ),
        ];
        for (what, page) in same {
            assert_eq!(summary(&page), original, "{what}");
        }
        // Markup a browser shows differently: the figure it shows, not ours.
        let angle = summary(&DAY.replacen(">828,163 <", ">8>28,163 <", 1));
        assert_eq!(angle.buyers[4].lots, None);
        let lots = "<span class=\"text-up\">828,163 </span>";
        let early_cell_end = summary(&DAY.replacen(lots, &format!("</td >{lots}"), 1));
        assert_eq!(early_cell_end.buyers[4].lots, None);
        let commented_date =
            summary(&DAY.replacen("value=\"10/16/2026\"", "value=\"10/<!--x-->16/2026\"", 1));
        assert_eq!(commented_date.from, None);
        let second_start = DAY.replacen(
            "</tbody>",
            "</tbody><input id=\"startDateBrokerSummary\" value=\"10/15/2026\">",
            1,
        );
        assert_eq!(
            summary(&second_start).from,
            None,
            "an id twice is no echoed date"
        );
        let closed = DAY.replacen("<input", "<details><input", 1);
        assert_eq!(
            summary(&closed).from,
            None,
            "a date in closed details is not shown"
        );
    }
}
