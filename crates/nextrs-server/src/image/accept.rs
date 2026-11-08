//! `getSupportedMimeType` from Next's image optimizer: `@hapi/accept`'s
//! `mediaType(accept, formats)`, kept only when the raw header contains the
//! chosen type literally.

use std::cmp::Ordering;
use std::collections::HashMap;

/// The negotiated output type, or `""` (keep the source's type).
/// `Err` where hapi throws (a parameter without a value): next start
/// answers 500.
pub(crate) fn supported_mime_type(formats: &[String], accept: Option<&str>) -> Result<String, ()> {
    let header = accept.unwrap_or("");
    if formats.is_empty() {
        return Ok(String::new());
    }
    let chosen = media_type(accept, formats)?;
    if chosen.is_empty() || !header.contains(chosen.as_str()) {
        return Ok(String::new());
    }
    Ok(chosen)
}

#[derive(Clone)]
struct Range {
    token: String,
    kind: String,
    subtype: String,
    original: String,
    specificity: usize,
    q: f64,
    pos: usize,
}

fn valid_media(token: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    // hapi's pattern, alternation precedence included: only the first
    // branch is anchored at the start and only the last at the end.
    RE.get_or_init(|| {
        let t = r"[A-Za-z0-9_!#$%&'*+\-.^`|~]";
        regex::Regex::new(&format!(r"^(?:\*/\*)|(?:{t}+/\*)|(?:{t}+/{t}+)$")).expect("valid")
    })
    .is_match(token)
}

/// `parseFloat`: the longest decimal prefix, else NaN.
fn parse_float(s: &str) -> f64 {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    if end < bytes.len() && (bytes[end] == b'+' || bytes[end] == b'-') {
        end += 1;
    }
    while end < bytes.len() {
        match bytes[end] {
            b'0'..=b'9' => seen_digit = true,
            b'.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        end += 1;
    }
    if !seen_digit {
        return if s.starts_with("Infinity") {
            f64::INFINITY
        } else {
            f64::NAN
        };
    }
    // An exponent, if complete.
    let mut stop = end;
    if stop < bytes.len() && (bytes[stop] == b'e' || bytes[stop] == b'E') {
        let mut k = stop + 1;
        if k < bytes.len() && (bytes[k] == b'+' || bytes[k] == b'-') {
            k += 1;
        }
        let digits = k;
        while k < bytes.len() && bytes[k].is_ascii_digit() {
            k += 1;
        }
        if k > digits {
            stop = k;
        }
    }
    s[..stop].parse().unwrap_or(f64::NAN)
}

fn media_type(accept: Option<&str>, preferences: &[String]) -> Result<String, ()> {
    // normalize(): quoted values aside, then spaces and tabs removed.
    let raw = match accept {
        Some(h) if !h.is_empty() => h,
        _ => "*/*",
    };
    let mut quoted: HashMap<String, String> = HashMap::new();
    let mut header = String::new();
    if raw.contains('"') {
        let re = regex::Regex::new(r#"="([^"]*)""#).expect("valid");
        let mut n = 0;
        let mut last = 0;
        for cap in re.captures_iter(raw) {
            let m = cap.get(0).expect("match");
            n += 1;
            let key = format!("\"{n}");
            quoted.insert(key.clone(), cap[1].to_owned());
            header.push_str(&raw[last..m.start()]);
            header.push('=');
            header.push_str(&key);
            last = m.end();
        }
        header.push_str(&raw[last..]);
    } else {
        header.push_str(raw);
    }
    let header: String = header.chars().filter(|c| *c != ' ' && *c != '\t').collect();

    let mut all: HashMap<String, Range> = HashMap::new();
    let mut accepted: Vec<Range> = Vec::new();
    for (pos, part) in header.split(',').enumerate() {
        if part.is_empty() {
            continue;
        }
        let mut pieces = part.split(';');
        let token = pieces.next().unwrap_or("").to_lowercase();
        if !valid_media(&token) {
            continue;
        }
        let mut params: Vec<(String, String)> = Vec::new();
        let mut in_exts = false;
        let mut q = None;
        for piece in pieces {
            let kv: Vec<&str> = piece.split('=').collect();
            if kv.len() != 2 || kv[1].is_empty() {
                return Err(());
            }
            let (key, value) = (kv[0], kv[1]);
            if key == "q" || key == "Q" {
                in_exts = true;
                let mut v = parse_float(value);
                if !v.is_finite() || v > 1.0 || (v < 0.001 && v != 0.0) {
                    v = 1.0;
                }
                q = Some(v);
            } else if !in_exts {
                let value = if value.starts_with('"') {
                    format!("\"{}\"", quoted.get(value).cloned().unwrap_or_default())
                } else {
                    value.to_owned()
                };
                // Object keys: a repeated parameter keeps its first position.
                if let Some(slot) = params.iter_mut().find(|(k, _)| k == key) {
                    slot.1 = value;
                } else {
                    params.push((key.to_owned(), value));
                }
            }
        }
        let original: String = params.iter().map(|(k, v)| format!(";{k}={v}")).collect();
        let (kind, subtype) = token.split_once('/').unwrap_or((token.as_str(), ""));
        let range = Range {
            kind: kind.to_owned(),
            subtype: subtype.to_owned(),
            token: token.clone(),
            original,
            specificity: params.len(),
            q: q.unwrap_or(1.0),
            pos,
        };
        all.insert(token, range.clone());
        if range.q != 0.0 {
            accepted.push(range);
        }
    }
    accepted.sort_by(|e, t| {
        if t.q != e.q {
            return t.q.partial_cmp(&e.q).unwrap_or(Ordering::Equal);
        }
        let inner = |a: &str, b: &str| {
            if a == "*" {
                Ordering::Greater
            } else if b == "*" {
                Ordering::Less
            } else {
                a.cmp(b)
            }
        };
        if e.kind != t.kind {
            return inner(&e.kind, &t.kind);
        }
        if e.subtype != t.subtype {
            return inner(&e.subtype, &t.subtype);
        }
        if e.specificity != t.specificity {
            return t.specificity.cmp(&e.specificity);
        }
        e.pos.cmp(&t.pos)
    });

    // preferences()
    let mut by_type: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut lower: Vec<(String, String)> = Vec::new();
    let mut any = false;
    for preference in preferences {
        let t = preference.to_lowercase();
        if let Some(slot) = lower.iter_mut().find(|(k, _)| *k == t) {
            slot.1 = preference.clone();
        } else {
            lower.push((t.clone(), preference.clone()));
        }
        let (kind, subtype) = t.split_once('/').unwrap_or((t.as_str(), ""));
        if kind == "*" {
            any = true;
            continue;
        }
        let subs = by_type.entry(kind.to_owned()).or_default();
        if let Some(slot) = subs.iter_mut().find(|(k, _)| k == subtype) {
            slot.1 = preference.clone();
        } else {
            subs.push((subtype.to_owned(), preference.clone()));
        }
    }
    let lookup = |key: &str| lower.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let mut out: Vec<String> = Vec::new();
    for range in &accepted {
        let t = &range.token;
        let entry = &all[t];
        let (kind, subtype) = (&entry.kind, &entry.subtype);
        let subs = by_type.get(kind.as_str());
        if kind == "*" {
            for (key, value) in &lower {
                if !all.contains_key(key) {
                    out.push(value.clone());
                }
            }
            if any {
                out.push("*/*".into());
            }
            continue;
        }
        if any {
            out.push(format!(
                "{}{}",
                lookup(t).unwrap_or_else(|| t.clone()),
                range.original
            ));
            continue;
        }
        if subtype != "*" {
            let explicit = lookup(t);
            let wildcard = subs.is_some_and(|s| s.iter().any(|(k, _)| k == "*"));
            if explicit.is_some() || wildcard {
                out.push(format!(
                    "{}{}",
                    explicit.unwrap_or_else(|| t.clone()),
                    range.original
                ));
            }
            continue;
        }
        if let Some(subs) = subs {
            for (sub, value) in subs {
                if !all.contains_key(&format!("{kind}/{sub}")) {
                    out.push(value.clone());
                }
            }
        }
    }
    Ok(out.into_iter().next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(accept: Option<&str>, formats: &[&str]) -> String {
        let formats: Vec<String> = formats.iter().map(|s| s.to_string()).collect();
        supported_mime_type(&formats, accept).unwrap()
    }

    // Expected values from Next 16.4.3:
    //   accept.mediaType(h, formats), then `h.includes(type) ? type : ''`.
    #[test]
    fn matches_next() {
        let both = ["image/avif", "image/webp"];
        let webp = ["image/webp"];
        assert_eq!(pick(Some("image/avif,image/webp,*/*"), &both), "image/avif");
        assert_eq!(pick(Some("image/avif,image/webp,*/*"), &webp), "image/webp");
        assert_eq!(pick(Some("image/webp;q=0,image/*"), &both), "");
        assert_eq!(pick(Some("image/webp;q=0,image/*"), &webp), "");
        assert_eq!(pick(Some("*/*"), &both), "");
        assert_eq!(pick(Some("image/png,image/*;q=0.8"), &both), "");
        assert_eq!(
            pick(Some("image/webp;q=0.5,image/avif;q=0.9"), &both),
            "image/avif"
        );
        assert_eq!(
            pick(Some("image/webp;q=0.5,image/avif;q=0.9"), &webp),
            "image/webp"
        );
        assert_eq!(pick(Some("image/WEBP"), &webp), "");
        assert_eq!(pick(Some("image/webp "), &webp), "image/webp");
        assert_eq!(
            pick(
                Some("image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8"),
                &both
            ),
            "image/avif"
        );
        assert_eq!(pick(Some("image/webp,*/*"), &both), "image/webp");
        assert_eq!(pick(None, &both), "");
        assert_eq!(pick(Some(""), &webp), "");
    }

    #[test]
    fn parameter_without_value_throws() {
        let formats = vec!["image/webp".to_owned()];
        assert!(supported_mime_type(&formats, Some("image/webp;q")).is_err());
        assert!(supported_mime_type(&formats, Some("image/webp;q=")).is_err());
    }

    #[test]
    fn q_values() {
        assert_eq!(parse_float("0.5"), 0.5);
        assert_eq!(parse_float(".5x"), 0.5);
        assert!(parse_float("x").is_nan());
        assert_eq!(parse_float("1e-1"), 0.1);
        assert_eq!(parse_float("1e"), 1.0);
    }
}
