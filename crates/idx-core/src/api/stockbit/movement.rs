//! Shareholder movements from `GET /insider/company/majorholder`: reported
//! changes in a holder's stake, newest first, 50 rows a page.
//!
//! Two sources share the feed: IDX insider disclosures (directors,
//! commissioners and other insiders, any size; a trade price) and KSEI's
//! daily changes of holders at or above 5% (no price; Stockbit estimates
//! one). Each row (`data.movement[]`) carries `id` (the holder's id, not the
//! row's), `name`, `symbol`, `date` (`"25 Mar 26"`), `previous` / `current` /
//! `changes` (`value`: shares as `"+147,933"`, `percentage`: `"+0.0001"`),
//! `action_type` (`ACTION_TYPE_BUY`, ..._SELL, ..._TRANSFER, ..._CROSS,
//! ..._RIGHT_ISSUE, ..._PRIVATE_PLACEMENT, ..._TENDER_OFFER, ...),
//! `nationality`, `badges`, `price.raw.value`, `value.raw.value`,
//! `is_estimated_price`, `purpose` and `data_source.type`.
//!
//! Query parameters (all optional): `symbol`, `date_start` / `date_end`
//! (YYYY-MM-DD), `insider` (holder id), `action_type`, `source_type`, `page`
//! (from 1).

use chrono::NaiveDate;
use serde_json::Value;

use crate::error::IdxError;
use crate::movements::types::{
    ActionFilter, MovementAction, MovementPage, MovementQuery, MovementSource, ShareholderMovement,
};

pub const PATH: &str = "/insider/company/majorholder";

/// Stockbit's `action_type` value for `filter`.
pub fn action_type(filter: ActionFilter) -> &'static str {
    match filter {
        ActionFilter::Buy => "ACTION_TYPE_BUY",
        ActionFilter::Sell => "ACTION_TYPE_SELL",
        ActionFilter::Transfer => "ACTION_TYPE_TRANSFER",
        ActionFilter::Cross => "ACTION_TYPE_CROSS",
        ActionFilter::CorporateAction => "ACTION_TYPE_CORPACTION",
    }
}

/// Stockbit's `source_type` value for `source`.
pub fn source_type(source: MovementSource) -> &'static str {
    match source {
        MovementSource::Idx => "SOURCE_TYPE_IDX",
        MovementSource::Ksei => "SOURCE_TYPE_KSEI",
    }
}

/// The request path for `page` of `query`; `code` is the bare IDX code.
/// Every value is a date, a digit id, a code or a fixed enum name, so
/// nothing needs escaping.
pub fn request_path(query: &MovementQuery, code: Option<&str>, page: u32) -> String {
    let mut path = format!("{PATH}?page={page}");
    if let Some(code) = code {
        path.push_str(&format!("&symbol={code}"));
    }
    if let Some(from) = query.from {
        path.push_str(&format!("&date_start={from}"));
    }
    if let Some(to) = query.to {
        path.push_str(&format!("&date_end={to}"));
    }
    if let Some(id) = &query.holder_id {
        path.push_str(&format!("&insider={id}"));
    }
    if let Some(action) = query.action {
        path.push_str(&format!("&action_type={}", action_type(action)));
    }
    if let Some(source) = query.source {
        path.push_str(&format!("&source_type={}", source_type(source)));
    }
    path
}

/// One page of the feed. A row without a symbol, holder or readable date is
/// skipped; a body without `data` is a parse error.
pub fn parse_movements(body: &str) -> Result<MovementPage, IdxError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| IdxError::ParseError(format!("Stockbit majorholder: {e}")))?;
    let data = value
        .get("data")
        .filter(|data| data.is_object())
        .ok_or_else(|| IdxError::ParseError("Stockbit majorholder: no data".to_string()))?;
    let movements = data
        .get("movement")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(parse_row).collect())
        .unwrap_or_default();
    Ok(MovementPage {
        movements,
        more: data
            .get("is_more")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn parse_row(row: &Value) -> Option<ShareholderMovement> {
    let symbol = text(row.get("symbol"))?.to_ascii_uppercase();
    let holder = text(row.get("name"))?.to_string();
    let date = NaiveDate::parse_from_str(text(row.get("date"))?, "%d %b %y").ok()?;
    let shares = |key: &str| signed(row.get(key).and_then(|v| v.get("value"))).unwrap_or(0);
    let bps = |key: &str| {
        percent(row.get(key).and_then(|v| v.get("percentage")))
            .map(|p| (p * 100.0).round() as i64)
            .unwrap_or(0)
    };
    let raw = |key: &str| {
        signed(
            row.get(key)
                .and_then(|v| v.get("raw"))
                .and_then(|v| v.get("value")),
        )
        .filter(|n| *n != 0)
    };
    Some(ShareholderMovement {
        symbol,
        date,
        holder,
        holder_id: text(row.get("id")).unwrap_or_default().to_string(),
        action: action(text(row.get("action_type")).unwrap_or_default()),
        source: match text(row.get("data_source").and_then(|v| v.get("type"))) {
            Some("SOURCE_TYPE_IDX") => Some(MovementSource::Idx),
            Some("SOURCE_TYPE_KSEI") => Some(MovementSource::Ksei),
            _ => None,
        },
        locality: match text(row.get("nationality")) {
            Some("NATIONALITY_TYPE_LOCAL") => Some("Local".to_string()),
            Some("NATIONALITY_TYPE_FOREIGN") => Some("Foreign".to_string()),
            _ => None,
        },
        roles: row
            .get("badges")
            .and_then(Value::as_array)
            .map(|badges| {
                badges
                    .iter()
                    .filter_map(|b| text(Some(b)))
                    .map(role)
                    .collect()
            })
            .unwrap_or_default(),
        shares_before: shares("previous"),
        shares_after: shares("current"),
        shares_change: shares("changes"),
        percentage_before_bps: bps("previous"),
        percentage_after_bps: bps("current"),
        percentage_change_bps: bps("changes"),
        price: raw("price"),
        price_estimated: row
            .get("is_estimated_price")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        value: raw("value"),
        purpose: text(row.get("purpose"))
            .filter(|p| *p != "-")
            .map(str::to_string),
    })
}

fn action(raw: &str) -> MovementAction {
    match raw.strip_prefix("ACTION_TYPE_").unwrap_or(raw) {
        "BUY" => MovementAction::Buy,
        "SELL" => MovementAction::Sell,
        "TRANSFER" => MovementAction::Transfer,
        "CROSS" => MovementAction::Cross,
        "RIGHT_ISSUE" => MovementAction::RightsIssue,
        "PRIVATE_PLACEMENT" => MovementAction::PrivatePlacement,
        "TENDER_OFFER" => MovementAction::TenderOffer,
        "BONUS_STOCK" => MovementAction::BonusStock,
        "DIVIDEND_STOCK" => MovementAction::StockDividend,
        "DIVIDEND_CASH" => MovementAction::CashDividend,
        _ => MovementAction::Other,
    }
}

/// `SHAREHOLDER_BADGE_DIREKTUR` -> `director`; an unknown badge keeps its
/// own name, lowercased.
fn role(badge: &str) -> String {
    match badge.strip_prefix("SHAREHOLDER_BADGE_").unwrap_or(badge) {
        "DIREKTUR" => "director".to_string(),
        "KOMISARIS" => "commissioner".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn text(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// `"+147,933"`, `"-10,000,000"`, `"6982"` or a JSON number.
fn signed(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.round() as i64)),
        Value::String(s) => s.trim().replace(',', "").parse().ok(),
        _ => None,
    }
}

/// `"+0.0001"`, `"-0.16"`, `"54.94"`.
fn percent(value: Option<&Value>) -> Option<f64> {
    let n = match value? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().replace(',', "").parse().ok()?,
        _ => return None,
    };
    n.is_finite().then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/fixtures/stockbit_exodus_{name}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    #[test]
    fn insider_buy_reads_shares_price_and_roles() {
        let page = parse_movements(&fixture("majorholder_bbca")).unwrap();
        assert!(page.more);
        let row = page
            .movements
            .iter()
            .find(|m| m.holder == "HENDRA TANUMIHARDJA")
            .expect("BBCA director row");
        assert_eq!(row.symbol, "BBCA");
        assert_eq!(row.date, NaiveDate::from_ymd_opt(2026, 3, 25).unwrap());
        assert_eq!(row.action, MovementAction::Buy);
        assert_eq!(row.source, Some(MovementSource::Idx));
        assert_eq!(row.roles, ["director"]);
        assert_eq!(
            (row.shares_before, row.shares_after, row.shares_change),
            (193_206, 341_139, 147_933)
        );
        // 0.0003% of BBCA rounds to 0 basis points; the shares stay exact.
        assert_eq!(row.percentage_after_bps, 0);
        assert_eq!(row.price, Some(6982));
        assert_eq!(row.value, Some(1_032_868_206));
        assert!(!row.price_estimated);
        assert_eq!(row.locality.as_deref(), Some("Local"));
    }

    #[test]
    fn ksei_sale_is_negative_with_an_estimated_price() {
        let page = parse_movements(&fixture("majorholder")).unwrap();
        let row = page
            .movements
            .iter()
            .find(|m| m.holder == "RUDI R. SUTANTRA")
            .expect("ATLA KSEI sale");
        assert_eq!(row.action, MovementAction::Sell);
        assert_eq!(row.source, Some(MovementSource::Ksei));
        assert_eq!(row.holder_id, "1000004481");
        assert_eq!(row.shares_change, -10_000_000);
        assert_eq!(row.percentage_before_bps, 4081);
        assert_eq!(row.percentage_change_bps, -16);
        assert!(row.price_estimated);
        assert!(row.roles.is_empty());
        assert_eq!(row.purpose, None);
    }

    #[test]
    fn unknown_labels_are_kept_not_dropped() {
        let body = r#"{"data":{"is_more":false,"movement":[
            {"id":"7","name":"X","symbol":"abcd","date":"02 Nov 26",
             "action_type":"ACTION_TYPE_SOMETHING_NEW","badges":["SHAREHOLDER_BADGE_CEO"],
             "data_source":{"type":"SOURCE_TYPE_NEW"},"purpose":"-",
             "previous":{"value":"0","percentage":"0"},
             "current":{"value":"1,000","percentage":"0.01"},
             "changes":{"value":"+1,000","percentage":"+0.01"},
             "price":{"raw":{"value":"0"}},"value":{"raw":{"value":"0"}}},
            {"id":"8","name":"","symbol":"ABCD","date":"02 Nov 26"},
            {"id":"9","name":"Y","symbol":"ABCD","date":"2026-11-02"}
        ]}}"#;
        let page = parse_movements(body).unwrap();
        assert!(!page.more);
        assert_eq!(page.movements.len(), 1);
        let row = &page.movements[0];
        assert_eq!(row.symbol, "ABCD");
        assert_eq!(row.action, MovementAction::Other);
        assert_eq!(row.source, None);
        assert_eq!(row.roles, ["ceo"]);
        assert_eq!(
            (row.price, row.value, row.purpose.as_deref()),
            (None, None, None)
        );
        assert_eq!(row.percentage_after_bps, 1);
    }

    #[test]
    fn empty_and_broken_bodies() {
        let empty = parse_movements(r#"{"data":{"is_more":false,"movement":null}}"#).unwrap();
        assert_eq!(empty, MovementPage::default());
        assert!(parse_movements(r#"{"message":"unauthorized"}"#).is_err());
        assert!(parse_movements("<html>").is_err());
    }

    #[test]
    fn request_path_carries_every_filter() {
        let mut query = MovementQuery::from_args(
            None,
            Some("2026-10-01"),
            Some("2026-10-15"),
            Some("14074"),
            1,
        )
        .unwrap();
        query.action = Some(ActionFilter::CorporateAction);
        query.source = Some(MovementSource::Ksei);
        assert_eq!(
            request_path(&query, Some("BBCA"), 2),
            "/insider/company/majorholder?page=2&symbol=BBCA&date_start=2026-10-01\
             &date_end=2026-10-15&insider=14074&action_type=ACTION_TYPE_CORPACTION\
             &source_type=SOURCE_TYPE_KSEI"
        );
        assert_eq!(
            request_path(&MovementQuery::default(), None, 1),
            "/insider/company/majorholder?page=1"
        );
    }
}
