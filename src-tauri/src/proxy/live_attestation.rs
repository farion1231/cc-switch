//! Local generation of the opaque DeviceCheck proof required by Codex Live.
//!
//! Codex Desktop may send a short placeholder while its API traffic is routed
//! through CC Switch. On macOS we can ask the signed DeviceCheck module bundled
//! with the official ChatGPT app for a fresh proof without modifying Codex.

use http::{HeaderMap, HeaderValue};
#[cfg(target_os = "macos")]
use once_cell::sync::Lazy;
#[cfg(target_os = "macos")]
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Output;
#[cfg(target_os = "macos")]
use std::time::Duration;
#[cfg(target_os = "macos")]
use tokio::process::Command;
#[cfg(target_os = "macos")]
use uuid::Uuid;

pub(crate) const HEADER_NAME: &str = "x-oai-attestation";
pub(crate) const MIN_BYTES: usize = 20;
pub(crate) const MAX_BYTES: usize = 16 * 1024;

#[cfg(target_os = "macos")]
const CHATGPT_APPLICATION_PATH: &str = "/Applications/ChatGPT.app";
#[cfg(target_os = "macos")]
const GENERATION_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(target_os = "macos")]
const OPENAI_TEAM_IDENTIFIER: &str = "2DC432GLL2";
#[cfg(target_os = "macos")]
const OPENAI_BUNDLE_IDENTIFIERS: &[&str] = &["com.openai.codex", "com.openai.chat"];

#[cfg(target_os = "macos")]
static APP_SESSION_ID: Lazy<String> = Lazy::new(|| Uuid::new_v4().to_string());

#[cfg(target_os = "macos")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSignals {
    schema_version: u8,
    preferred_languages: Vec<String>,
    locale: String,
    timezone: String,
    screen_size_sum: u64,
    screen_scale: f64,
    app_session_id: String,
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
struct MacOsSignals {
    locale: String,
    languages: Vec<String>,
    timezone: String,
    width: f64,
    height: f64,
    scale: f64,
}

pub(crate) fn is_usable_value(value: &HeaderValue) -> bool {
    validate_value(value).is_ok()
}

pub(crate) async fn ensure(headers: &mut HeaderMap) -> Result<HeaderValue, String> {
    if let Some(value) = headers
        .get(HEADER_NAME)
        .filter(|value| is_usable_value(value))
    {
        return Ok(value.clone());
    }

    let generated = generate().await?;
    install_generated(headers, generated)
}

fn install_generated(
    headers: &mut HeaderMap,
    generated: HeaderValue,
) -> Result<HeaderValue, String> {
    validate_value(&generated)?;
    headers.insert(HEADER_NAME, generated.clone());
    Ok(generated)
}

async fn generate() -> Result<HeaderValue, String> {
    #[cfg(not(target_os = "macos"))]
    {
        Err("local Codex Live attestation generation is only available on macOS".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        generate_macos().await
    }
}

#[cfg(target_os = "macos")]
async fn generate_macos() -> Result<HeaderValue, String> {
    if std::env::consts::ARCH != "aarch64" {
        return Err("local Codex Live attestation requires Apple Silicon".to_string());
    }

    let app_path = find_chatgpt_application()?;
    verify_openai_application(&app_path).await?;
    let resources = app_path.join("Contents/Resources");
    let node_path = resources.join("cua_node/bin/node");
    let module_path = resources.join("native/devicecheck.node");
    require_file(&node_path, "bundled Node.js runtime")?;
    require_file(&module_path, "DeviceCheck native module")?;

    let bundle_id = read_bundle_identifier(&app_path).await?;
    let signals = read_signals().await?;
    let signals_json = serde_json::to_string(&signals)
        .map_err(|error| format!("encode Live attestation signals: {error}"))?;

    let mut command = Command::new(node_path);
    command
        .arg("-e")
        .arg(DEVICECHECK_SCRIPT)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CCSWITCH_DEVICECHECK_MODULE", module_path)
        .env("CCSWITCH_ATTESTATION_BUNDLE_ID", bundle_id)
        .env("CCSWITCH_ATTESTATION_SIGNALS", signals_json);
    let output = run_command(command, "ChatGPT DeviceCheck token generation").await?;
    if !output.status.success() {
        return Err(format!(
            "ChatGPT DeviceCheck token generation failed: {}",
            output_reason(&output)
        ));
    }

    let raw = String::from_utf8(output.stdout)
        .map_err(|_| "ChatGPT DeviceCheck returned non-UTF-8 data".to_string())?;
    let value = HeaderValue::from_str(raw.trim())
        .map_err(|_| "ChatGPT DeviceCheck returned an invalid header value".to_string())?;
    validate_value(&value)?;
    Ok(value)
}

#[cfg(target_os = "macos")]
fn find_chatgpt_application() -> Result<PathBuf, String> {
    let mut candidates = vec![PathBuf::from(CHATGPT_APPLICATION_PATH)];
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join("Applications/ChatGPT.app"));
    }
    candidates
        .into_iter()
        .find(|path| path.is_dir())
        .ok_or_else(|| "the official ChatGPT app is not installed".to_string())
}

#[cfg(target_os = "macos")]
fn require_file(path: &Path, label: &str) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("the official ChatGPT app is missing its {label}"))
    }
}

#[cfg(target_os = "macos")]
async fn read_bundle_identifier(app_path: &Path) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/plutil");
    command
        .arg("-extract")
        .arg("CFBundleIdentifier")
        .arg("raw")
        .arg(app_path.join("Contents/Info.plist"));
    let output = run_command(command, "read ChatGPT bundle identifier").await?;
    if !output.status.success() {
        return Err("cannot read the official ChatGPT app bundle identifier".to_string());
    }
    let bundle_id = String::from_utf8(output.stdout)
        .map_err(|_| "the ChatGPT bundle identifier is not UTF-8".to_string())?
        .trim()
        .to_string();
    if !OPENAI_BUNDLE_IDENTIFIERS.contains(&bundle_id.as_str()) {
        return Err("the installed ChatGPT app has an unexpected bundle identifier".to_string());
    }
    Ok(bundle_id)
}

#[cfg(target_os = "macos")]
async fn verify_openai_application(app_path: &Path) -> Result<(), String> {
    let mut verify = Command::new("/usr/bin/codesign");
    verify
        .arg("--verify")
        .arg("--deep")
        .arg("--strict")
        .arg(app_path);
    let output = run_command(verify, "verify OpenAI desktop app signature").await?;
    if !output.status.success() {
        return Err(format!(
            "the OpenAI desktop app signature is invalid: {}",
            output_reason(&output)
        ));
    }

    let mut inspect = Command::new("/usr/bin/codesign");
    inspect.arg("-dv").arg("--verbose=4").arg(app_path);
    let output = run_command(inspect, "inspect OpenAI desktop app signature").await?;
    if !output.status.success() {
        return Err(format!(
            "cannot inspect the OpenAI desktop app signature: {}",
            output_reason(&output)
        ));
    }
    let details = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let team_identifier = signature_detail(&details, "TeamIdentifier");
    let identifier = signature_detail(&details, "Identifier");
    if team_identifier.as_deref() != Some(OPENAI_TEAM_IDENTIFIER)
        || !identifier
            .as_deref()
            .is_some_and(|value| OPENAI_BUNDLE_IDENTIFIERS.contains(&value))
    {
        return Err("the desktop app is not signed by the expected OpenAI team".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn signature_detail(details: &str, name: &str) -> Option<String> {
    details.lines().find_map(|line| {
        line.strip_prefix(name)
            .and_then(|value| value.strip_prefix('='))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    })
}

#[cfg(target_os = "macos")]
async fn read_signals() -> Result<DeviceSignals, String> {
    const SCRIPT: &str = r#"ObjC.import("Foundation"); ObjC.import("AppKit");
const screen = $.NSScreen.mainScreen;
const frame = screen.frame;
JSON.stringify({
  locale: ObjC.unwrap($.NSLocale.currentLocale.localeIdentifier),
  languages: ObjC.deepUnwrap($.NSLocale.preferredLanguages),
  timezone: ObjC.unwrap($.NSTimeZone.localTimeZone.name),
  width: Number(frame.size.width),
  height: Number(frame.size.height),
  scale: Number(screen.backingScaleFactor)
})"#;

    let mut command = Command::new("/usr/bin/osascript");
    command.arg("-l").arg("JavaScript").arg("-e").arg(SCRIPT);
    let output = run_command(command, "read macOS signals for Live attestation").await?;
    if !output.status.success() {
        return Err(format!(
            "read macOS signals for Live attestation failed: {}",
            output_reason(&output)
        ));
    }
    let values: MacOsSignals = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("decode macOS signals for Live attestation: {error}"))?;

    let locale = truncate_signal(values.locale, 64, "unknown");
    let mut languages = values.languages;
    if languages.is_empty() {
        languages.push(locale.clone());
    }
    languages.truncate(16);
    for language in &mut languages {
        *language = truncate_signal(std::mem::take(language), 64, &locale);
    }
    let screen_scale = if values.scale > 0.0 {
        values.scale
    } else {
        1.0
    };
    let screen_size_sum = (values.width + values.height + 0.5).max(0.0) as u64;

    Ok(DeviceSignals {
        schema_version: 1,
        preferred_languages: languages,
        locale,
        timezone: truncate_signal(values.timezone, 64, "unknown"),
        screen_size_sum,
        screen_scale,
        app_session_id: truncate_signal(
            APP_SESSION_ID.as_str().to_string(),
            128,
            &Uuid::new_v4().to_string(),
        ),
    })
}

#[cfg(target_os = "macos")]
fn truncate_signal(mut value: String, limit: usize, fallback: &str) -> String {
    value = value.trim().to_string();
    if value.is_empty() {
        value = fallback.to_string();
    }
    value.chars().take(limit).collect()
}

#[cfg(target_os = "macos")]
async fn run_command(mut command: Command, label: &str) -> Result<Output, String> {
    command.kill_on_drop(true);
    tokio::time::timeout(GENERATION_TIMEOUT, command.output())
        .await
        .map_err(|_| format!("{label} timed out"))?
        .map_err(|error| format!("{label} failed to start: {error}"))
}

#[cfg(target_os = "macos")]
fn output_reason(output: &Output) -> String {
    let mut reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if reason.is_empty() {
        reason = output.status.to_string();
    }
    reason.chars().take(240).collect()
}

fn validate_value(value: &HeaderValue) -> Result<(), String> {
    if !(MIN_BYTES..=MAX_BYTES).contains(&value.as_bytes().len()) {
        return Err("ChatGPT DeviceCheck returned a malformed attestation length".to_string());
    }
    let decoded: serde_json::Value = serde_json::from_slice(value.as_bytes())
        .map_err(|_| "ChatGPT DeviceCheck returned malformed attestation JSON".to_string())?;
    let valid = decoded.get("v").and_then(serde_json::Value::as_u64) == Some(1)
        && decoded
            .get("s")
            .and_then(serde_json::Value::as_u64)
            .is_some()
        && decoded
            .get("t")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|token| token.starts_with("v1.") && token.len() >= MIN_BYTES);
    if !valid {
        return Err("ChatGPT DeviceCheck returned an unexpected attestation shape".to_string());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const DEVICECHECK_SCRIPT: &str = r#"
const addon = require(process.env.CCSWITCH_DEVICECHECK_MODULE);
const signals = JSON.parse(process.env.CCSWITCH_ATTESTATION_SIGNALS);
const bundleID = process.env.CCSWITCH_ATTESTATION_BUNDLE_ID;

function head(major, value) {
  if (value < 24) return Buffer.from([major + value]);
  if (value <= 255) return Buffer.from([major + 24, value]);
  if (value <= 65535) {
    const out = Buffer.allocUnsafe(3);
    out[0] = major + 25;
    out.writeUInt16BE(value, 1);
    return out;
  }
  const out = Buffer.allocUnsafe(5);
  out[0] = major + 26;
  out.writeUInt32BE(value, 1);
  return out;
}
function uint(value) { return head(0, value); }
function text(value) {
  const body = Buffer.from(value, "utf8");
  return Buffer.concat([head(96, body.length), body]);
}
function float(value) {
  if (Number.isSafeInteger(value) && value >= 0) return uint(value);
  const out = Buffer.allocUnsafe(9);
  out[0] = 251;
  out.writeDoubleBE(value, 1);
  return out;
}
function array(values) { return Buffer.concat([head(128, values.length), ...values]); }
function map(entries) {
  return Buffer.concat([head(160, entries.length), ...entries.flatMap(([key, value]) => [uint(key), value])]);
}
function field(key, value) { return Buffer.concat([text(key), text(value)]); }
function base64url(value) {
  return value.toString("base64").replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/u, "");
}

(async () => {
  const result = await addon.generateToken();
  if (!result || !result.supported) throw new Error("DeviceCheck is not supported on this Mac");
  if (!result.tokenBase64) throw new Error("DeviceCheck returned no token");
  const fingerprint = map([
    [0, uint(signals.schemaVersion)],
    [1, array(signals.preferredLanguages.map(text))],
    [2, text(signals.locale)],
    [3, text(signals.timezone)],
    [4, uint(signals.screenSizeSum)],
    [5, float(signals.screenScale)],
    [6, text(signals.appSessionId)]
  ]);
  const fields = [
    field("token", result.tokenBase64),
    field("bundle_id", bundleID),
    Buffer.concat([text("f"), head(64, fingerprint.length), fingerprint])
  ];
  if (result.latencyMs != null) {
    fields.push(Buffer.concat([text("t"), float(result.latencyMs)]));
  }
  const token = "v1." + base64url(Buffer.concat([Buffer.from([160 + fields.length]), ...fields]));
  process.stdout.write(JSON.stringify({v: 1, s: 0, t: token}));
})().catch((error) => {
  process.stderr.write(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_attestation(label: &str) -> HeaderValue {
        HeaderValue::from_str(
            &serde_json::json!({"v": 1, "s": 0, "t": format!("v1.{label}")}).to_string(),
        )
        .unwrap()
    }

    #[test]
    fn mock_generator_replaces_short_placeholder() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER_NAME, HeaderValue::from_static("ccswitch-key"));

        let installed = install_generated(&mut headers, fake_attestation("generated-proof-value"))
            .expect("install generated attestation");

        assert_eq!(headers.get(HEADER_NAME), Some(&installed));
        assert!(is_usable_value(&installed));
    }

    #[test]
    fn malformed_generated_value_is_rejected() {
        let mut headers = HeaderMap::new();
        let error = install_generated(
            &mut headers,
            HeaderValue::from_static("not-a-valid-attestation-value"),
        )
        .expect_err("reject malformed attestation");

        assert!(error.contains("JSON"));
        assert!(!headers.contains_key(HEADER_NAME));
    }

    #[test]
    fn supplied_attestation_requires_valid_structure_not_only_length() {
        assert!(!is_usable_value(&HeaderValue::from_static(
            "not-a-valid-attestation-value"
        )));
        assert!(is_usable_value(&fake_attestation("valid-proof-value")));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "requires the installed official ChatGPT app and DeviceCheck service"]
    async fn installed_chatgpt_app_generates_a_valid_attestation() {
        let value = generate().await.expect("generate local attestation");
        assert!(is_usable_value(&value));
        validate_value(&value).expect("validate local attestation");
    }
}
