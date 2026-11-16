//! picomatch's `makeRe(glob, { dot })`, for the glob syntax `next.config`
//! image patterns use: literal characters, `/`, `?`, `*` and `**`.
//!
//! Next.js compiles `images.localPatterns[].pathname` (`{ dot: true }`) and
//! `remotePatterns[].hostname` (no options) / `.pathname` (`{ dot: true }`)
//! with picomatch on every image request; `next build` stores the same
//! compiled sources in `images-manifest.json`. `output: 'standalone'` does
//! not copy that manifest, so a standalone build is read from
//! `required-server-files.json#config.images`, whose patterns are still
//! globs. The regular expressions built here are not picomatch's text, but
//! accept exactly the strings picomatch's do (`tests/data/picomatch-*.json`
//! holds picomatch's own answers for a corpus of patterns and inputs).
//! Brace, bracket, extglob, negation and escape syntax is refused: a build
//! using it cannot be served without the manifest.

/// A segment of `.` or `..` never matches a wildcard with `dot: true`.
const DOT_SEGMENT_GUARD: &str = r"(?!\.{1,2}(?:\/|$))";
/// Without `dot`, a wildcard never matches a leading `.`.
const NO_DOT_GUARD: &str = r"(?!\.)";
/// Any characters, `/` included, but no `.`/`..` segment.
const GLOBSTAR_DOT: &str = r"(?:(?:(?!(?:^|\/)\.{1,2}(?:\/|$)).)*?)";
/// Any characters, `/` included, but no segment starting with `.`.
const GLOBSTAR_NO_DOT: &str = r"(?:(?:(?!(?:^|\/)\.).)*?)";

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Literal(char),
    /// `?`: one character, not `/`.
    One,
    /// `*`: characters up to the next `/`.
    Star,
}

enum Segment {
    /// A whole `**` segment.
    Globstar,
    Tokens(Vec<Token>),
}

fn tokenize(segment: &str) -> Result<Segment, String> {
    if segment == "**" {
        return Ok(Segment::Globstar);
    }
    let mut tokens = Vec::new();
    let mut chars = segment.chars().peekable();
    while let Some(c) = chars.next() {
        // `*(`, `?(`, `+(`, `@(`, `!(`: extglobs.
        if "*?+@!".contains(c) && chars.peek() == Some(&'(') {
            return Err(format!("'{c}(' (extglob syntax)"));
        }
        match c {
            '*' => {
                if chars.peek() == Some(&'*') {
                    chars.next();
                    if chars.peek() == Some(&'*') {
                        return Err("three or more consecutive '*'".into());
                    }
                    // picomatch treats `**` that is not a whole segment as
                    // `*` (`/a/**.png` does not match `/a/b/x.png`).
                    tokens.push(Token::Star);
                } else {
                    tokens.push(Token::Star);
                }
            }
            '?' => tokens.push(Token::One),
            '[' | ']' | '{' | '}' | '(' | ')' | '\\' | '"' | '\'' => {
                return Err(format!("'{c}' (brace, bracket, group or escape syntax)"));
            }
            c => tokens.push(Token::Literal(c)),
        }
    }
    Ok(Segment::Tokens(tokens))
}

fn escape(c: char, out: &mut String) {
    if "\\^$.|?*+()[]{}/".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

fn segment_regex(tokens: &[Token], dot: bool) -> String {
    let guard = if dot { DOT_SEGMENT_GUARD } else { NO_DOT_GUARD };
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        let first = i == 0;
        match token {
            Token::Literal(c) => escape(*c, &mut out),
            Token::One => {
                if first && !dot {
                    out.push_str(NO_DOT_GUARD);
                }
                out.push_str("[^/]");
            }
            Token::Star => {
                if first {
                    out.push_str(guard);
                    out.push_str("(?=.)");
                }
                out.push_str("[^/]*?");
            }
        }
    }
    out
}

/// A JavaScript (and Rust `regex`) source matching what
/// `picomatch.makeRe(glob, { dot }).test(input)` accepts.
pub(crate) fn make_re(glob: &str, dot: bool) -> Result<String, String> {
    if glob.is_empty() {
        return Err("empty pattern".into());
    }
    if glob.starts_with('!') {
        return Err(format!("negated pattern {glob:?}"));
    }
    // A pathname is always absolute; picomatch compiles relative patterns
    // other than `**` through special cases (fast paths with their own
    // trailing-slash and dot rules) this does not model.
    if dot && !glob.starts_with('/') && glob != "**" {
        return Err(format!(
            "relative pathname pattern {glob:?} (only \"**\" or an absolute pattern)"
        ));
    }
    let segments = glob
        .split('/')
        .map(tokenize)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|why| format!("unsupported glob syntax in {glob:?}: {why}"))?;
    let guard = if dot { DOT_SEGMENT_GUARD } else { NO_DOT_GUARD };
    let globstar = if dot { GLOBSTAR_DOT } else { GLOBSTAR_NO_DOT };
    let any = format!("{guard}{globstar}");

    let mut out = String::from("^(?:");
    let last = segments.len() - 1;
    // Whether the pattern so far is only a leading "/" (the empty segment).
    let mut prefix_empty = true;
    for (i, segment) in segments.iter().enumerate() {
        match segment {
            Segment::Globstar => {
                let next_is_globstar = segments
                    .get(i + 1)
                    .is_some_and(|s| matches!(s, Segment::Globstar));
                if next_is_globstar {
                    // "**/**" is "**".
                    continue;
                }
                if i == 0 {
                    if i == last {
                        // "**": everything, an optional trailing slash.
                        out.push_str(&any);
                        out.push_str(r"\/?");
                    } else {
                        // "**/rest": zero or more leading segments.
                        out.push_str(&format!(r"(?:{any}\/)?"));
                    }
                } else if i == last {
                    if prefix_empty {
                        // "/**": a slash, then anything.
                        out.push_str(&format!(r"\/{any}"));
                    } else {
                        // "prefix/**": the prefix itself, or below it.
                        out.push_str(&format!(r"(?:\/{any}|$)"));
                    }
                } else if prefix_empty {
                    // "/**/rest": the globstar segment is required (it
                    // may be empty: `//rest`).
                    out.push_str(&format!(r"\/{any}\/"));
                } else {
                    // "prefix/**/rest": one slash, or slash-segments-slash.
                    out.push_str(&format!(r"(?:\/{any}\/|\/)"));
                }
            }
            Segment::Tokens(tokens) => {
                let after_globstar = i > 0 && matches!(segments[i - 1], Segment::Globstar);
                if i > 0 && !after_globstar {
                    out.push_str(r"\/");
                }
                out.push_str(&segment_regex(tokens, dot));
                if !tokens.is_empty() {
                    prefix_empty = false;
                }
                // picomatch lets a pattern ending in a wildcard match a
                // trailing slash too.
                if i == last && matches!(tokens.last(), Some(Token::Star)) {
                    out.push_str(r"\/?");
                }
            }
        }
    }
    out.push_str(")$");
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Differential tests: picomatch 4 (next/dist/compiled/picomatch from
    //! Next.js 16.4.6) answered every (pattern, input) pair in the corpus;
    //! the regular expressions built here must give the same answers.
    use super::*;
    use serde::Deserialize;

    /// Written by tests/data/picomatch-corpus.cjs.
    #[derive(Deserialize)]
    struct Corpus {
        paths: Vec<String>,
        hosts: Vec<String>,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        pattern: String,
        /// Pathname patterns (`dot: true`) answer for `paths`; hostname
        /// patterns (no options) for `hosts`.
        dot: bool,
        /// picomatch's answer per input, `'1'` for a match.
        answers: String,
    }

    #[test]
    fn agrees_with_picomatch_on_the_corpus() {
        let corpus: Corpus =
            serde_json::from_str(include_str!("../tests/data/picomatch-corpus.json")).unwrap();
        let mut checked = 0;
        let mut mismatches = Vec::new();
        for case in &corpus.cases {
            let inputs = if case.dot {
                &corpus.paths
            } else {
                &corpus.hosts
            };
            assert_eq!(inputs.len(), case.answers.len());
            let source = make_re(&case.pattern, case.dot)
                .unwrap_or_else(|e| panic!("{}: {e}", case.pattern));
            let regex = fancy_regex::Regex::new(&source)
                .unwrap_or_else(|e| panic!("{} -> {source}: {e}", case.pattern));
            let mut wrong = Vec::new();
            for (input, answer) in inputs.iter().zip(case.answers.chars()) {
                let expected = answer == '1';
                if regex.is_match(input).unwrap() != expected {
                    wrong.push(format!("{input:?} (picomatch: {expected})"));
                }
                checked += 1;
            }
            if !wrong.is_empty() {
                mismatches.push(format!(
                    "pattern {:?} (dot: {}), regex {source}: {} wrong, e.g. {}",
                    case.pattern,
                    case.dot,
                    wrong.len(),
                    wrong.iter().take(6).cloned().collect::<Vec<_>>().join(", ")
                ));
            }
        }
        assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
        assert!(checked > 1000, "only {checked} cases");
    }

    #[test]
    fn refuses_syntax_it_does_not_model() {
        for glob in [
            "/a/{b,c}", "/a/[ab]", "/a/!(b)", "/a/+(b)", "/a/@(b)", "/a/*(b)", "/a/?(b)", r"/a/\*",
            "/a/***", "", "!/a", "/a/(b)",
        ] {
            assert!(make_re(glob, true).is_err(), "{glob}");
        }
    }
}
