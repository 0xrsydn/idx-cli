//! Corporate actions from Stockbit's session API:
//! `GET /corpaction/{type}?symbol={CODE}` (one stock's history) or without
//! `symbol` (the market, about a year back and the announced ones ahead; the
//! `limit` the web app sends is not honoured by every type).
//!
//! Each type names its fields with its own prefix (`dividend_`,
//! `rightissue_`, `wrant_`, `rups_`, `puexp_`, `tender_`, ...), bonus rows
//! reuse `stocksplit_*`, and the list sits under a key that is not always the
//! path's (`tenderoffer` → `tender`, `reversesplit` → `stock_reverse`). So
//! the entitlement kinds are read by field suffix (`_cumdate`, `_exdate`,
//! ...), which also covers the two kinds no row was recorded for yet
//! (`stock_dividend`, `reversesplit`, both empty on 2026-11-03). Placeholders
//! (`""`, `0`, `0001-01-01 00:00:00`) read as absent; a row without a symbol
//! or a date is dropped.

use chrono::{Datelike, NaiveDate};
use serde_json::{Map, Value};

use crate::corporate::types::{ActionKind, CorporateAction, DateRange, Ratio};
use crate::error::IdxError;

/// Stockbit's path segment for `kind`.
pub fn path_type(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Dividend => "dividend",
        ActionKind::StockDividend => "stock_dividend",
        ActionKind::Bonus => "bonus",
        ActionKind::Rights => "rightissue",
        ActionKind::Warrant => "warrant",
        ActionKind::Split => "stocksplit",
        ActionKind::ReverseSplit => "reversesplit",
        ActionKind::Meeting => "rups",
        ActionKind::PublicExpose => "pubex",
        ActionKind::Ipo => "ipo",
        ActionKind::TenderOffer => "tenderoffer",
    }
}

/// The actions of one `kind` in a `/corpaction/{type}` response, in the
/// response's order.
pub fn parse_corporate_actions(
    kind: ActionKind,
    body: &str,
) -> Result<Vec<CorporateAction>, IdxError> {
    let bad = |what: String| IdxError::ParseError(format!("stockbit corporate actions: {what}"));
    let root: Value = serde_json::from_str(body).map_err(|e| bad(e.to_string()))?;
    let rows = match root.get("data") {
        Some(Value::Object(data)) => match data.values().find_map(Value::as_array) {
            Some(rows) => rows.as_slice(),
            None if data.values().all(Value::is_null) => &[],
            None => return Err(bad("no list in data".to_string())),
        },
        Some(Value::Array(rows)) => rows.as_slice(),
        Some(Value::Null) => &[],
        _ => return Err(bad("no data".to_string())),
    };
    Ok(rows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|row| parse_row(kind, row))
        .collect())
}

fn parse_row(kind: ActionKind, row: &Map<String, Value>) -> Option<CorporateAction> {
    let code = text(row.get("company_symbol"))?.to_ascii_uppercase();
    if !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let symbol = format!("{code}.JK");
    let mut action = if kind.is_entitlement() {
        entitlement(kind, symbol, row)?
    } else {
        event(kind, symbol, row)?
    };
    if matches!(kind, ActionKind::Ipo | ActionKind::TenderOffer) {
        action.name = text(row.get("company_name")).map(str::to_string);
    }
    Some(action)
}

/// Dividends, bonuses, rights and splits: cum/ex/record/pay dates, a ratio
/// and an adjustment factor.
fn entitlement(
    kind: ActionKind,
    symbol: String,
    row: &Map<String, Value>,
) -> Option<CorporateAction> {
    let cum = date(suffixed(row, "cumdate"));
    let ex = date(suffixed(row, "exdate"));
    let mut action = CorporateAction::new(symbol, kind, cum.or(ex)?);
    action.cum_date = cum;
    action.ex_date = ex;
    action.record_date = date(suffixed(row, "recdate"));
    action.pay_date = date(suffixed(row, "paydate")).or_else(|| date(suffixed(row, "paymentdate")));
    action.ratio = ratio(row);
    action.factor = number(suffixed(row, "factor"));
    match kind {
        ActionKind::Dividend => {
            action.cash_per_share = number(suffixed(row, "value")).map(|v| round_to(v, 2));
        }
        ActionKind::Rights => {
            action.price = whole(suffixed(row, "price"));
            action.period = range(suffixed(row, "trading_start"), suffixed(row, "trading_end"));
        }
        _ => {}
    }
    Some(action)
}

/// Warrants, meetings, exposes, IPOs and tender offers.
fn event(kind: ActionKind, symbol: String, row: &Map<String, Value>) -> Option<CorporateAction> {
    let get = |key: &str| row.get(key);
    let mut action;
    match kind {
        ActionKind::Warrant => {
            let trading = range(get("wrant_trading_from"), get("wrant_trading_end"));
            let exercise = range(get("wrant_exc_from"), get("wrant_exc_end"));
            let first = date(get("wrant_trading_from")).or(exercise.map(|r| r.start))?;
            action = CorporateAction::new(symbol, kind, first);
            action.period = trading;
            action.exercise = exercise;
            action.price = whole(get("wrant_exc_price"));
        }
        ActionKind::Meeting | ActionKind::PublicExpose => {
            let prefix = if kind == ActionKind::Meeting {
                "rups"
            } else {
                "puexp"
            };
            let field = |name: &str| row.get(&format!("{prefix}_{name}"));
            action = CorporateAction::new(symbol, kind, date(field("date"))?);
            action.time = text(field("time")).map(clock);
            action.venue = text(field("venue")).map(squeeze);
            if kind == ActionKind::Meeting {
                action.record_date = date(field("eligible_date"));
            }
        }
        ActionKind::Ipo => {
            action = CorporateAction::new(symbol, kind, date(get("ipo_listing_date"))?);
            action.price = whole(row.get("ipo_price").and_then(|p| p.get("final")));
            let detail = row.get("ipo_data_detail");
            let detail = |key: &str| detail.and_then(|d| d.get(key));
            action.shares = count(detail("shares"));
            action.period = range(detail("offering_start"), detail("offering_end"));
        }
        ActionKind::TenderOffer => {
            let period = range(get("tender_start"), get("tender_end"));
            let first = date(get("tender_start")).or(date(get("tender_end")))?;
            action = CorporateAction::new(symbol, kind, first);
            action.period = period;
            action.pay_date = date(get("tender_paydate"));
            action.price = whole(get("tender_price"));
            action.shares = count(get("tender_shares"));
            action.percentage_bps =
                number(get("tender_percentage")).map(|p| (p * 100.0).round() as i64);
        }
        _ => return None,
    }
    Some(action)
}

/// The value of the one field named `<prefix>_<suffix>`; the shortest name
/// wins should several end so.
fn suffixed<'a>(row: &'a Map<String, Value>, suffix: &str) -> Option<&'a Value> {
    let tail = format!("_{suffix}");
    row.iter()
        .filter(|(key, _)| key.ends_with(&tail))
        .min_by_key(|(key, _)| key.len())
        .map(|(_, value)| value)
}

/// `old`/`new` fields, else the `"6 : 4"` text.
fn ratio(row: &Map<String, Value>) -> Option<Ratio> {
    let pair = |old: Option<u64>, new: Option<u64>| {
        Some(Ratio {
            old: old?,
            new: new?,
        })
    };
    pair(count(suffixed(row, "old")), count(suffixed(row, "new"))).or_else(|| {
        let text = text(suffixed(row, "ratio"))?;
        let (old, new) = text.split_once(':')?;
        pair(
            old.trim().replace(',', "").parse().ok().filter(|n| *n > 0),
            new.trim().replace(',', "").parse().ok().filter(|n| *n > 0),
        )
    })
}

fn text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// `YYYY-MM-DD`, optionally followed by a time; year 1 is a placeholder.
fn date(value: Option<&Value>) -> Option<NaiveDate> {
    let text = text(value)?;
    let day = text.get(..10)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .ok()
        .filter(|d| d.year() >= 1900)
}

fn range(start: Option<&Value>, end: Option<&Value>) -> Option<DateRange> {
    let (start, end) = (date(start)?, date(end)?);
    (start <= end).then_some(DateRange { start, end })
}

/// A positive number from a JSON number or a display string (`"1,191.09"`,
/// `"Rp 25"`); zero is a placeholder.
fn number(value: Option<&Value>) -> Option<f64> {
    let n = match value? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => {
            let cleaned: String = s
                .trim()
                .trim_start_matches("Rp")
                .chars()
                .filter(|c| *c != ',' && !c.is_whitespace())
                .collect();
            cleaned.parse().ok()?
        }
        _ => return None,
    };
    (n.is_finite() && n > 0.0).then_some(n)
}

fn whole(value: Option<&Value>) -> Option<i64> {
    number(value).map(|n| n.round() as i64)
}

fn count(value: Option<&Value>) -> Option<u64> {
    number(value).map(|n| n.round() as u64)
}

fn round_to(value: f64, places: i32) -> f64 {
    let scale = 10f64.powi(places);
    (value * scale).round() / scale
}

/// `13:00:00` → `13:00`; anything else as given.
fn clock(raw: &str) -> String {
    match raw.split(':').collect::<Vec<_>>().as_slice() {
        [h, m, s] if s.len() == 2 && h.len() <= 2 && m.len() == 2 => format!("{h}:{m}"),
        _ => raw.to_string(),
    }
}

fn squeeze(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/stockbit_exodus_corpaction_{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn find<'a>(actions: &'a [CorporateAction], symbol: &str) -> &'a CorporateAction {
        actions.iter().find(|a| a.symbol == symbol).unwrap()
    }

    #[test]
    fn test_parse_dividends_keep_cents_and_dates() {
        let bbca =
            parse_corporate_actions(ActionKind::Dividend, &fixture("dividend_bbca")).unwrap();
        assert_eq!(bbca.len(), 37);
        let latest = &bbca[0];
        assert_eq!(latest.symbol, "BBCA.JK");
        assert_eq!(latest.date, day("2026-08-28"));
        assert_eq!(
            (latest.ex_date, latest.record_date, latest.pay_date),
            (
                Some(day("2026-08-31")),
                Some(day("2026-09-01")),
                Some(day("2026-09-16"))
            )
        );
        assert_eq!(latest.cash_per_share, Some(25.0));
        assert_eq!(latest.ratio, None);

        let market = parse_corporate_actions(ActionKind::Dividend, &fixture("dividend")).unwrap();
        assert_eq!(find(&market, "ITMG.JK").cash_per_share, Some(1191.09));
    }

    #[test]
    fn test_parse_bonus_reads_split_named_fields() {
        let bonus = parse_corporate_actions(ActionKind::Bonus, &fixture("bonus")).unwrap();
        let ppgl = find(&bonus, "PPGL.JK");
        assert_eq!(ppgl.date, day("2026-11-02"));
        assert_eq!(ppgl.ex_date, Some(day("2026-11-03")));
        assert_eq!(ppgl.pay_date, Some(day("2026-11-25")));
        assert_eq!(ppgl.ratio, Some(Ratio { old: 6, new: 4 }));
        assert_eq!(ppgl.factor, Some(1.6667));
    }

    #[test]
    fn test_parse_rights_and_split() {
        let rights = parse_corporate_actions(ActionKind::Rights, &fixture("rightissue")).unwrap();
        let bmas = find(&rights, "BMAS.JK");
        assert_eq!(bmas.date, day("2026-11-23"));
        assert_eq!(bmas.ratio, Some(Ratio { old: 655, new: 104 }));
        assert_eq!(bmas.price, Some(350));
        assert_eq!(
            bmas.period,
            Some(DateRange {
                start: day("2026-11-27"),
                end: day("2026-12-03")
            })
        );
        let split =
            parse_corporate_actions(ActionKind::Split, &fixture("stocksplit_bbca")).unwrap();
        assert_eq!(split[0].date, day("2021-10-12"));
        assert_eq!(split[0].ratio, Some(Ratio { old: 1, new: 5 }));
        assert_eq!(split[0].factor, Some(5.0));
    }

    #[test]
    fn test_parse_events() {
        let warrants = parse_corporate_actions(ActionKind::Warrant, &fixture("warrant")).unwrap();
        let brna = find(&warrants, "BRNA.JK");
        assert_eq!(brna.date, day("2026-10-16"));
        assert_eq!(brna.price, Some(800));
        assert_eq!(brna.exercise.map(|r| r.end), Some(day("2029-10-15")));

        let meetings = parse_corporate_actions(ActionKind::Meeting, &fixture("rups")).unwrap();
        let arti = find(&meetings, "ARTI.JK");
        assert_eq!(arti.date, day("2026-12-09"));
        assert_eq!(arti.time.as_deref(), Some("10:30"));
        assert_eq!(arti.record_date, Some(day("2026-11-12")));

        let exposes = parse_corporate_actions(ActionKind::PublicExpose, &fixture("pubex")).unwrap();
        assert_eq!(find(&exposes, "MEJA.JK").time.as_deref(), Some("13:00"));

        let tenders =
            parse_corporate_actions(ActionKind::TenderOffer, &fixture("tenderoffer")).unwrap();
        let auto = find(&tenders, "AUTO.JK");
        assert_eq!(auto.date, day("2026-10-13"));
        assert_eq!(auto.price, Some(3600));
        assert_eq!(auto.shares, Some(238_442_293));
        assert_eq!(auto.percentage_bps, Some(495));
        assert_eq!(auto.pay_date, Some(day("2026-11-18")));
        assert_eq!(auto.name.as_deref(), Some("Astra Otoparts Tbk."));
    }

    #[test]
    fn test_parse_ipo_drops_placeholders() {
        let ipos = parse_corporate_actions(ActionKind::Ipo, &fixture("ipo")).unwrap();
        let rans = find(&ipos, "RANS.JK");
        assert_eq!(rans.price, Some(170));
        assert_eq!(rans.shares, Some(2_525_000_000));
        assert_eq!(rans.period.map(|r| r.start), Some(day("2026-07-02")));
        // An ETF listed without offer terms: no price, no period.
        let xkgs = find(&ipos, "XKGS.JK");
        assert_eq!((xkgs.price, xkgs.period, xkgs.shares), (None, None, None));
    }

    #[test]
    fn test_parse_empty_lists_and_odd_keys() {
        for (kind, body) in [
            (
                ActionKind::StockDividend,
                r#"{"data":{"stock_dividend":[]}}"#,
            ),
            (ActionKind::ReverseSplit, r#"{"data":{"stock_reverse":[]}}"#),
            (ActionKind::Dividend, r#"{"data":null}"#),
        ] {
            assert!(parse_corporate_actions(kind, body).unwrap().is_empty());
        }
        assert!(parse_corporate_actions(ActionKind::Dividend, r#"{"message":"x"}"#).is_err());
        // A reverse split named like a split, read by suffix.
        let body = r#"{"data":{"stock_reverse":[{"company_symbol":"ABCD",
            "reverse_cumdate":"2026-12-01","reverse_exdate":"2026-12-02",
            "reverse_old":"10","reverse_new":"1","reverse_factor":"0.1"}]}}"#;
        let rows = parse_corporate_actions(ActionKind::ReverseSplit, body).unwrap();
        assert_eq!(rows[0].ratio, Some(Ratio { old: 10, new: 1 }));
        assert_eq!(rows[0].factor, Some(0.1));
    }

    #[test]
    fn test_parse_numbers_and_placeholders() {
        assert_eq!(number(Some(&Value::from("1,191.09"))), Some(1191.09));
        assert_eq!(number(Some(&Value::from("Rp 25"))), Some(25.0));
        assert_eq!(number(Some(&Value::from(0))), None);
        assert_eq!(date(Some(&Value::from("0001-01-01 00:00:00"))), None);
        assert_eq!(
            date(Some(&Value::from("2026-07-02 00:00:00"))),
            Some(day("2026-07-02"))
        );
        assert_eq!(clock("09:00:00"), "09:00");
        assert_eq!(clock("10:30"), "10:30");
    }
}
