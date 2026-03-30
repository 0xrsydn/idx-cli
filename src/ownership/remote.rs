use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::curl_impersonate;
use crate::error::IdxError;

pub const IDX_ANNOUNCEMENT_LISTING_URL: &str = "https://www.idx.co.id/id/berita/pengumuman/";
const DEFAULT_IDX_ANNOUNCEMENT_API_URL: &str =
    "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement";
const IDX_ANNOUNCEMENT_API_ENV: &str = "IDX_OWNERSHIP_ANNOUNCEMENT_API_URL";
const IDX_ANNOUNCEMENT_LISTING_ENV: &str = "IDX_OWNERSHIP_ANNOUNCEMENT_PAGE_URL";
const IDX_ANNOUNCEMENT_PAGE_SIZE: usize = 10;
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Deserialize)]
pub struct AnnouncementPage {
    #[serde(rename = "Items", default)]
    pub items: Vec<AnnouncementItem>,
    #[serde(rename = "ItemCount")]
    pub item_count: Option<usize>,
    #[serde(rename = "PageCount")]
    pub page_count: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnnouncementItem {
    #[serde(rename = "PublishDate")]
    pub publish_date: String,
    #[serde(rename = "Title")]
    pub title: String,
    #[serde(rename = "AnnouncementType")]
    pub announcement_type: Option<String>,
    #[serde(rename = "Code")]
    pub code: Option<String>,
    #[serde(rename = "Attachments", default)]
    pub attachments: Vec<AnnouncementAttachment>,
    #[serde(rename = "PdfPath")]
    pub pdf_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnnouncementAttachment {
    #[serde(rename = "FullSavePath")]
    pub full_save_path: String,
    #[serde(rename = "OriginalFilename")]
    pub original_filename: Option<String>,
    #[serde(rename = "PDFFilename")]
    pub pdf_filename: Option<String>,
    #[serde(rename = "IsAttachment")]
    pub is_attachment: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipReportFamily {
    AboveOnePercent,
    AboveFivePercent,
    InvestorTypeBreakdown,
}

impl OwnershipReportFamily {
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::AboveOnePercent => "above1",
            Self::AboveFivePercent => "above5",
            Self::InvestorTypeBreakdown => "investor-type",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AboveOnePercent => "Above 1%",
            Self::AboveFivePercent => "Above 5%",
            Self::InvestorTypeBreakdown => "Investor Type Breakdown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredOwnershipPdf {
    pub family: OwnershipReportFamily,
    pub listing_page_url: String,
    pub query_url: String,
    pub pdf_url: String,
    pub title: String,
    pub publish_date: String,
    pub code: Option<String>,
    pub original_filename: Option<String>,
    pub is_attachment: bool,
}

struct DiscoveryQuery {
    family: OwnershipReportFamily,
    keywords: &'static str,
    title_needles: &'static [&'static str],
}

const DISCOVERY_QUERIES: &[DiscoveryQuery] = &[
    DiscoveryQuery {
        family: OwnershipReportFamily::AboveOnePercent,
        keywords: "pemegang saham di atas 1",
        title_needles: &["PEMEGANG SAHAM DI ATAS 1", "SHAREHOLDERS ABOVE 1"],
    },
    DiscoveryQuery {
        family: OwnershipReportFamily::AboveFivePercent,
        keywords: "pemegang saham di atas 5",
        title_needles: &["PEMEGANG SAHAM DI ATAS 5", "SHAREHOLDERS ABOVE 5"],
    },
    DiscoveryQuery {
        family: OwnershipReportFamily::InvestorTypeBreakdown,
        keywords: "kepemilikan saham perusahaan tercatat",
        title_needles: &[
            "KEPEMILIKAN SAHAM PERUSAHAAN TERCATAT BERDASARKAN TIPE INVESTOR",
            "DATA KSEI TERKAIT KEPEMILIKAN SAHAM PERUSAHAAN TERCATAT",
        ],
    },
];

pub fn announcement_listing_url() -> String {
    std::env::var(IDX_ANNOUNCEMENT_LISTING_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| IDX_ANNOUNCEMENT_LISTING_URL.to_string())
}

pub fn announcement_api_url() -> String {
    std::env::var(IDX_ANNOUNCEMENT_API_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_IDX_ANNOUNCEMENT_API_URL.to_string())
}

pub fn build_announcement_query_url(
    keywords: &str,
    page_number: usize,
    page_size: usize,
) -> String {
    format!(
        "{}?keywords={}&pageNumber={page_number}&pageSize={page_size}&lang=id",
        announcement_api_url(),
        percent_encode(keywords),
    )
}

pub fn parse_announcement_page(raw: &str) -> Result<AnnouncementPage, IdxError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(IdxError::Http(
            "IDX ownership discovery returned an empty announcement payload".to_string(),
        ));
    }
    let normalized = trimmed.to_ascii_lowercase();
    if normalized.starts_with("<!doctype html") || normalized.starts_with("<html") {
        return Err(IdxError::Http(
            "IDX ownership discovery returned HTML instead of announcement JSON".to_string(),
        ));
    }

    serde_json::from_str(trimmed)
        .map_err(|e| IdxError::ParseError(format!("failed to parse IDX announcement JSON: {e}")))
}

pub fn select_latest_ownership_reports(
    page: &AnnouncementPage,
    query_url: &str,
    family: OwnershipReportFamily,
) -> Result<Vec<DiscoveredOwnershipPdf>, IdxError> {
    let Some(query) = DISCOVERY_QUERIES
        .iter()
        .find(|query| query.family == family)
    else {
        return Err(IdxError::Http(format!(
            "failed to discover IDX ownership reports from {query_url}: unknown report family"
        )));
    };

    let mut matches: Vec<&AnnouncementItem> = page
        .items
        .iter()
        .filter(|item| item_matches_family(item, query))
        .collect();
    matches.sort_by(|left, right| right.publish_date.cmp(&left.publish_date));

    let Some(item) = matches.into_iter().next() else {
        return Err(IdxError::Http(format!(
            "failed to discover IDX ownership reports from {query_url}: no matching announcement found"
        )));
    };

    let mut attachments = item_pdf_attachments(item)
        .into_iter()
        .map(|attachment| {
            let is_attachment = attachment_is_attachment(&attachment);
            let original_filename = attachment
                .original_filename
                .clone()
                .or(attachment.pdf_filename.clone())
                .map(|value| value.trim().to_string());

            DiscoveredOwnershipPdf {
                family,
                listing_page_url: announcement_listing_url(),
                query_url: query_url.to_string(),
                pdf_url: attachment.full_save_path,
                title: item.title.clone(),
                publish_date: item.publish_date.clone(),
                code: clean_option(item.code.as_deref()),
                original_filename,
                is_attachment,
            }
        })
        .collect::<Vec<_>>();

    attachments.sort_by(|left, right| {
        left.is_attachment
            .cmp(&right.is_attachment)
            .then_with(|| left.original_filename.cmp(&right.original_filename))
    });

    if attachments.is_empty() {
        return Err(IdxError::Http(format!(
            "failed to discover IDX ownership reports from {query_url}: matching announcement had no PDF attachments"
        )));
    }

    Ok(attachments)
}

pub fn discover_idx_ownership_reports(
    family_filter: Option<OwnershipReportFamily>,
    limit: usize,
) -> Result<Vec<DiscoveredOwnershipPdf>, IdxError> {
    let mut discovered = Vec::new();
    let mut errors = Vec::new();

    for query in DISCOVERY_QUERIES
        .iter()
        .filter(|query| family_filter.is_none_or(|family| family == query.family))
    {
        let query_url = build_announcement_query_url(query.keywords, 1, IDX_ANNOUNCEMENT_PAGE_SIZE);
        match fetch_text(
            "IDX ownership announcement discovery",
            &query_url,
            &json_headers(),
        )
        .and_then(|raw| parse_announcement_page(&raw))
        .and_then(|page| select_latest_ownership_reports(&page, &query_url, query.family))
        {
            Ok(mut reports) => discovered.append(&mut reports),
            Err(err) => errors.push(err.to_string()),
        }
    }

    discovered.sort_by(|left, right| {
        right
            .publish_date
            .cmp(&left.publish_date)
            .then_with(|| left.is_attachment.cmp(&right.is_attachment))
            .then_with(|| left.original_filename.cmp(&right.original_filename))
    });

    if limit < discovered.len() {
        discovered.truncate(limit);
    }

    if discovered.is_empty() {
        let detail = if errors.is_empty() {
            "no matching ownership reports found".to_string()
        } else {
            errors.join("; ")
        };
        return Err(IdxError::Http(format!(
            "failed to discover IDX ownership reports from {}: {detail}",
            announcement_listing_url()
        )));
    }

    Ok(discovered)
}

pub fn download_idx_pdf(url: &str, target: &Path) -> Result<(), IdxError> {
    let bytes = fetch_bytes("IDX ownership PDF download", url, &pdf_headers())?;
    validate_pdf_payload(&bytes)?;

    fs::write(target, &bytes).map_err(|e| {
        IdxError::Io(format!(
            "failed writing cached PDF {}: {e}",
            target.display()
        ))
    })?;

    Ok(())
}

pub fn validate_pdf_payload(bytes: &[u8]) -> Result<(), IdxError> {
    if bytes.is_empty() {
        return Err(IdxError::Http(
            "IDX ownership download returned an empty response".to_string(),
        ));
    }

    let trimmed = bytes
        .iter()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .copied()
        .collect::<Vec<_>>();
    if trimmed.starts_with(b"%PDF-") {
        return Ok(());
    }

    let preview = String::from_utf8_lossy(&trimmed[..trimmed.len().min(256)]).to_ascii_lowercase();
    if preview.contains("<!doctype html") || preview.contains("<html") {
        return Err(IdxError::Http(
            "IDX ownership download returned HTML instead of a PDF".to_string(),
        ));
    }

    Err(IdxError::Http(
        "IDX ownership download did not look like a PDF".to_string(),
    ))
}

fn fetch_text(stage: &str, url: &str, headers: &[(String, String)]) -> Result<String, IdxError> {
    let bytes = fetch_bytes(stage, url, headers)?;
    String::from_utf8(bytes)
        .map_err(|e| IdxError::Http(format!("failed to decode {stage} response as utf-8: {e}")))
}

fn fetch_bytes(stage: &str, url: &str, headers: &[(String, String)]) -> Result<Vec<u8>, IdxError> {
    let mut args = vec![
        "--silent".to_string(),
        "--show-error".to_string(),
        "--location".to_string(),
        "--fail".to_string(),
        "--compressed".to_string(),
    ];
    for (name, value) in headers {
        args.push("--header".to_string());
        args.push(format!("{name}: {value}"));
    }
    args.push(url.to_string());

    let output = curl_impersonate::run_owned(stage, &args)?;
    Ok(output.stdout)
}

fn json_headers() -> Vec<(String, String)> {
    vec![
        ("User-Agent".to_string(), USER_AGENT.to_string()),
        (
            "Accept".to_string(),
            "application/json,text/plain,*/*".to_string(),
        ),
        (
            "Accept-Language".to_string(),
            "id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7".to_string(),
        ),
        ("Referer".to_string(), announcement_listing_url()),
    ]
}

fn pdf_headers() -> Vec<(String, String)> {
    vec![
        ("User-Agent".to_string(), USER_AGENT.to_string()),
        (
            "Accept".to_string(),
            "application/pdf,application/octet-stream,*/*;q=0.8".to_string(),
        ),
        (
            "Accept-Language".to_string(),
            "id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7".to_string(),
        ),
        ("Referer".to_string(), announcement_listing_url()),
    ]
}

fn item_matches_family(item: &AnnouncementItem, query: &DiscoveryQuery) -> bool {
    let title = item.title.to_ascii_uppercase();
    query
        .title_needles
        .iter()
        .any(|needle| title.contains(needle))
}

fn item_pdf_attachments(item: &AnnouncementItem) -> Vec<AnnouncementAttachment> {
    let attachments = if item.attachments.is_empty() {
        item.pdf_path
            .as_deref()
            .and_then(parse_pdf_path)
            .unwrap_or_default()
    } else {
        item.attachments.clone()
    };

    attachments
        .into_iter()
        .filter(|attachment| {
            attachment
                .full_save_path
                .to_ascii_lowercase()
                .ends_with(".pdf")
        })
        .collect()
}

fn attachment_label(attachment: &AnnouncementAttachment) -> String {
    attachment
        .original_filename
        .as_deref()
        .or(attachment.pdf_filename.as_deref())
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn parse_pdf_path(raw: &str) -> Option<Vec<AnnouncementAttachment>> {
    serde_json::from_str::<Vec<AnnouncementAttachment>>(raw).ok()
}

fn attachment_is_attachment(attachment: &AnnouncementAttachment) -> bool {
    match attachment.is_attachment.as_ref() {
        Some(serde_json::Value::Bool(value)) => *value,
        Some(serde_json::Value::Number(value)) => value.as_i64() == Some(1),
        Some(serde_json::Value::String(value)) => {
            matches!(value.trim(), "1" | "true" | "TRUE" | "True")
        }
        _ => attachment_label(attachment).contains("lamp"),
    }
}

fn clean_option(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push_str("%20"),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        AnnouncementPage, IDX_ANNOUNCEMENT_LISTING_URL, OwnershipReportFamily,
        build_announcement_query_url, parse_announcement_page, select_latest_ownership_reports,
        validate_pdf_payload,
    };
    use crate::error::IdxError;

    #[test]
    fn builds_announcement_query_url() {
        let url = build_announcement_query_url("pemegang saham di atas 5", 1, 10);
        assert_eq!(
            url,
            "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=pemegang%20saham%20di%20atas%205&pageNumber=1&pageSize=10&lang=id"
        );
    }

    #[test]
    fn parses_fixture_and_selects_latest_above_5_reports() {
        let raw = include_str!("../../tests/fixtures/idx_announcement_kepemilikan.json");
        let page = parse_announcement_page(raw).expect("announcement fixture should parse");
        let discovered = select_latest_ownership_reports(
            &page,
            "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=pemegang%20saham%20di%20atas%205&pageNumber=1&pageSize=10&lang=id",
            OwnershipReportFamily::AboveFivePercent,
        )
        .expect("should select ownership reports");

        assert_eq!(discovered.len(), 2);
        assert_eq!(
            discovered[0].family,
            OwnershipReportFamily::AboveFivePercent
        );
        assert_eq!(discovered[0].listing_page_url, IDX_ANNOUNCEMENT_LISTING_URL);
        assert_eq!(discovered[0].publish_date, "2026-03-27T16:34:20");
        assert_eq!(
            discovered[0].pdf_url,
            "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/5d31bb6f49_announcement.pdf"
        );
        assert_eq!(
            discovered[0].original_filename.as_deref(),
            Some("20260327_Semua Emiten Saham_Pengumuman Bursa_32055594.pdf")
        );
        assert!(!discovered[0].is_attachment);
        assert_eq!(
            discovered[1].pdf_url,
            "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/4f5c4efc6f_bf70f249ac_lamp1.pdf"
        );
        assert_eq!(
            discovered[1].original_filename.as_deref(),
            Some("20260327_Semua Emiten Saham_Pengumuman Bursa_32055594_lamp1.pdf")
        );
        assert!(discovered[1].is_attachment);
    }

    #[test]
    fn selects_above_1_reports() {
        let raw = r#"{
          "Items": [
            {
              "PublishDate": "2026-03-10T12:09:09",
              "Title": "Pemegang Saham di atas 1% (KSEI)",
              "AnnouncementType": "",
              "Code": " Semua Emiten Saham ",
              "Attachments": [
                {
                  "PDFFilename": "d67ebf37e6_10d4080288.pdf",
                  "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/d67ebf37e6_10d4080288.pdf",
                  "IsAttachment": 0,
                  "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf"
                },
                {
                  "PDFFilename": "b9b638e5a8_8928aca255.pdf",
                  "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf",
                  "IsAttachment": 1,
                  "OriginalFilename": "20260310_Semua Emiten Saham_Pengumuman Bursa_32052554_lamp1.pdf"
                }
              ],
              "PdfPath": ""
            }
          ],
          "ItemCount": 1,
          "PageCount": 1
        }"#;

        let page = parse_announcement_page(raw).expect("above-1 fixture should parse");
        let discovered = select_latest_ownership_reports(
            &page,
            "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=pemegang%20saham%20di%20atas%201&pageNumber=1&pageSize=10&lang=id",
            OwnershipReportFamily::AboveOnePercent,
        )
        .expect("should select above-1 reports");

        assert_eq!(discovered.len(), 2);
        assert_eq!(discovered[0].family, OwnershipReportFamily::AboveOnePercent);
        assert_eq!(discovered[0].code.as_deref(), Some("Semua Emiten Saham"));
        assert_eq!(
            discovered[0].original_filename.as_deref(),
            Some("20260310_Semua Emiten Saham_Pengumuman Bursa_32052554.pdf")
        );
        assert!(!discovered[0].is_attachment);
        assert_eq!(
            discovered[1].pdf_url,
            "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf"
        );
        assert!(discovered[1].is_attachment);
    }

    #[test]
    fn selects_investor_type_breakdown_reports() {
        let raw = r#"{
          "Items": [
            {
              "PublishDate": "2026-03-02T16:14:37",
              "Title": "Data KSEI terkait Kepemilikan Saham Perusahaan Tercatat Berdasarkan Tipe Investor per 27 Februari 2026",
              "AnnouncementType": "",
              "Code": "  ",
              "Attachments": [
                {
                  "PDFFilename": "20260302_Pengumuman Bursa_32040089.pdf",
                  "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/43025665c8_fda8953269.pdf",
                  "IsAttachment": 0,
                  "OriginalFilename": "20260302_Pengumuman Bursa_32040089.pdf"
                },
                {
                  "PDFFilename": "20260302_Pengumuman Bursa_32040089_lamp1.pdf",
                  "FullSavePath": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/db5b5a86e1_2e2b3d976a.pdf",
                  "IsAttachment": 1,
                  "OriginalFilename": "20260302_Pengumuman Bursa_32040089_lamp1.pdf"
                }
              ],
              "PdfPath": ""
            }
          ],
          "ItemCount": 1,
          "PageCount": 1
        }"#;

        let page = parse_announcement_page(raw).expect("investor-type fixture should parse");
        let discovered = select_latest_ownership_reports(
            &page,
            "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=kepemilikan%20saham%20perusahaan%20tercatat&pageNumber=1&pageSize=10&lang=id",
            OwnershipReportFamily::InvestorTypeBreakdown,
        )
        .expect("should select investor-type reports");

        assert_eq!(discovered.len(), 2);
        assert_eq!(
            discovered[0].original_filename.as_deref(),
            Some("20260302_Pengumuman Bursa_32040089.pdf")
        );
        assert_eq!(
            discovered[1].original_filename.as_deref(),
            Some("20260302_Pengumuman Bursa_32040089_lamp1.pdf")
        );
    }

    #[test]
    fn falls_back_to_pdf_path_when_attachments_are_missing() {
        let raw = r#"{
          "Items": [
            {
              "PublishDate": "2026-03-04T08:54:00",
              "Title": "Pemegang Saham di atas 5% (KSEI)",
              "AnnouncementType": "",
              "Code": "Semua Emiten Saham",
              "Attachments": [],
              "PdfPath": "[{\"PDFFilename\":\"20260305_LKS_KSEI_000043_lamp1.pdf\",\"FullSavePath\":\"https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/ec835d7451_c30c886a1a.pdf\",\"IsAttachment\":\"1\",\"OriginalFilename\":\"20260305_LKS_KSEI_000043_lamp1.pdf\"}]"
            }
          ],
          "ItemCount": 1,
          "PageCount": 1
        }"#;

        let page: AnnouncementPage = parse_announcement_page(raw).expect("fallback page parses");
        let discovered = select_latest_ownership_reports(
            &page,
            "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?keywords=pemegang%20saham%20di%20atas%205&pageNumber=1&pageSize=10&lang=id",
            OwnershipReportFamily::AboveFivePercent,
        )
        .expect("fallback attachment should be parsed");

        assert_eq!(discovered.len(), 1);
        assert_eq!(
            discovered[0].pdf_url,
            "https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/ec835d7451_c30c886a1a.pdf"
        );
        assert_eq!(discovered[0].code.as_deref(), Some("Semua Emiten Saham"));
        assert!(discovered[0].is_attachment);
    }

    #[test]
    fn rejects_html_instead_of_announcement_json() {
        let err = parse_announcement_page("<!doctype html><html><body>blocked</body></html>")
            .expect_err("html response must fail");
        assert!(matches!(err, IdxError::Http(_)));
    }

    #[test]
    fn accepts_pdf_header() {
        validate_pdf_payload(b"%PDF-1.7\n1 0 obj\n").expect("pdf header should pass");
    }

    #[test]
    fn rejects_html_instead_of_pdf() {
        let err = validate_pdf_payload(b"<!doctype html><html><body>blocked</body></html>")
            .expect_err("html body must fail");
        assert!(matches!(err, IdxError::Http(_)));
    }
}
