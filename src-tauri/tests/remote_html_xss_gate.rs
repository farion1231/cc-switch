//! Executable gate: the remote page never hands a provider field to the HTML parser.
//!
//! The remote management page is served same-origin with the management API and
//! carries the session cookie, so any script it runs runs with the operator's
//! credentials. Provider fields are attacker-reachable: a name can arrive from a
//! deep-link import or from cloud sync, not only from local typing. A name of
//!
//!     <img src=x onerror=alert(1)>
//!
//! written into a markup string becomes a live element, and its `onerror` runs
//! script inside the authenticated page.
//!
//! The page therefore builds every card through DOM nodes, and user data only
//! ever reaches `textContent` or a property assignment. This file is the machine
//! check for that, and it scans `src/remote/html.rs` as it stands on disk.
//!
//! Four rules, because one shape of the bug has several spellings. The first two
//! are the shapes that were actually present (a markup string holding an `img`
//! with an inline `onerror`). The third generalises them past line breaks: a
//! template literal that starts on one line and interpolates on the next is the
//! same defect, and a rule that only looked at single lines would wave it
//! through. The fourth closes the obvious escape hatch: assigning `innerHTML`
//! from a variable that a later edit is free to make unsafe.
//!
//! Two properties this gate would be worthless without:
//!
//! - **Negative fixtures.** Every rule gets a snippet that must trip it and a
//!   snippet that must not, so "always returns clean" cannot pass.
//! - **Non-vacuity.** The real page is asserted to still contain the DOM calls
//!   the fix is made of, proving the scan reads a file with content in it.
//!
//! Every path is anchored at `CARGO_MANIFEST_DIR`, never a literal machine-local
//! path: the repo is cloned onto many machines and CI runners.

use std::fs;
use std::path::{Path, PathBuf};

/// The page source this gate owns, relative to the crate root.
const PAGE_SOURCE: &str = "src/remote/html.rs";

/// The declaration whose body renders provider cards.
const CARD_RENDERER: &str = "async function loadProviders() {";

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The remote page source, read from the tree rather than imported: `html` is a
/// private module, so `REMOTE_HTML` is not nameable from an integration test.
fn page_source() -> String {
    let path = crate_root().join(PAGE_SOURCE);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()))
}

/// 1-based line numbers carrying `needle`.
fn lines_with<'a>(src: &'a str, needle: &str) -> Vec<usize> {
    src.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(needle))
        .map(|(i, _)| i + 1)
        .collect()
}

/// The body of one top-level page function.
///
/// The page is a single `<script>` whose functions all start at column zero, so
/// the next column-zero declaration marks the end. Nesting is unambiguous here:
/// every inner statement is indented.
fn function_section(src: &str, declaration: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.contains(declaration))
        .unwrap_or_else(|| panic!("the page must still declare `{declaration}`"));
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        if line.starts_with("function ")
            || line.starts_with("async function ")
            || line.starts_with("let ")
            || line.starts_with("const ")
            || line.starts_with("window.")
            || line.starts_with("document.")
        {
            end = i;
            break;
        }
    }
    lines[start..end].join("\n")
}

/// A finding: which rule fired, and where.
struct Finding {
    rule: &'static str,
    lines: Vec<usize>,
}

/// Every rule this gate enforces, each with the findings it produces for `src`.
fn findings(src: &str) -> Vec<Finding> {
    // 1. The shape that was present: an HTML sink and a provider field on the
    //    same line.
    let rule_inner_html_with_field: Vec<usize> = src
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("innerHTML") && line.contains("${p."))
        .map(|(i, _)| i + 1)
        .collect();

    // 2. The attribute that carried the name into a parsed markup string.
    let rule_alt_interpolation = lines_with(src, "alt=\"${p.name}\"");

    // 3. The same defect written so it hides across a line break: no provider
    //    field may be interpolated into any string anywhere on the page.
    let rule_any_interpolation = lines_with(src, "${p.");

    // 4. Inside the card renderer, `innerHTML` is only ever allowed for the
    //    empty-string list clear; anything else reopens the sink.
    let section = function_section(src, CARD_RENDERER);
    let rule_renderer_sink: Vec<usize> = section
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("innerHTML") && !line.contains("innerHTML = ''"))
        .map(|(i, _)| i + 1)
        .collect();

    [
        Finding {
            rule: "innerHTML interpolates a provider field",
            lines: rule_inner_html_with_field,
        },
        Finding {
            rule: "alt attribute interpolates the provider name",
            lines: rule_alt_interpolation,
        },
        Finding {
            rule: "provider field interpolated into a page string",
            lines: rule_any_interpolation,
        },
        Finding {
            rule: "card renderer assigns innerHTML beyond the list clear",
            lines: rule_renderer_sink,
        },
    ]
    .into_iter()
    .filter(|f| !f.lines.is_empty())
    .collect()
}

fn report(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| {
            let lines: Vec<String> = f.lines.iter().map(usize::to_string).collect();
            format!("{} (line {})", f.rule, lines.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

// ---------------------------------------------------------------------------
// The two required claims
// ---------------------------------------------------------------------------

#[test]
fn page_never_interpolates_provider_fields_into_html() {
    let src = page_source();
    let found = findings(&src);
    assert!(
        found.is_empty(),
        "the remote page must build cards with DOM nodes, not markup strings; got: {}",
        report(&found)
    );

    // Non-vacuity: the scan read a file that actually carries the constructs
    // these rules discriminate between. A page with neither the old sink nor
    // the required list clear would satisfy the rules while proving nothing.
    assert!(
        src.contains("list.innerHTML = ''"),
        "the fixture this gate scans must still contain the allowed list clear"
    );
}

#[test]
fn card_built_with_dom_api() {
    let src = page_source();
    let section = function_section(&src, CARD_RENDERER);

    for (what, needle) in [
        ("name via textContent", ".textContent = p.name;"),
        ("category via textContent", ".textContent = p.category;"),
        (
            "status via textContent",
            ".textContent = p.is_current ? 'Active' : 'Switch';",
        ),
        ("icon element via DOM", "createElement('img')"),
        ("fallback via an error listener", "addEventListener('error'"),
        ("fallback text node", "createTextNode(initial)"),
        ("alt as a property", "img.alt = p.name || '';"),
        (
            "icon colour only through an encoder",
            "'?color=' + encodeURIComponent(color)",
        ),
        (
            "icon path only through an encoder",
            "encodeURIComponent(p.icon)",
        ),
        ("card class preserved", "card.className = 'provider-card'"),
        ("info class preserved", "info.className = 'info'"),
        ("status class preserved", "status.className = 'status'"),
    ] {
        assert!(
            section.contains(needle),
            "the card renderer must build `{what}` with the DOM; `{needle}` is missing"
        );
    }
}

// ---------------------------------------------------------------------------
// Fixtures — each rule has to be able to go red, and to stay quiet
// ---------------------------------------------------------------------------

const VULNERABLE_CARD: &str = "\
async function loadProviders() {
    const list = document.getElementById('providerList');
    list.innerHTML = '';
    (data.providers || []).forEach(p => {
        const initial = (p.name || '?')[0].toUpperCase();
        const iconHtml = p.icon
            ? `<img src=\"/api/provider-icons/${p.icon}\" alt=\"${p.name}\" onerror=\"this.parentElement.innerHTML='${initial}'\">`
            : `<span>${initial}</span>`;
        card.innerHTML = `
                <div class=\"info\">
                    <div class=\"name\">${p.name}</div>
                </div>
            `;
    });
}
";

const SAFE_CARD: &str = "\
async function loadProviders() {
    const list = document.getElementById('providerList');
    list.innerHTML = '';
    (data.providers || []).forEach(p => {
        const nameEl = document.createElement('div');
        nameEl.className = 'name';
        nameEl.textContent = p.name;
        const img = document.createElement('img');
        img.src = '/api/provider-icons/' + encodeURIComponent(p.icon);
        img.alt = p.name || '';
        img.addEventListener('error', () => icon.replaceChildren(document.createTextNode(initial)));
        card.replaceChildren(info);
    });
}
";

#[test]
fn every_rule_fires_on_the_vulnerable_card() {
    let found = findings(VULNERABLE_CARD);
    for rule in [
        "innerHTML interpolates a provider field",
        "alt attribute interpolates the provider name",
        "provider field interpolated into a page string",
        "card renderer assigns innerHTML beyond the list clear",
    ] {
        assert!(
            found.iter().any(|f| f.rule == rule),
            "`{rule}` must fire on the markup-string card; got: {}",
            report(&found)
        );
    }
}

#[test]
fn no_rule_fires_on_the_dom_card() {
    let found = findings(SAFE_CARD);
    assert!(
        found.is_empty(),
        "the DOM-built card must trip nothing; got: {}",
        report(&found)
    );
}

#[test]
fn rules_are_scoped_to_the_card_renderer() {
    // The list clear and the body-level messages are not provider fields.
    let src = "\
async function loadProviders() {
    list.innerHTML = '';
}
document.addEventListener('DOMContentLoaded', loadProviders);
document.body.innerHTML = '<div>Remote server stopped.</div>';
";
    assert!(
        findings(src).is_empty(),
        "literal messages are not provider data; got: {}",
        report(&findings(src))
    );
}
