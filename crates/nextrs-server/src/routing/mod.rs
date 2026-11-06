//! Next's routing rules at request time: the header, redirect and rewrite
//! rules of `routes-manifest.json` and the proxy's matchers, applied the
//! way `next start` applies them (`resolve-routes.js`, `filesystem.js`).
//!
//! Matching uses the regexes Next wrote into the manifests (through
//! `fancy-regex`: they use lookarounds); the rest (params, `has`, and
//! building destinations) is a port of Next's own functions, checked
//! against them by `tests/routing_vectors.rs`.

pub mod destination;
pub mod path_to_regexp;
pub mod query;
pub mod url;

use fancy_regex::{Regex, RegexBuilder};
use nextrs_adapter::{HasSpec, ProxyMatcher, RuleSpec};

use self::destination::{HasCondition, HasKind, RequestView, match_has};
use self::path_to_regexp::Key;
use self::query::{JsMap, decode_uri_component};

/// Which list a rule came from; decides case sensitivity and Next's
/// treatment of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    Header,
    Redirect,
    BeforeFiles,
    AfterFiles,
    Fallback,
}

#[derive(Debug)]
pub struct Rule {
    pub kind: RuleKind,
    pub source: String,
    regex: Regex,
    keys: Vec<Key>,
    has: Vec<HasCondition>,
    missing: Vec<HasCondition>,
    pub destination: Option<String>,
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
}

fn has_conditions(specs: &[HasSpec]) -> Result<Vec<HasCondition>, String> {
    specs
        .iter()
        .map(|spec| {
            let kind = match spec.kind.as_str() {
                "header" => HasKind::Header,
                "cookie" => HasKind::Cookie,
                "query" => HasKind::Query,
                "host" => HasKind::Host,
                other => return Err(format!("unknown has/missing type {other:?}")),
            };
            HasCondition::new(kind, spec.key.as_deref(), spec.value.as_deref())
        })
        .collect()
}

/// A JavaScript regex source compiled as `new RegExp(source, flags)`.
fn js_regex(source: &str, case_insensitive: bool) -> Result<Regex, String> {
    RegexBuilder::new(source)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|error| format!("regex {source:?}: {error}"))
}

impl Rule {
    /// `buildCustomRoute`: the manifest regex, case-insensitive unless
    /// `caseSensitiveRoutes` (beforeFiles rewrites always are), with the
    /// keys `pathToRegexp` finds in `source`.
    pub fn compile(kind: RuleKind, spec: &RuleSpec, case_sensitive: bool) -> Result<Self, String> {
        let sensitive = case_sensitive && kind != RuleKind::BeforeFiles;
        let regex = js_regex(&spec.regex, !sensitive)?;
        let tokens = path_to_regexp::parse(&spec.source)
            .map_err(|error| format!("source {:?}: {error}", spec.source))?;
        let keys = path_to_regexp::keys(&tokens);
        let groups = regex.captures_len() - 1;
        if groups != keys.len() {
            return Err(format!(
                "source {:?}: its regex has {groups} groups for {} keys",
                spec.source,
                keys.len()
            ));
        }
        Ok(Self {
            kind,
            source: spec.source.clone(),
            regex,
            keys,
            has: has_conditions(&spec.has)?,
            missing: has_conditions(&spec.missing)?,
            destination: spec.destination.clone(),
            status: spec.status,
            headers: spec.headers.clone(),
        })
    }

    /// Whether matching reads the request (`has` / `missing`).
    pub fn has_conditions(&self) -> bool {
        !self.has.is_empty() || !self.missing.is_empty()
    }

    /// `route.match(pathname)` then `matchHas`: the params, or `None`.
    pub fn matches(&self, pathname: &str, request: &RequestView<'_>) -> Option<JsMap> {
        // A plain match first: it runs on a DFA, captures need a slower
        // engine, and most requests match no rule.
        if !self.regex.is_match(pathname).unwrap_or(false) {
            return None;
        }
        let captures = self.regex.captures(pathname).ok()??;
        let mut params = path_to_regexp::params_from_captures(&captures, &self.keys, true);
        if !self.has.is_empty() || !self.missing.is_empty() {
            let extra = match_has(request, &self.has, &self.missing)?;
            params.assign(&extra);
        }
        Some(params)
    }
}

/// `getMiddlewareRouteMatcher`: case-sensitive regexes with optional
/// conditions; a request matches when any matcher does.
#[derive(Debug)]
pub struct ProxyMatchers {
    matchers: Vec<(Regex, Vec<HasCondition>, Vec<HasCondition>)>,
}

impl ProxyMatchers {
    pub fn compile(specs: &[ProxyMatcher]) -> Result<Self, String> {
        let matchers = specs
            .iter()
            .map(|spec| {
                Ok((
                    js_regex(&spec.regexp, false)?,
                    has_conditions(&spec.has)?,
                    has_conditions(&spec.missing)?,
                ))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { matchers })
    }

    /// One pathname against the matchers (`getMiddlewareRouteMatcher`).
    pub fn matches_path(&self, pathname: &str, request: &RequestView<'_>) -> bool {
        self.matchers.iter().any(|(regex, has, missing)| {
            regex.is_match(pathname).unwrap_or(false)
                && ((has.is_empty() && missing.is_empty())
                    || match_has(request, has, missing).is_some())
        })
    }

    /// resolve-routes: the pathname as routed, or its decoded form.
    pub fn matches(&self, pathname: &str, request: &RequestView<'_>) -> bool {
        if self.matches_path(pathname, request) {
            return true;
        }
        match decode_uri_component(pathname) {
            Some(decoded) if decoded != pathname => self.matches_path(&decoded, request),
            _ => false,
        }
    }
}
