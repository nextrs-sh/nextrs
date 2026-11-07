//! A port of the parts of `path-to-regexp` 6 (as bundled in
//! `next/dist/compiled/path-to-regexp`) that Next's router uses at request
//! time: `parse` (to learn a pattern's keys), `compile` with
//! `validate: false` (destinations), and the parameter extraction of
//! `regexpToFunction`. Matching itself uses the regex Next wrote into
//! `routes-manifest.json`; this module never builds regexes.
//!
//! The functions follow the JavaScript line by line, including its error
//! cases, so that a destination Next would refuse fails here too.

use super::query::{JsMap, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyName {
    Named(String),
    /// An unnamed group `(…)`: path-to-regexp numbers them from 0.
    Index(usize),
}

impl KeyName {
    /// The property name in a params object (`params[0]` is `params["0"]`).
    pub fn as_key(&self) -> String {
        match self {
            Self::Named(name) => name.clone(),
            Self::Index(n) => n.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub name: KeyName,
    pub prefix: String,
    pub suffix: String,
    pub pattern: String,
    pub modifier: String,
}

impl Key {
    fn repeats(&self) -> bool {
        self.modifier == "*" || self.modifier == "+"
    }

    fn optional(&self) -> bool {
        self.modifier == "?" || self.modifier == "*"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Text(String),
    Key(Key),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lex {
    Modifier,
    EscapedChar,
    Open,
    Close,
    Name,
    Pattern,
    Char,
    End,
}

struct LexToken {
    kind: Lex,
    index: usize,
    value: String,
}

fn lexer(input: &[char]) -> Result<Vec<LexToken>, String> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let push = |tokens: &mut Vec<LexToken>, kind, index, value: String| {
        tokens.push(LexToken { kind, index, value })
    };
    while i < input.len() {
        let c = input[i];
        match c {
            '*' | '+' | '?' => {
                push(&mut tokens, Lex::Modifier, i, c.to_string());
                i += 1;
            }
            '\\' => {
                // JavaScript reads past the end as `undefined`, which
                // concatenates as "undefined"; an escape at the very end
                // never occurs in a valid pattern, so it is refused.
                let Some(&next) = input.get(i + 1) else {
                    return Err(format!("Unterminated escape at {i}"));
                };
                push(&mut tokens, Lex::EscapedChar, i, next.to_string());
                i += 2;
            }
            '{' => {
                push(&mut tokens, Lex::Open, i, c.to_string());
                i += 1;
            }
            '}' => {
                push(&mut tokens, Lex::Close, i, c.to_string());
                i += 1;
            }
            ':' => {
                let mut name = String::new();
                let mut j = i + 1;
                while j < input.len() {
                    let ch = input[j];
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        name.push(ch);
                        j += 1;
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    return Err(format!("Missing parameter name at {i}"));
                }
                push(&mut tokens, Lex::Name, i, name);
                i = j;
            }
            '(' => {
                let mut count = 1;
                let mut pattern = String::new();
                let mut j = i + 1;
                if input.get(j) == Some(&'?') {
                    return Err(format!("Pattern cannot start with \"?\" at {j}"));
                }
                while j < input.len() {
                    if input[j] == '\\' {
                        pattern.push(input[j]);
                        if let Some(&next) = input.get(j + 1) {
                            pattern.push(next);
                        }
                        j += 2;
                        continue;
                    }
                    if input[j] == ')' {
                        count -= 1;
                        if count == 0 {
                            j += 1;
                            break;
                        }
                    } else if input[j] == '(' {
                        count += 1;
                        if input.get(j + 1) != Some(&'?') {
                            return Err(format!("Capturing groups are not allowed at {j}"));
                        }
                    }
                    pattern.push(input[j]);
                    j += 1;
                }
                if count != 0 {
                    return Err(format!("Unbalanced pattern at {i}"));
                }
                if pattern.is_empty() {
                    return Err(format!("Missing pattern at {i}"));
                }
                push(&mut tokens, Lex::Pattern, i, pattern);
                i = j;
            }
            _ => {
                push(&mut tokens, Lex::Char, i, c.to_string());
                i += 1;
            }
        }
    }
    push(&mut tokens, Lex::End, i, String::new());
    Ok(tokens)
}

/// path-to-regexp's `escapeString`.
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if ".+*?=^!:${}()[]|/\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

const PREFIXES: &str = "./";
const DELIMITER: &str = "/#?";

/// `parse(str)` with path-to-regexp's defaults (`prefixes: "./"`,
/// `delimiter: "/#?"`), the options Next uses.
pub fn parse(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.chars().collect();
    let tokens = lexer(&chars)?;
    let mut result: Vec<Token> = Vec::new();
    let mut key_index = 0usize;
    let mut i = 0usize;
    let mut path = String::new();

    let try_consume = |i: &mut usize, kind: Lex| -> Option<String> {
        if *i < tokens.len() && tokens[*i].kind == kind {
            *i += 1;
            Some(tokens[*i - 1].value.clone())
        } else {
            None
        }
    };
    let must_consume = |i: &mut usize, kind: Lex| -> Result<String, String> {
        try_consume(i, kind).ok_or_else(|| {
            let token = &tokens[*i];
            format!(
                "Unexpected {:?} at {}, expected {kind:?}",
                token.kind, token.index
            )
        })
    };
    let consume_text = |i: &mut usize| -> String {
        let mut text = String::new();
        while let Some(value) =
            try_consume(i, Lex::Char).or_else(|| try_consume(i, Lex::EscapedChar))
        {
            text.push_str(&value);
        }
        text
    };
    let is_safe = |value: &str| DELIMITER.chars().any(|d| value.contains(d));
    let safe_pattern = |result: &[Token], prefix: &str| -> Result<String, String> {
        let previous = result.last();
        let text = if !prefix.is_empty() {
            prefix.to_owned()
        } else {
            match previous {
                Some(Token::Text(t)) => t.clone(),
                _ => String::new(),
            }
        };
        if let (Some(previous), true) = (previous, text.is_empty()) {
            let name = match previous {
                Token::Key(key) => key.name.as_key(),
                Token::Text(t) => t.clone(),
            };
            return Err(format!(
                "Must have text between two parameters, missing text after \"{name}\""
            ));
        }
        if text.is_empty() || is_safe(&text) {
            return Ok(format!("[^{}]+?", escape_string(DELIMITER)));
        }
        Ok(format!(
            "(?:(?!{})[^{}])+?",
            escape_string(&text),
            escape_string(DELIMITER)
        ))
    };

    while i < tokens.len() {
        let character = try_consume(&mut i, Lex::Char);
        let name = try_consume(&mut i, Lex::Name);
        let pattern = try_consume(&mut i, Lex::Pattern);
        if name.is_some() || pattern.is_some() {
            let mut prefix = character.unwrap_or_default();
            // Only "." and "/" stay prefixes; other text joins the path.
            if !prefix.is_empty() && !PREFIXES.contains(prefix.as_str()) {
                path.push_str(&prefix);
                prefix.clear();
            }
            if !path.is_empty() {
                result.push(Token::Text(std::mem::take(&mut path)));
            }
            let key_name = match name {
                Some(name) => KeyName::Named(name),
                None => {
                    key_index += 1;
                    KeyName::Index(key_index - 1)
                }
            };
            let pattern = match pattern {
                Some(pattern) => pattern,
                None => safe_pattern(&result, &prefix)?,
            };
            let modifier = try_consume(&mut i, Lex::Modifier).unwrap_or_default();
            result.push(Token::Key(Key {
                name: key_name,
                prefix,
                suffix: String::new(),
                pattern,
                modifier,
            }));
            continue;
        }
        if let Some(value) = character.or_else(|| try_consume(&mut i, Lex::EscapedChar)) {
            path.push_str(&value);
            continue;
        }
        if !path.is_empty() {
            result.push(Token::Text(std::mem::take(&mut path)));
        }
        if try_consume(&mut i, Lex::Open).is_some() {
            let prefix = consume_text(&mut i);
            let name = try_consume(&mut i, Lex::Name).unwrap_or_default();
            let pattern = try_consume(&mut i, Lex::Pattern).unwrap_or_default();
            let suffix = consume_text(&mut i);
            must_consume(&mut i, Lex::Close)?;
            let key_name = if !name.is_empty() {
                KeyName::Named(name.clone())
            } else if !pattern.is_empty() {
                key_index += 1;
                KeyName::Index(key_index - 1)
            } else {
                KeyName::Named(String::new())
            };
            let pattern = if !name.is_empty() && pattern.is_empty() {
                safe_pattern(&result, &prefix)?
            } else {
                pattern
            };
            let modifier = try_consume(&mut i, Lex::Modifier).unwrap_or_default();
            result.push(Token::Key(Key {
                name: key_name,
                pattern,
                prefix,
                suffix,
                modifier,
            }));
            continue;
        }
        must_consume(&mut i, Lex::End)?;
    }
    Ok(result)
}

/// The keys `pathToRegexp(str, keys)` reports: tokens with a pattern.
pub fn keys(tokens: &[Token]) -> Vec<Key> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Key(key) if !key.pattern.is_empty() => Some(key.clone()),
            _ => None,
        })
        .collect()
}

/// `compile(str, { validate: false, encode })(params)`: Next only compiles
/// destinations, never with validation.
pub fn compile(
    tokens: &[Token],
    params: &JsMap,
    encode: impl Fn(&str) -> String,
) -> Result<String, String> {
    let mut out = String::new();
    for token in tokens {
        let key = match token {
            Token::Text(text) => {
                out.push_str(text);
                continue;
            }
            Token::Key(key) => key,
        };
        let name = key.name.as_key();
        match params.get(&name) {
            Some(Value::Arr(items)) => {
                if !key.repeats() {
                    return Err(format!(
                        "Expected \"{name}\" to not repeat, but got an array"
                    ));
                }
                if items.is_empty() {
                    if key.optional() {
                        continue;
                    }
                    return Err(format!("Expected \"{name}\" to not be empty"));
                }
                for item in items {
                    out.push_str(&key.prefix);
                    out.push_str(&encode(item));
                    out.push_str(&key.suffix);
                }
            }
            Some(Value::Str(value)) => {
                out.push_str(&key.prefix);
                out.push_str(&encode(value));
                out.push_str(&key.suffix);
            }
            None => {
                if key.optional() {
                    continue;
                }
                let expected = if key.repeats() {
                    "an array"
                } else {
                    "a string"
                };
                return Err(format!("Expected \"{name}\" to be {expected}"));
            }
        }
    }
    Ok(out)
}

/// `regexpToFunction(regexp, keys)` on one match: each defined group is
/// the value of its key, split on `prefix + suffix` for repeating keys.
/// Unnamed keys are dropped (`removeUnnamedParams: true`).
pub fn params_from_captures(
    captures: &fancy_regex::Captures<'_>,
    keys: &[Key],
    remove_unnamed: bool,
) -> JsMap {
    let mut params = JsMap::new();
    for i in 1..captures.len() {
        let (Some(group), Some(key)) = (captures.get(i), keys.get(i - 1)) else {
            continue;
        };
        let text = group.as_str();
        let value = if key.repeats() {
            let separator = format!("{}{}", key.prefix, key.suffix);
            Value::Arr(js_split(text, &separator))
        } else {
            Value::Str(text.to_owned())
        };
        params.insert(key.name.as_key(), value);
    }
    if remove_unnamed {
        for key in keys {
            if let KeyName::Index(n) = key.name {
                params.remove(&n.to_string());
            }
        }
    }
    params
}

/// JavaScript's `String.prototype.split(separator)`.
fn js_split(text: &str, separator: &str) -> Vec<String> {
    if separator.is_empty() {
        return text.chars().map(String::from).collect();
    }
    text.split(separator).map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(input: &str) -> Vec<String> {
        keys(&parse(input).unwrap())
            .iter()
            .map(|k| k.name.as_key())
            .collect()
    }

    #[test]
    fn keys_of_common_sources() {
        assert_eq!(names("/blog/:slug"), ["slug"]);
        assert_eq!(names("/:path*"), ["path"]);
        assert_eq!(names("/(.*)"), ["0"]);
        assert_eq!(names("/a/:id(\\d{1,})/:rest+"), ["id", "rest"]);
        assert_eq!(names("/x{-:y}?"), ["y"]);
        assert_eq!(names("/plain"), Vec::<String>::new());
    }

    #[test]
    fn parse_errors_match_path_to_regexp() {
        assert!(parse("/:").unwrap_err().contains("Missing parameter name"));
        assert!(parse("/(a(b))").unwrap_err().contains("Capturing groups"));
        assert!(
            parse("/:a:b")
                .unwrap_err()
                .contains("Must have text between")
        );
    }

    #[test]
    fn compile_repeats_and_optionals() {
        let tokens = parse("/blog/:path*").unwrap();
        let mut params = JsMap::new();
        assert_eq!(compile(&tokens, &params, str::to_owned).unwrap(), "/blog");
        params.insert("path", Value::Arr(vec!["a".into(), "b".into()]));
        assert_eq!(
            compile(&tokens, &params, str::to_owned).unwrap(),
            "/blog/a/b"
        );
        let one = parse("/x/:id").unwrap();
        assert!(
            compile(&one, &params, str::to_owned)
                .unwrap_err()
                .contains("to be a string")
        );
    }
}
