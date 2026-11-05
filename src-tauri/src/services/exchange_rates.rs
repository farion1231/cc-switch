//! 用量显示币种的汇率拉取。
//!
//! 两个源都按「1 美元折多少目标币种」返回，每天更新一次；主源取不到时退备用源。
//! 两个都失败就返回错误，由前端继续用上次缓存或手填的汇率，不影响记账。

use crate::error::AppError;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

/// 界面「币种」下拉里除美元以外的币种，也是这里要保留的键
pub const TARGET_CURRENCIES: [&str; 5] = ["CNY", "EUR", "JPY", "TWD", "TRY"];

const OPEN_ER_API_URL: &str = "https://open.er-api.com/v6/latest/USD";
const FLOATRATES_URL: &str = "https://www.floatrates.com/daily/usd.json";
/// 单次请求的超时：汇率只用于显示，取不到就退缓存，不能让设置页一直转
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedExchangeRates {
    /// 1 美元折多少目标币种，键是大写币种代码
    pub rates: BTreeMap<String, f64>,
    /// 取到的时间（毫秒时间戳）
    pub fetched_at: i64,
    /// 实际取到数据的源，界面标注用
    pub source: String,
}

#[derive(Deserialize)]
struct OpenErApiResponse {
    result: Option<String>,
    rates: Option<BTreeMap<String, f64>>,
}

/// 只留界面用得到的币种，顺手丢掉非正数、非有限值。
fn pick_target_rates(raw: impl IntoIterator<Item = (String, f64)>) -> BTreeMap<String, f64> {
    raw.into_iter()
        .filter_map(|(code, rate)| {
            let code = code.to_ascii_uppercase();
            let usable =
                TARGET_CURRENCIES.contains(&code.as_str()) && rate.is_finite() && rate > 0.0;
            usable.then_some((code, rate))
        })
        .collect()
}

/// open.er-api.com：`{"result":"success","rates":{"CNY":6.67,...}}`
pub fn parse_open_er_api(body: &str) -> Option<BTreeMap<String, f64>> {
    let parsed: OpenErApiResponse = serde_json::from_str(body).ok()?;
    if parsed.result.as_deref() != Some("success") {
        return None;
    }
    let rates = pick_target_rates(parsed.rates?);
    (!rates.is_empty()).then_some(rates)
}

/// floatrates：`{"cny":{"rate":"6.67",...},...}`，键小写、数值是字符串
pub fn parse_floatrates(body: &str) -> Option<BTreeMap<String, f64>> {
    let parsed: BTreeMap<String, serde_json::Value> = serde_json::from_str(body).ok()?;
    let rates = pick_target_rates(parsed.into_iter().filter_map(|(code, entry)| {
        let raw = entry.get("rate")?;
        let rate = raw.as_f64().or_else(|| raw.as_str()?.trim().parse().ok())?;
        Some((code, rate))
    }));
    (!rates.is_empty()).then_some(rates)
}

async fn fetch_source(
    client: &reqwest::Client,
    url: &str,
    parser: fn(&str) -> Option<BTreeMap<String, f64>>,
) -> Result<BTreeMap<String, f64>, String> {
    let response = client
        .get(url)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body = response.text().await.map_err(|error| error.to_string())?;
    parser(&body).ok_or_else(|| "返回内容无法解析".to_string())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// 一个汇率源：地址、解析函数、失败时写进日志的名字。
struct RateSource {
    name: &'static str,
    url: &'static str,
    parse: fn(&str) -> Option<BTreeMap<String, f64>>,
}

/// 依次尝试主源和备用源，用第一个能解析出目标币种的。
pub async fn fetch_usd_rates() -> Result<FetchedExchangeRates, AppError> {
    let client = crate::proxy::http_client::get();
    let sources = [
        RateSource {
            name: "open.er-api.com",
            url: OPEN_ER_API_URL,
            parse: parse_open_er_api,
        },
        RateSource {
            name: "floatrates.com",
            url: FLOATRATES_URL,
            parse: parse_floatrates,
        },
    ];

    let mut failures = Vec::new();
    for source in sources {
        match fetch_source(&client, source.url, source.parse).await {
            Ok(rates) => {
                return Ok(FetchedExchangeRates {
                    rates,
                    fetched_at: now_ms(),
                    source: source.name.to_string(),
                });
            }
            Err(error) => failures.push(format!("{}: {error}", source.name)),
        }
    }

    Err(AppError::Message(format!(
        "获取汇率失败（{}）",
        failures.join("；")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_er_api_and_keeps_only_target_currencies() {
        let body = r#"{"result":"success","base_code":"USD","rates":{
            "USD":1,"CNY":6.67,"EUR":0.89,"JPY":158.1,"TWD":32.4,"TRY":49.6,"GBP":0.74
        }}"#;
        let rates = parse_open_er_api(body).expect("应解析成功");
        assert_eq!(rates.len(), 5);
        assert_eq!(rates.get("CNY"), Some(&6.67));
        assert_eq!(rates.get("TWD"), Some(&32.4));
        assert!(!rates.contains_key("GBP"));
        assert!(!rates.contains_key("USD"));
    }

    #[test]
    fn rejects_failed_or_empty_open_er_api_response() {
        assert!(parse_open_er_api(r#"{"result":"error","rates":{"CNY":6.67}}"#).is_none());
        assert!(parse_open_er_api(r#"{"result":"success","rates":{"GBP":0.74}}"#).is_none());
        assert!(parse_open_er_api("not json").is_none());
    }

    #[test]
    fn parses_floatrates_string_rates_and_skips_invalid_values() {
        let body = r#"{
            "cny":{"code":"CNY","rate":"6.67"},
            "twd":{"code":"TWD","rate":"32.5"},
            "jpy":{"code":"JPY","rate":158.1},
            "try":{"code":"TRY","rate":"0"},
            "eur":{"code":"EUR","rate":"oops"}
        }"#;
        let rates = parse_floatrates(body).expect("应解析成功");
        assert_eq!(rates.len(), 3);
        assert_eq!(rates.get("CNY"), Some(&6.67));
        assert_eq!(rates.get("JPY"), Some(&158.1));
        assert!(!rates.contains_key("TRY"));
        assert!(!rates.contains_key("EUR"));
        assert!(parse_floatrates("[]").is_none());
    }
}
