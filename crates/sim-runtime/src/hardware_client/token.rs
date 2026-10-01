//! The control token hand-off: how the viewer gets the token a hardware
//! server generates at start (32 random bytes, 64 lowercase hex characters).
//!
//! **Chosen: read it from the page the server serves**, exactly as the
//! browser receives it. The calibration server injects
//! `<meta name="calibration-token" content="…">` before `</body>` of `/`;
//! the motor bench replaces `__TOKEN__` in `const token='…'` of `/` and
//! `__CONTROL_TOKEN__` in the `motor-bridge-token` meta of `/walking/`. The
//! viewer fetches that page over the same loopback client (with the
//! server's own `Host`) and extracts the token ([`discover`]); an operator
//! may name a file holding it instead ([`read_file`]).
//!
//! Why: it needs no server change, and it grants nothing new: any local
//! process that can reach the loopback port can already load the page and
//! read the token, as the browser does. The token guards against other
//! origins (a web page in the browser cannot read the server's page across
//! origins), not against local processes.
//!
//! Rejected: the server writing the token to a 0600 file for the viewer to
//! read. It would narrow access to the server's user, but it is a server
//! change (both servers, their configs and their tests) for no gain while
//! the page stays readable.
use super::{Client, ClientError, Endpoint, ServerKind, process_client_id};
use std::path::Path;
use std::time::Duration;

/// Timeout for fetching a page to read its token.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);

/// Whether `s` is a token as both servers make them: 64 lowercase hex
/// characters. Rejects the unreplaced placeholders and empty values.
pub fn is_token(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The token in a page the server served, if it carries a valid one.
pub fn from_page(kind: ServerKind, html: &str) -> Option<String> {
    match kind {
        ServerKind::Calibration => delimited(html, "<meta name=\"calibration-token\" content=\"", '"'),
        ServerKind::MotorBench => delimited(html, "const token='", '\'').or_else(|| delimited(html, "<meta name=\"motor-bridge-token\" content=\"", '"')),
    }
}

/// The first valid token that follows `prefix` and ends at `end`.
fn delimited(html: &str, prefix: &str, end: char) -> Option<String> {
    html.match_indices(prefix)
        .filter_map(|(i, _)| {
            let rest = &html[i + prefix.len()..];
            let value = &rest[..rest.find(end)?];
            is_token(value).then(|| value.to_string())
        })
        .next()
}

/// Reads the token from the server's page: `/`, and for the motor bench
/// `/walking/` when `/` carries none.
pub fn discover(endpoint: &Endpoint, kind: ServerKind) -> Result<String, ClientError> {
    let probe = Client::new(endpoint.clone(), String::new(), String::new()).with_timeout(DISCOVERY_TIMEOUT);
    let pages: &[&str] = match kind {
        ServerKind::Calibration => &["/"],
        ServerKind::MotorBench => &["/", "/walking/"],
    };
    let mut last_error = None;
    for page in pages {
        match probe.page(page) {
            Ok(html) => {
                if let Some(token) = from_page(kind, &html) {
                    return Ok(token);
                }
            }
            // The server answered: an error page is the reason if nothing else is found.
            Err(e @ ClientError::Server { .. }) => last_error = Some(e),
            Err(e) => return Err(e),
        }
    }
    let server = match kind {
        ServerKind::Calibration => "calibration server (serve_actuator_calibration)",
        ServerKind::MotorBench => "motor bench (serve_motor_bench)",
    };
    Err(match last_error {
        Some(ClientError::Server { status, error }) => ClientError::Server { status, error: format!("{}: no control token found ({error}); is this the {server}?", endpoint.origin()) },
        _ => ClientError::Decode(format!("{}: the page carries no control token; is this the {server}?", endpoint.origin())),
    })
}

/// A token from a file (surrounding whitespace ignored); the error names the path.
pub fn read_file(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let token = text.trim();
    if !is_token(token) {
        return Err(format!("{}: not a control token (expected 64 lowercase hex characters)", path.display()));
    }
    Ok(token.to_string())
}

/// A client for the server at `url` (loopback only): the token from
/// `token_file` when given, else from the server's page; this process's
/// client id ([`process_client_id`], the same on every connect and
/// reconnect, as the page keeps one per page load);
/// the body limit of `kind`'s server ([`Client::for_kind`]).
pub fn connect(url: &str, token_file: Option<&Path>, kind: ServerKind) -> Result<Client, ClientError> {
    let endpoint = Endpoint::parse(url)?;
    let token = match token_file {
        Some(path) => read_file(path).map_err(ClientError::Decode)?,
        None => discover(&endpoint, kind)?,
    };
    Ok(Client::new(endpoint, token, process_client_id()).for_kind(kind))
}
