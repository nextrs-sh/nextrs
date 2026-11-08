//! `hasLocalMatch` / `hasRemoteMatch` with the regular expressions the
//! build compiled from `images.localPatterns` / `remotePatterns`
//! (`images-manifest.json`; picomatch's `makeRe(..).source`, as next start
//! compiles them per request).

use fancy_regex::Regex;
use nextrs_adapter::ImagesConfig;

use crate::routing::js_regex;

struct Local {
    pathname: Regex,
    search: Option<String>,
}

struct Remote {
    protocol: Option<String>,
    hostname: Regex,
    port: Option<String>,
    pathname: Regex,
    search: Option<String>,
}

pub(crate) struct Patterns {
    /// `None`: no `localPatterns`, every local path is allowed.
    local: Option<Vec<Local>>,
    remote: Vec<Remote>,
    domains: Vec<String>,
}

/// JavaScript's `URL#search`: empty for no query or a bare `?`.
fn search(url: &url::Url) -> String {
    match url.query() {
        Some(q) if !q.is_empty() => format!("?{q}"),
        _ => String::new(),
    }
}

fn matches(regex: &Regex, text: &str) -> bool {
    // A regex that fails to run (backtracking limit) does not match.
    regex.is_match(text).unwrap_or(false)
}

impl Patterns {
    pub(crate) fn compile(config: &ImagesConfig) -> Result<Self, String> {
        let local = match &config.local_patterns {
            None => None,
            Some(patterns) => Some(
                patterns
                    .iter()
                    .map(|p| {
                        Ok(Local {
                            pathname: js_regex(&p.pathname, false)?,
                            search: p.search.clone(),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            ),
        };
        let remote = config
            .remote_patterns
            .iter()
            .map(|p| {
                Ok(Remote {
                    protocol: p.protocol.clone(),
                    hostname: js_regex(&p.hostname, false)?,
                    port: p.port.clone(),
                    pathname: js_regex(&p.pathname, false)?,
                    search: p.search.clone(),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            local,
            remote,
            domains: config.domains.clone(),
        })
    }

    /// `hasLocalMatch(localPatterns, url)` with `new URL(url, 'http://n')`.
    pub(crate) fn local_match(&self, url: &str) -> bool {
        let Some(local) = &self.local else {
            return true;
        };
        let Ok(parsed) = url::Url::parse("http://n").and_then(|base| base.join(url)) else {
            return false;
        };
        let search = search(&parsed);
        local.iter().any(|p| {
            p.search.as_ref().is_none_or(|s| *s == search) && matches(&p.pathname, parsed.path())
        })
    }

    /// `hasRemoteMatch(domains, remotePatterns, url)`.
    pub(crate) fn remote_match(&self, url: &url::Url) -> bool {
        let hostname = url.host_str().unwrap_or("");
        if self.domains.iter().any(|d| d == hostname) {
            return true;
        }
        // `URL#port`: empty for the scheme's default port.
        let port = url.port().map(|p| p.to_string()).unwrap_or_default();
        let search = search(url);
        self.remote.iter().any(|p| {
            p.protocol
                .as_ref()
                .is_none_or(|proto| proto.trim_end_matches(':') == url.scheme())
                && p.port.as_ref().is_none_or(|want| *want == port)
                && matches(&p.hostname, hostname)
                && p.search.as_ref().is_none_or(|s| *s == search)
                && matches(&p.pathname, url.path())
        })
    }
}
