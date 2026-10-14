//! Finding the session description inside a SIP message body.
//!
//! RFC 5621 §4.2: "SIP UAs MUST support parsing 'multipart' MIME bodies,
//! including nested body parts. Additionally, UAs MUST support the
//! 'multipart/mixed' and 'multipart/alternative' MIME types." Carriers do send
//! them: T2 (issue #103) wraps every mobile-terminating INVITE's SDP in a
//! `multipart/mixed` body alongside an optional PIDF-LO location part, and a
//! gate that only admitted `application/sdp` declined every one of those calls
//! `415`.
//!
//! [`extract_sdp`] is the one place that decides which bytes of a body are the
//! SDP — the inbound INVITE, the offerless-INVITE ACK and the outbound 2xx all
//! go through it. Framing follows RFC 2046 §5.1.1 (delimiters, preamble,
//! epilogue, the CRLF before a delimiter belonging to the delimiter);
//! disposition defaults follow RFC 3261 §20.11 (`session` for
//! `application/sdp`, `render` otherwise, `handling=required`). Part
//! `Content-Length` headers are ignored: they are not MIME headers, and the
//! boundary is the only framing.
//!
//! Deliberate leniency (specs/088 Clarification Q1): a part this bridge does
//! not understand is skipped whatever its `handling` says, logged at `warn`
//! when it was `required`. RFC 5621 §8.4 says SHOULD `415`; doing so would
//! repeat #103 for any carrier that forgets `handling=optional` on a 3GPP XML
//! part.

use std::fmt;

/// Nested multiparts deeper than this are refused (`400`). Real IMS bodies are
/// depth 1; the bound only limits work on a body the carrier controls.
const MAX_DEPTH: usize = 4;
/// Parts per multipart level beyond this are refused (`400`).
const MAX_PARTS: usize = 16;
/// RFC 2046 §5.1.1 BNF: `boundary := 0*69<bchars> bcharsnospace`.
const MAX_BOUNDARY_LEN: usize = 70;

/// A parsed `Content-Type` value (RFC 2045 §5.1): type, subtype and
/// parameters, with names lowercased and quoted values unquoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaType {
    pub(crate) type_: String,
    pub(crate) subtype: String,
    pub(crate) params: Vec<(String, String)>,
}

impl MediaType {
    /// `None` when the value has no `type/subtype`.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        let mut fields = split_params(value).into_iter();
        let essence = fields.next()?;
        let (type_, subtype) = essence.split_once('/')?;
        let (type_, subtype) = (type_.trim(), subtype.trim());
        if type_.is_empty() || subtype.is_empty() {
            return None;
        }
        Some(Self {
            type_: type_.to_ascii_lowercase(),
            subtype: subtype.to_ascii_lowercase(),
            params: fields.filter_map(parse_param).collect(),
        })
    }

    /// Whether this is `essence` (`"type/subtype"`), ignoring case and
    /// parameters.
    pub(crate) fn is(&self, essence: &str) -> bool {
        essence.split_once('/').is_some_and(|(t, s)| {
            self.type_.eq_ignore_ascii_case(t) && self.subtype.eq_ignore_ascii_case(s)
        })
    }

    /// The value of parameter `name` (case-insensitive), unquoted.
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub(crate) fn is_multipart(&self) -> bool {
        self.type_ == "multipart"
    }

    fn is_sdp(&self) -> bool {
        self.is("application/sdp")
    }

    /// `type/subtype`, without parameters — what the log line names.
    fn essence(&self) -> String {
        format!("{}/{}", self.type_, self.subtype)
    }
}

/// Splits a header value on `;` outside quoted strings, trimming each field.
fn split_params(value: &str) -> Vec<&str> {
    let mut fields = Vec::new();
    let (mut start, mut in_quotes, mut escaped) = (0, false, false);
    for (i, c) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if in_quotes && c == '\\' {
            escaped = true;
        } else if c == '"' {
            in_quotes = !in_quotes;
        } else if c == ';' && !in_quotes {
            fields.push(value[start..i].trim());
            start = i + 1;
        }
    }
    fields.push(value[start..].trim());
    fields
}

/// `name=value` or `name="quoted \" value"` → lowercased name, unquoted value.
/// A bare `name` (no `=`) is kept with an empty value.
fn parse_param(field: &str) -> Option<(String, String)> {
    if field.is_empty() {
        return None;
    }
    let (name, value) = field.split_once('=').unwrap_or((field, ""));
    let value = value.trim();
    let value = match value.strip_prefix('"') {
        Some(quoted) => {
            let mut out = String::new();
            let mut chars = quoted.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => out.extend(chars.next()),
                    '"' => break,
                    c => out.push(c),
                }
            }
            out
        }
        None => value.to_string(),
    };
    Some((name.trim().to_ascii_lowercase(), value))
}

/// RFC 3261 §20.11 `handling` parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handling {
    Required,
    Optional,
}

/// A parsed `Content-Disposition`, with RFC 3261 §20.11's defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Disposition {
    pub(crate) kind: String,
    pub(crate) handling: Handling,
}

impl Disposition {
    /// `header` is the part's `Content-Disposition`, if any; `media_type` its
    /// `Content-Type`, which decides the default kind (`session` for
    /// `application/sdp`, `render` otherwise). An unknown `handling` value is
    /// treated as `required` (RFC 3261 §20.11's default).
    pub(crate) fn parse(header: Option<&str>, media_type: Option<&MediaType>) -> Self {
        let default_kind = if media_type.is_some_and(MediaType::is_sdp) {
            "session"
        } else {
            "render"
        };
        let mut fields = header.map(split_params).unwrap_or_default().into_iter();
        let kind = fields
            .next()
            .filter(|k| !k.is_empty())
            .map(str::to_ascii_lowercase)
            .unwrap_or_else(|| default_kind.to_string());
        let handling = fields
            .filter_map(parse_param)
            .find(|(k, _)| k == "handling")
            .map_or(Handling::Required, |(_, v)| {
                if v.eq_ignore_ascii_case("optional") {
                    Handling::Optional
                } else {
                    Handling::Required
                }
            });
        Self { kind, handling }
    }
}

/// One body part of a multipart body.
#[derive(Debug)]
struct Part<'a> {
    /// `None` when the part has no `Content-Type`: RFC 2045 §5.2 makes that
    /// `text/plain`, so such a part is never SDP.
    media_type: Option<MediaType>,
    disposition: Disposition,
    transfer_encoding: Option<String>,
    content: &'a [u8],
}

impl Part<'_> {
    fn type_name(&self) -> String {
        self.media_type
            .as_ref()
            .map_or_else(|| "text/plain".to_string(), MediaType::essence)
    }
}

/// Lines of `bytes` as `(start, end_of_content, end_including_terminator)`,
/// accepting CRLF or bare LF.
fn lines(bytes: &[u8]) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    let mut pos = 0;
    std::iter::from_fn(move || {
        if pos >= bytes.len() {
            return None;
        }
        let start = pos;
        let (content_end, next) = match bytes[start..].iter().position(|&b| b == b'\n') {
            Some(i) => {
                let lf = start + i;
                let content_end = if lf > start && bytes[lf - 1] == b'\r' {
                    lf - 1
                } else {
                    lf
                };
                (content_end, lf + 1)
            }
            None => (bytes.len(), bytes.len()),
        };
        pos = next;
        Some((start, content_end, next))
    })
}

enum Delimiter {
    Part,
    Close,
}

/// Whether `line` is a delimiter line for `boundary` — `--boundary` then only
/// whitespace (RFC 2046 §5.1.1 permits trailing linear whitespace), or the
/// close delimiter `--boundary--`.
fn delimiter(line: &[u8], boundary: &[u8]) -> Option<Delimiter> {
    let rest = line.strip_prefix(b"--")?.strip_prefix(boundary)?;
    let (kind, rest) = match rest.strip_prefix(b"--") {
        Some(rest) => (Delimiter::Close, rest),
        None => (Delimiter::Part, rest),
    };
    rest.iter()
        .all(|b| *b == b' ' || *b == b'\t')
        .then_some(kind)
}

/// RFC 2046 §5.1.1: splits `body` on `boundary` into its parts, dropping the
/// preamble and epilogue. The line break before each delimiter belongs to the
/// delimiter, not the preceding part.
fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Result<Vec<Part<'a>>, &'static str> {
    if boundary.is_empty() || boundary.len() > MAX_BOUNDARY_LEN {
        return Err("boundary length out of range");
    }
    let boundary = boundary.as_bytes();
    let mut parts = Vec::new();
    // Where the current part's content begins, once the first delimiter is seen.
    let mut part_start: Option<usize> = None;
    for (start, content_end, next) in lines(body) {
        let Some(kind) = delimiter(&body[start..content_end], boundary) else {
            continue;
        };
        if let Some(from) = part_start {
            // Strip the line break that ends the previous line: it is part of
            // this delimiter. For an empty part that break is the previous
            // delimiter's own, so never step back past `from`.
            let mut end = start;
            if end > from && body[end - 1] == b'\n' {
                end -= 1;
                if end > from && body[end - 1] == b'\r' {
                    end -= 1;
                }
            }
            if parts.len() == MAX_PARTS {
                return Err("too many parts");
            }
            parts.push(parse_part(&body[from..end.max(from)]));
        }
        match kind {
            Delimiter::Part => part_start = Some(next),
            Delimiter::Close => {
                return if part_start.is_some() {
                    Ok(parts)
                } else {
                    Err("close delimiter before any part")
                };
            }
        }
    }
    Err(if part_start.is_some() {
        "missing close delimiter"
    } else {
        "no boundary delimiter"
    })
}

/// A part's headers (up to the first blank line; folded lines joined; names
/// case-insensitive) and content. SIP's compact header forms do not apply
/// inside MIME parts (RFC 3261 §7.3.3 covers SIP message headers only).
fn parse_part(raw: &[u8]) -> Part<'_> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_start = raw.len();
    for (start, content_end, next) in lines(raw) {
        let line = &raw[start..content_end];
        if line.is_empty() {
            content_start = next;
            break;
        }
        let line = String::from_utf8_lossy(line);
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let media_type = header("content-type").and_then(MediaType::parse);
    let disposition = Disposition::parse(header("content-disposition"), media_type.as_ref());
    let transfer_encoding =
        header("content-transfer-encoding").map(|v| v.trim().to_ascii_lowercase());
    Part {
        media_type,
        disposition,
        transfer_encoding,
        content: &raw[content_start..],
    }
}

/// What a body turned out to hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Decoded {
    /// No body (or only whitespace): an offerless INVITE, an ACK with no
    /// answer.
    Empty,
    /// Exactly one session description.
    Sdp { text: String, info: DecodeInfo },
}

/// What [`extract_sdp`] looked at — content types only, never contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodeInfo {
    pub(crate) outer: String,
    pub(crate) selected: String,
    pub(crate) skipped: Vec<String>,
}

/// Why a body could not yield a session description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Rejection {
    /// A media type (top-level, or an SDP part's transfer encoding) this
    /// bridge cannot read as SDP.
    Unsupported { outer: String, offending: String },
    /// A well-formed multipart body with no session description in it.
    NoSession { outer: String, skipped: Vec<String> },
    /// Broken multipart framing, or over the depth/part limits.
    Malformed { outer: String, reason: &'static str },
    /// More than one session description: no basis to pick one.
    Ambiguous { outer: String, count: usize },
}

impl Rejection {
    /// The SIP status to refuse a request with: `415` (RFC 3261 §21.4.13)
    /// for a body we can't use, `400` (§21.4.1) for one that is broken.
    pub(crate) fn status(&self) -> u16 {
        match self {
            Self::Unsupported { .. } | Self::NoSession { .. } => 415,
            Self::Malformed { .. } | Self::Ambiguous { .. } => 400,
        }
    }

    fn outcome(&self) -> &'static str {
        match self {
            Self::Unsupported { .. } => "unsupported",
            Self::NoSession { .. } => "no_session",
            Self::Malformed { .. } => "malformed",
            Self::Ambiguous { .. } => "ambiguous",
        }
    }
}

impl fmt::Display for Rejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { outer, offending } if outer == offending => {
                write!(f, "unsupported body type {outer}")
            }
            Self::Unsupported { outer, offending } => {
                write!(f, "{outer} body has an unsupported part: {offending}")
            }
            Self::NoSession { outer, skipped } => write!(
                f,
                "{outer} body has no session description (parts: {})",
                skipped.join(", ")
            ),
            Self::Malformed { outer, reason } => write!(f, "malformed {outer} body: {reason}"),
            Self::Ambiguous { outer, count } => {
                write!(f, "{outer} body has {count} session descriptions")
            }
        }
    }
}

/// An SDP part with disposition `session` found while walking a multipart.
struct Candidate<'a> {
    content: &'a [u8],
}

/// State gathered while walking a multipart tree.
#[derive(Default)]
struct Walk {
    skipped: Vec<String>,
    skipped_required: bool,
    /// An SDP session part whose transfer encoding we don't decode.
    encoded_sdp: Option<String>,
}

impl Walk {
    fn skip(&mut self, part: &Part<'_>) {
        if part.disposition.handling == Handling::Required {
            self.skipped_required = true;
        }
        self.skipped.push(part.type_name());
    }
}

/// Every session SDP part under the multipart `media_type`/`content`, at
/// nesting `depth` (1 for the top-level body). `multipart/alternative` yields
/// the last alternative that has one (RFC 2046 §5.1.4); every other subtype
/// — `mixed`, `related`, or one this bridge has never heard of (RFC 5621 §4.2
/// treats those as `mixed`) — yields all of them.
fn sessions<'a>(
    media_type: &MediaType,
    content: &'a [u8],
    depth: usize,
    walk: &mut Walk,
) -> Result<Vec<Candidate<'a>>, &'static str> {
    if depth > MAX_DEPTH {
        return Err("multipart nested too deeply");
    }
    let boundary = media_type.param("boundary").ok_or("missing boundary")?;
    let parts = split_multipart(content, boundary)?;

    let mut per_part = Vec::with_capacity(parts.len());
    for part in &parts {
        per_part.push(part_sessions(part, depth, walk)?);
    }

    if media_type.subtype == "alternative" {
        let mut chosen = None;
        for (i, found) in per_part.iter().enumerate().rev() {
            if !found.is_empty() {
                chosen = Some(i);
                break;
            }
        }
        let Some(chosen) = chosen else {
            return Ok(Vec::new());
        };
        for (i, part) in parts.iter().enumerate() {
            if i != chosen && !per_part[i].is_empty() {
                walk.skipped.push(part.type_name());
            }
        }
        return Ok(per_part.swap_remove(chosen));
    }
    Ok(per_part.into_iter().flatten().collect())
}

/// The session SDP candidates inside one part: itself, its nested parts, or
/// none (recorded as skipped).
fn part_sessions<'a>(
    part: &Part<'a>,
    depth: usize,
    walk: &mut Walk,
) -> Result<Vec<Candidate<'a>>, &'static str> {
    match &part.media_type {
        Some(mt) if mt.is_multipart() => sessions(mt, part.content, depth + 1, walk),
        Some(mt) if mt.is_sdp() && part.disposition.kind == "session" => {
            let identity = part
                .transfer_encoding
                .as_deref()
                .is_none_or(|e| matches!(e, "7bit" | "8bit" | "binary"));
            if identity {
                Ok(vec![Candidate {
                    content: part.content,
                }])
            } else {
                walk.encoded_sdp = Some(format!(
                    "application/sdp; content-transfer-encoding={}",
                    part.transfer_encoding.as_deref().unwrap_or_default()
                ));
                Ok(Vec::new())
            }
        }
        _ => {
            walk.skip(part);
            Ok(Vec::new())
        }
    }
}

fn is_blank(body: &[u8]) -> bool {
    body.iter().all(u8::is_ascii_whitespace)
}

/// Finds the one session description in a SIP body.
///
/// `content_type` is the message's `Content-Type` header. A non-empty body
/// with no `Content-Type` is read as SDP — this bridge's long-standing
/// tolerance, kept deliberately despite RFC 3261 §20.15. A top-level
/// `application/sdp` is read whole whatever its parameters. A `multipart/*`
/// body is walked per the module docs. Anything else is
/// [`Rejection::Unsupported`].
///
/// Logs one line per multipart or refused body (outer type, selected type,
/// skipped types, outcome) — never part contents.
pub(crate) fn extract_sdp(content_type: Option<&str>, body: &[u8]) -> Result<Decoded, Rejection> {
    if is_blank(body) {
        return Ok(Decoded::Empty);
    }
    let Some(raw_type) = content_type else {
        return Ok(sdp_whole(body, "(none)"));
    };
    let Some(media_type) = MediaType::parse(raw_type) else {
        let outer = raw_type.trim().to_string();
        return log(
            Err(Rejection::Unsupported {
                offending: outer.clone(),
                outer,
            }),
            false,
        );
    };
    let outer = media_type.essence();
    if media_type.is_sdp() {
        return Ok(sdp_whole(body, &outer));
    }
    if !media_type.is_multipart() {
        return log(
            Err(Rejection::Unsupported {
                offending: outer.clone(),
                outer,
            }),
            false,
        );
    }

    let mut walk = Walk::default();
    let result = match sessions(&media_type, body, 1, &mut walk) {
        Err(reason) => Err(Rejection::Malformed {
            outer: outer.clone(),
            reason,
        }),
        Ok(found) => match found.as_slice() {
            [one] => Ok(Decoded::Sdp {
                text: String::from_utf8_lossy(one.content).into_owned(),
                info: DecodeInfo {
                    outer: outer.clone(),
                    selected: "application/sdp".to_string(),
                    skipped: walk.skipped.clone(),
                },
            }),
            [] => Err(match walk.encoded_sdp.take() {
                Some(offending) => Rejection::Unsupported {
                    outer: outer.clone(),
                    offending,
                },
                None => Rejection::NoSession {
                    outer: outer.clone(),
                    skipped: walk.skipped.clone(),
                },
            }),
            many => Err(Rejection::Ambiguous {
                outer: outer.clone(),
                count: many.len(),
            }),
        },
    };
    log(result, walk.skipped_required)
}

fn sdp_whole(body: &[u8], outer: &str) -> Decoded {
    Decoded::Sdp {
        text: String::from_utf8_lossy(body).into_owned(),
        info: DecodeInfo {
            outer: outer.to_string(),
            selected: "application/sdp".to_string(),
            skipped: Vec::new(),
        },
    }
}

/// The one diagnostic line per multipart or refused body (specs/088
/// research.md Decision 10). `warn` when a `required` part was skipped — the
/// RFC 5621 §8.4 deviation this module makes on purpose — `info` otherwise.
fn log(result: Result<Decoded, Rejection>, skipped_required: bool) -> Result<Decoded, Rejection> {
    match &result {
        Ok(Decoded::Sdp { info, .. }) => {
            if skipped_required {
                tracing::warn!(
                    outer = %info.outer,
                    selected = %info.selected,
                    skipped = ?info.skipped,
                    outcome = "sdp",
                    "SIP body: skipped a part marked handling=required that we do not understand"
                );
            } else {
                tracing::info!(
                    outer = %info.outer,
                    selected = %info.selected,
                    skipped = ?info.skipped,
                    outcome = "sdp",
                    "SIP body decoded"
                );
            }
        }
        Ok(Decoded::Empty) => {}
        Err(rejection) => tracing::info!(
            selected = "none",
            outcome = rejection.outcome(),
            reason = %rejection,
            "SIP body has no usable session description"
        ),
    }
    result
}

/// Synthetic fixtures shaped like the T2 (issue #103) INVITE body: a PIDF-LO
/// part first (`render;handling=optional`, a `Content-ID`, a per-part
/// `Content-Length` that is not used for framing), then the SDP part with no
/// disposition, a blank line before the close delimiter, and an unquoted
/// boundary. No real numbers, addresses or coordinates.
#[cfg(test)]
pub(crate) mod fixtures {
    pub(crate) const T2_CONTENT_TYPE: &str = "multipart/mixed;boundary=boundary1";

    /// The SDP the T2 body carries — what decoding must return byte for byte.
    pub(crate) const T2_SDP: &str = "v=0\r\n\
        o=- 1700000000 1700000000 IN IP4 203.0.113.30\r\n\
        s=-\r\n\
        c=IN IP4 203.0.113.30\r\n\
        b=AS:80\r\n\
        t=0 0\r\n\
        m=audio 21892 RTP/AVP 104 110 102 108 96 97\r\n\
        b=AS:80\r\n\
        a=rtpmap:104 AMR-WB/16000/1\r\n\
        a=fmtp:104 mode-change-capability=2;max-red=0\r\n\
        a=rtpmap:110 AMR-WB/16000/1\r\n\
        a=fmtp:110 octet-align=1;mode-change-capability=2;max-red=0\r\n\
        a=rtpmap:102 AMR/8000/1\r\n\
        a=fmtp:102 mode-change-capability=2;max-red=0\r\n\
        a=rtpmap:108 AMR/8000/1\r\n\
        a=fmtp:108 octet-align=1;mode-change-capability=2;max-red=0\r\n\
        a=rtpmap:96 telephone-event/16000\r\n\
        a=fmtp:96 0-15\r\n\
        a=rtpmap:97 telephone-event/8000\r\n\
        a=fmtp:97 0-15\r\n\
        a=curr:qos local sendrecv\r\n\
        a=curr:qos remote none\r\n\
        a=des:qos mandatory local sendrecv\r\n\
        a=des:qos optional remote sendrecv\r\n\
        a=sendrecv\r\n\
        a=ptime:20\r\n\
        a=maxptime:240\r\n";

    pub(crate) const T2_PIDF: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\
        <presence xmlns=\"urn:ietf:params:xml:ns:pidf\" \
        entity=\"pres:+919000000000@example.invalid\">\r\n\
        <tuple id=\"loc\"><status/></tuple>\r\n\
        </presence>";

    /// The full multipart body: PIDF first, SDP second.
    pub(crate) fn t2_body() -> String {
        format!(
            "--boundary1\r\n\
             Content-Type: application/pidf+xml\r\n\
             Content-ID: <loc1@example.invalid>\r\n\
             Content-Disposition: render;handling=optional\r\n\
             Content-Length: 9999\r\n\
             \r\n\
             {T2_PIDF}\r\n\
             --boundary1\r\n\
             Content-Type: application/sdp\r\n\
             Content-Length: {}\r\n\
             \r\n\
             {T2_SDP}\r\n\
             --boundary1--\r\n",
            T2_SDP.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    const SDP: &str = "v=0\r\nc=IN IP4 192.0.2.1\r\nm=audio 4000 RTP/AVP 0\r\n";
    const PIDF_PART: &str = "Content-Type: application/pidf+xml\r\n\
                             Content-Disposition: render;handling=optional\r\n\r\n<presence/>";

    fn sdp_part(sdp: &str) -> String {
        format!("Content-Type: application/sdp\r\n\r\n{sdp}")
    }

    /// `multipart/<subtype>` text with boundary `b` holding `parts` verbatim.
    fn multipart(parts: &[&str]) -> String {
        let mut out = String::new();
        for p in parts {
            out.push_str(&format!("--b\r\n{p}\r\n"));
        }
        out.push_str("--b--\r\n");
        out
    }

    fn decode(ct: &str, body: &str) -> Result<Decoded, Rejection> {
        extract_sdp(Some(ct), body.as_bytes())
    }

    fn sdp_text(result: Result<Decoded, Rejection>) -> String {
        match result {
            Ok(Decoded::Sdp { text, .. }) => text,
            other => panic!("expected SDP, got {other:?}"),
        }
    }

    // --- MediaType (T002) ---

    #[test]
    fn media_type_names_are_case_insensitive() {
        let mt = MediaType::parse("Multipart/MIXED; BOUNDARY=abc").unwrap();
        assert_eq!(
            (mt.type_.as_str(), mt.subtype.as_str()),
            ("multipart", "mixed")
        );
        assert!(mt.is("multipart/mixed"));
        assert!(mt.is("MULTIPART/Mixed"));
        assert!(mt.is_multipart());
        assert_eq!(mt.param("boundary"), Some("abc"));
        assert_eq!(mt.param("Boundary"), Some("abc"));
    }

    #[test]
    fn a_quoted_boundary_keeps_its_spaces_semicolons_and_escapes() {
        let mt = MediaType::parse(r#"multipart/mixed; boundary="a b;c\"d"; x=1"#).unwrap();
        assert_eq!(mt.param("boundary"), Some("a b;c\"d"));
        assert_eq!(mt.param("x"), Some("1"));
    }

    #[test]
    fn whitespace_around_separators_is_ignored() {
        let mt = MediaType::parse("  application / sdp ;  charset = utf-8  ").unwrap();
        assert!(mt.is("application/sdp"));
        assert_eq!(mt.param("charset"), Some("utf-8"));
    }

    #[test]
    fn a_media_type_without_a_subtype_does_not_parse() {
        assert_eq!(MediaType::parse("application"), None);
        assert_eq!(MediaType::parse("application/"), None);
        assert_eq!(MediaType::parse(""), None);
    }

    // --- Disposition (T003) ---

    #[test]
    fn disposition_defaults_follow_rfc_3261() {
        let sdp = MediaType::parse("application/sdp").unwrap();
        let pidf = MediaType::parse("application/pidf+xml").unwrap();
        assert_eq!(
            Disposition::parse(None, Some(&sdp)),
            Disposition {
                kind: "session".into(),
                handling: Handling::Required
            }
        );
        assert_eq!(Disposition::parse(None, Some(&pidf)).kind, "render");
        assert_eq!(Disposition::parse(None, None).kind, "render");
    }

    #[test]
    fn an_explicit_disposition_and_handling_are_read() {
        let d = Disposition::parse(Some("Render; Handling=Optional"), None);
        assert_eq!(d.kind, "render");
        assert_eq!(d.handling, Handling::Optional);
    }

    #[test]
    fn an_unknown_handling_value_is_required() {
        let d = Disposition::parse(Some("render;handling=maybe"), None);
        assert_eq!(d.handling, Handling::Required);
    }

    // --- Splitter (T004) ---

    #[test]
    fn two_crlf_parts_split_with_the_delimiter_crlf_stripped() {
        let body = "--b\r\nContent-Type: text/plain\r\n\r\none\r\n--b\r\n\r\ntwo\r\n--b--";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].content, b"one");
        assert_eq!(parts[0].type_name(), "text/plain");
        assert_eq!(parts[1].content, b"two");
        assert!(parts[1].media_type.is_none());
    }

    #[test]
    fn a_bare_lf_body_splits_the_same() {
        let body = "--b\nContent-Type: application/sdp\n\nv=0\n--b--\n";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].content, b"v=0");
    }

    #[test]
    fn preamble_and_epilogue_are_ignored() {
        let body = "This is a preamble.\r\n--b\r\n\r\nx\r\n--b--\r\nand an epilogue\r\n";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].content, b"x");
    }

    #[test]
    fn trailing_whitespace_on_a_delimiter_line_is_allowed() {
        let body = "--b  \r\n\r\nx\r\n--b-- \t\r\n";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts[0].content, b"x");
    }

    #[test]
    fn a_line_that_merely_starts_with_the_boundary_is_content() {
        let body = "--b\r\n\r\n--bogus\r\n--b--\r\n";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts[0].content, b"--bogus");
    }

    #[test]
    fn a_wrong_part_content_length_is_ignored() {
        let body = "--b\r\nContent-Length: 2\r\n\r\nlonger than two\r\n--b--";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts[0].content, b"longer than two");
    }

    #[test]
    fn a_folded_part_header_is_joined() {
        let body = "--b\r\nContent-Disposition: render;\r\n handling=optional\r\n\r\nx\r\n--b--";
        let parts = split_multipart(body.as_bytes(), "b").unwrap();
        assert_eq!(parts[0].disposition.handling, Handling::Optional);
    }

    #[test]
    fn a_missing_close_delimiter_is_malformed() {
        let body = "--b\r\n\r\nx\r\n";
        assert_eq!(
            split_multipart(body.as_bytes(), "b").unwrap_err(),
            "missing close delimiter"
        );
        assert!(matches!(
            decode("multipart/mixed;boundary=b", body),
            Err(Rejection::Malformed { .. })
        ));
    }

    #[test]
    fn an_empty_or_overlong_boundary_is_malformed() {
        let body = multipart(&[&sdp_part(SDP)]);
        assert!(matches!(
            decode("multipart/mixed;boundary=\"\"", &body),
            Err(Rejection::Malformed { .. })
        ));
        let long = "x".repeat(71);
        assert!(matches!(
            decode(&format!("multipart/mixed;boundary={long}"), &body),
            Err(Rejection::Malformed { .. })
        ));
        assert!(matches!(
            decode("multipart/mixed", &body),
            Err(Rejection::Malformed {
                reason: "missing boundary",
                ..
            })
        ));
    }

    #[test]
    fn seventeen_parts_are_malformed_sixteen_are_not() {
        let filler = "Content-Type: text/plain\r\n\r\nx";
        let mut sixteen = vec![filler; 15];
        let sdp = sdp_part(SDP);
        sixteen.push(&sdp);
        assert!(decode("multipart/mixed;boundary=b", &multipart(&sixteen)).is_ok());
        let mut seventeen = sixteen.clone();
        seventeen.push(filler);
        assert_eq!(
            decode("multipart/mixed;boundary=b", &multipart(&seventeen)),
            Err(Rejection::Malformed {
                outer: "multipart/mixed".into(),
                reason: "too many parts"
            })
        );
    }

    /// `levels` multiparts nested inside each other, the SDP in the innermost.
    fn nested(levels: usize) -> (String, String) {
        let mut body = format!("--b{levels}\r\n{}\r\n--b{levels}--", sdp_part(SDP));
        for level in (1..levels).rev() {
            body = format!(
                "--b{level}\r\nContent-Type: multipart/mixed;boundary=b{}\r\n\r\n{body}\r\n--b{level}--",
                level + 1
            );
        }
        ("multipart/mixed;boundary=b1".to_string(), body)
    }

    #[test]
    fn nesting_depth_four_is_accepted_five_is_malformed() {
        let (ct, body) = nested(4);
        assert_eq!(sdp_text(decode(&ct, &body)), SDP);
        let (ct, body) = nested(5);
        assert!(matches!(
            decode(&ct, &body),
            Err(Rejection::Malformed {
                reason: "multipart nested too deeply",
                ..
            })
        ));
    }

    // --- extract_sdp (T005) ---

    #[test]
    fn an_empty_or_whitespace_body_is_empty() {
        assert_eq!(extract_sdp(None, b""), Ok(Decoded::Empty));
        assert_eq!(
            extract_sdp(Some("application/sdp"), b"\r\n \r\n"),
            Ok(Decoded::Empty)
        );
        assert_eq!(extract_sdp(Some("text/plain"), b""), Ok(Decoded::Empty));
    }

    #[test]
    fn a_body_with_no_content_type_is_read_as_sdp() {
        assert_eq!(sdp_text(extract_sdp(None, SDP.as_bytes())), SDP);
    }

    #[test]
    fn a_parameterised_application_sdp_is_sdp() {
        assert_eq!(sdp_text(decode("application/sdp;charset=x", SDP)), SDP);
        assert_eq!(sdp_text(decode("APPLICATION/SDP", SDP)), SDP);
    }

    #[test]
    fn the_t2_body_yields_its_sdp_part_byte_for_byte() {
        let result = extract_sdp(Some(T2_CONTENT_TYPE), t2_body().as_bytes());
        assert_eq!(
            result,
            Ok(Decoded::Sdp {
                text: T2_SDP.to_string(),
                info: DecodeInfo {
                    outer: "multipart/mixed".into(),
                    selected: "application/sdp".into(),
                    skipped: vec!["application/pidf+xml".into()],
                },
            })
        );
    }

    #[test]
    fn the_t2_parts_reversed_yield_the_same_sdp() {
        let body = format!(
            "--boundary1\r\nContent-Type: application/sdp\r\n\r\n{T2_SDP}\r\n\
             --boundary1\r\nContent-Type: application/pidf+xml\r\n\
             Content-Disposition: render;handling=optional\r\n\r\n{T2_PIDF}\r\n\
             --boundary1--\r\n"
        );
        assert_eq!(sdp_text(decode(T2_CONTENT_TYPE, &body)), T2_SDP);
    }

    #[test]
    fn a_nested_mixed_inside_mixed_is_searched() {
        let inner = multipart(&[PIDF_PART, &sdp_part(SDP)]).replace("--b", "--inner");
        let outer = format!(
            "--b\r\n{PIDF_PART}\r\n--b\r\nContent-Type: multipart/mixed; boundary=inner\r\n\r\n{inner}\r\n--b--"
        );
        let result = decode("multipart/mixed;boundary=b", &outer);
        match result {
            Ok(Decoded::Sdp { text, info }) => {
                assert_eq!(text, SDP);
                assert_eq!(info.skipped.len(), 2);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unknown_multipart_subtype_is_treated_as_mixed() {
        let body = multipart(&[PIDF_PART, &sdp_part(SDP)]);
        assert_eq!(
            sdp_text(decode("multipart/x-vendor;boundary=b", &body)),
            SDP
        );
        assert_eq!(sdp_text(decode("multipart/related;boundary=b", &body)), SDP);
    }

    #[test]
    fn alternative_picks_the_last_usable_part() {
        let first = "v=0\r\nc=IN IP4 192.0.2.1\r\nm=audio 1 RTP/AVP 0\r\n";
        let last = "v=0\r\nc=IN IP4 192.0.2.2\r\nm=audio 2 RTP/AVP 0\r\n";
        let body = multipart(&[&sdp_part(first), &sdp_part(last), PIDF_PART]);
        assert_eq!(
            sdp_text(decode("multipart/alternative;boundary=b", &body)),
            last
        );
    }

    #[test]
    fn an_unknown_required_part_is_skipped_not_refused() {
        // Clarification Q1: lenient, logged at warn.
        let required = "Content-Type: application/vnd.example+xml\r\n\r\n<x/>";
        let body = multipart(&[required, &sdp_part(SDP)]);
        assert_eq!(sdp_text(decode("multipart/mixed;boundary=b", &body)), SDP);
    }

    #[test]
    fn two_session_descriptions_are_ambiguous() {
        let body = multipart(&[&sdp_part(SDP), &sdp_part(SDP)]);
        let err = decode("multipart/mixed;boundary=b", &body).unwrap_err();
        assert_eq!(
            err,
            Rejection::Ambiguous {
                outer: "multipart/mixed".into(),
                count: 2
            }
        );
        assert_eq!(err.status(), 400);
    }

    #[test]
    fn only_unknown_parts_is_no_session() {
        let body = multipart(&[PIDF_PART]);
        let err = decode("multipart/mixed;boundary=b", &body).unwrap_err();
        assert_eq!(
            err,
            Rejection::NoSession {
                outer: "multipart/mixed".into(),
                skipped: vec!["application/pidf+xml".into()]
            }
        );
        assert_eq!(err.status(), 415);
    }

    #[test]
    fn an_sdp_part_with_render_disposition_is_not_the_session() {
        let rendered =
            format!("Content-Type: application/sdp\r\nContent-Disposition: render\r\n\r\n{SDP}");
        let body = multipart(&[&rendered]);
        assert!(matches!(
            decode("multipart/mixed;boundary=b", &body),
            Err(Rejection::NoSession { .. })
        ));
    }

    #[test]
    fn a_base64_sdp_part_is_unsupported() {
        let encoded =
            "Content-Type: application/sdp\r\nContent-Transfer-Encoding: BASE64\r\n\r\ndj0w";
        let body = multipart(&[encoded]);
        let err = decode("multipart/mixed;boundary=b", &body).unwrap_err();
        assert!(matches!(err, Rejection::Unsupported { .. }), "{err:?}");
        assert_eq!(err.status(), 415);
        // The identity encodings are fine.
        let binary = format!(
            "Content-Type: application/sdp\r\nContent-Transfer-Encoding: binary\r\n\r\n{SDP}"
        );
        assert_eq!(
            sdp_text(decode("multipart/mixed;boundary=b", &multipart(&[&binary]))),
            SDP
        );
    }

    #[test]
    fn a_top_level_non_sdp_type_is_unsupported() {
        let err = decode("text/plain", "hello").unwrap_err();
        assert_eq!(
            err,
            Rejection::Unsupported {
                outer: "text/plain".into(),
                offending: "text/plain".into()
            }
        );
        assert_eq!(err.status(), 415);
        assert_eq!(decode("garbage", "hello").unwrap_err().status(), 415);
    }

    #[test]
    fn rejection_statuses_match_the_contract() {
        let outer = String::new();
        assert_eq!(
            Rejection::Malformed {
                outer: outer.clone(),
                reason: "x"
            }
            .status(),
            400
        );
        assert_eq!(
            Rejection::NoSession {
                outer,
                skipped: vec![]
            }
            .status(),
            415
        );
    }
}
