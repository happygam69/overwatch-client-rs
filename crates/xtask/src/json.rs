//! A minimal JSON reader for cargo's `--message-format=json` stream.
//!
//! `xtask` deliberately has no dependencies, and the only JSON it ever
//! needs is one line per message from `cargo test --no-run`. This module
//! parses exactly that subset — objects, arrays, strings, numbers,
//! booleans and null — in insertion order, because the field order inside
//! cargo's `profile` object is not part of any stability promise and
//! nothing here may depend on it. Numbers are read as `f64`: the runner
//! only compares strings and booleans, never numeric values.

use std::fmt;

/// A parsed JSON value. Object members keep their insertion order.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    /// The value stored under `key` in this object, if any.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            // Duplicate keys are not expected from cargo; if one slips
            // through, the last member wins, the way a JSON reader that
            // builds a map would resolve it.
            Value::Obj(pairs) => pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(items) => Some(items),
            _ => None,
        }
    }
}

/// Where a parse stopped, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: &'static str,
    pub offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

/// Parse one JSON document. Trailing whitespace is allowed; anything else
/// after the root value is an error, so a half-received line cannot pass
/// as a complete one.
pub fn parse(input: &str) -> Result<Value, ParseError> {
    let bytes = input.as_bytes();
    let mut pos = 0usize;
    let value = parse_value(bytes, &mut pos)?;
    skip_ws(bytes, &mut pos);
    if pos != bytes.len() {
        return Err(ParseError {
            message: "trailing bytes after the root value",
            offset: pos,
        });
    }
    Ok(value)
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn parse_value(bytes: &[u8], pos: &mut usize) -> Result<Value, ParseError> {
    skip_ws(bytes, pos);
    let Some(&first) = bytes.get(*pos) else {
        return Err(ParseError {
            message: "expected a value, found end of input",
            offset: *pos,
        });
    };
    match first {
        b'{' => parse_obj(bytes, pos),
        b'[' => parse_arr(bytes, pos),
        b'"' => parse_string(bytes, pos).map(Value::Str),
        b't' => parse_lit(bytes, pos, "true", Value::Bool(true)),
        b'f' => parse_lit(bytes, pos, "false", Value::Bool(false)),
        b'n' => parse_lit(bytes, pos, "null", Value::Null),
        b'-' | b'0'..=b'9' => parse_num(bytes, pos),
        _ => Err(ParseError {
            message: "expected a value",
            offset: *pos,
        }),
    }
}

fn parse_lit(bytes: &[u8], pos: &mut usize, lit: &str, value: Value) -> Result<Value, ParseError> {
    if bytes[*pos..].starts_with(lit.as_bytes()) {
        *pos += lit.len();
        Ok(value)
    } else {
        Err(ParseError {
            message: "unrecognised literal",
            offset: *pos,
        })
    }
}

fn parse_num(bytes: &[u8], pos: &mut usize) -> Result<Value, ParseError> {
    let start = *pos;
    if bytes.get(*pos) == Some(&b'-') {
        *pos += 1;
    }
    while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
        *pos += 1;
    }
    if bytes.get(*pos) == Some(&b'.') {
        *pos += 1;
        while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
            *pos += 1;
        }
    }
    if matches!(bytes.get(*pos), Some(b'e') | Some(b'E')) {
        *pos += 1;
        if matches!(bytes.get(*pos), Some(b'+') | Some(b'-')) {
            *pos += 1;
        }
        while matches!(bytes.get(*pos), Some(b'0'..=b'9')) {
            *pos += 1;
        }
    }
    let text = std::str::from_utf8(&bytes[start..*pos]).unwrap_or("");
    text.parse::<f64>().map(Value::Num).map_err(|_| ParseError {
        message: "malformed number",
        offset: start,
    })
}

fn parse_string(bytes: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    // The opening quote is already confirmed by the caller.
    *pos += 1;
    let mut out = String::new();
    loop {
        let Some(&b) = bytes.get(*pos) else {
            return Err(ParseError {
                message: "unterminated string",
                offset: *pos,
            });
        };
        match b {
            b'"' => {
                *pos += 1;
                return Ok(out);
            }
            b'\\' => {
                *pos += 1;
                let Some(&esc) = bytes.get(*pos) else {
                    return Err(ParseError {
                        message: "unterminated escape",
                        offset: *pos,
                    });
                };
                *pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let cp = parse_hex4(bytes, pos)?;
                        let ch = match (cp, bytes.get(*pos)) {
                            // A high surrogate must be followed by \uXXXX low
                            // surrogate; anything else is replacement.
                            (0xD800..=0xDBFF, Some(&b'\\'))
                                if bytes.get(*pos + 1) == Some(&b'u') =>
                            {
                                let save = *pos;
                                *pos += 2;
                                let low = parse_hex4(bytes, pos)?;
                                if (0xDC00..=0xDFFF).contains(&low) {
                                    let combined = 0x10000
                                        + ((cp - 0xD800) as u32) * 0x400
                                        + (low - 0xDC00) as u32;
                                    char::from_u32(combined).unwrap_or('\u{FFFD}')
                                } else {
                                    *pos = save;
                                    '\u{FFFD}'
                                }
                            }
                            _ => char::from_u32(cp as u32).unwrap_or('\u{FFFD}'),
                        };
                        out.push(ch);
                    }
                    _ => {
                        return Err(ParseError {
                            message: "unknown escape",
                            offset: *pos - 1,
                        })
                    }
                }
            }
            0x00..=0x1F => {
                return Err(ParseError {
                    message: "raw control character in string",
                    offset: *pos,
                })
            }
            _ => {
                // Copy the whole UTF-8 sequence in one go.
                let start = *pos;
                let len = utf8_len(b).ok_or(ParseError {
                    message: "invalid UTF-8 lead byte",
                    offset: *pos,
                })?;
                let end = start + len;
                if end > bytes.len() {
                    return Err(ParseError {
                        message: "truncated UTF-8 sequence",
                        offset: start,
                    });
                }
                let s = std::str::from_utf8(&bytes[start..end]).map_err(|_| ParseError {
                    message: "invalid UTF-8 sequence",
                    offset: start,
                })?;
                out.push_str(s);
                *pos = end;
            }
        }
    }
}

fn parse_hex4(bytes: &[u8], pos: &mut usize) -> Result<u16, ParseError> {
    let start = *pos;
    let mut value = 0u16;
    for _ in 0..4 {
        let Some(&b) = bytes.get(*pos) else {
            return Err(ParseError {
                message: "truncated \\u escape",
                offset: *pos,
            });
        };
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => {
                return Err(ParseError {
                    message: "invalid hex digit in \\u escape",
                    offset: *pos,
                })
            }
        };
        value = value * 16 + digit as u16;
        *pos += 1;
    }
    let _ = start;
    Ok(value)
}

fn utf8_len(lead: u8) -> Option<usize> {
    match lead {
        0x00..=0x7F => Some(1),
        0xC2..=0xDF => Some(2),
        0xE0..=0xEF => Some(3),
        0xF0..=0xF4 => Some(4),
        _ => None,
    }
}

fn parse_arr(bytes: &[u8], pos: &mut usize) -> Result<Value, ParseError> {
    *pos += 1; // '['
    let mut items = Vec::new();
    skip_ws(bytes, pos);
    if bytes.get(*pos) == Some(&b']') {
        *pos += 1;
        return Ok(Value::Arr(items));
    }
    loop {
        items.push(parse_value(bytes, pos)?);
        skip_ws(bytes, pos);
        match bytes.get(*pos) {
            Some(&b',') => {
                *pos += 1;
            }
            Some(&b']') => {
                *pos += 1;
                return Ok(Value::Arr(items));
            }
            _ => {
                return Err(ParseError {
                    message: "expected ',' or ']' in array",
                    offset: *pos,
                })
            }
        }
    }
}

fn parse_obj(bytes: &[u8], pos: &mut usize) -> Result<Value, ParseError> {
    *pos += 1; // '{'
    let mut pairs = Vec::new();
    skip_ws(bytes, pos);
    if bytes.get(*pos) == Some(&b'}') {
        *pos += 1;
        return Ok(Value::Obj(pairs));
    }
    loop {
        skip_ws(bytes, pos);
        if bytes.get(*pos) != Some(&b'"') {
            return Err(ParseError {
                message: "expected a string key in object",
                offset: *pos,
            });
        }
        let key = parse_string(bytes, pos)?;
        skip_ws(bytes, pos);
        if bytes.get(*pos) != Some(&b':') {
            return Err(ParseError {
                message: "expected ':' after object key",
                offset: *pos,
            });
        }
        *pos += 1;
        let value = parse_value(bytes, pos)?;
        pairs.push((key, value));
        skip_ws(bytes, pos);
        match bytes.get(*pos) {
            Some(&b',') => {
                *pos += 1;
            }
            Some(&b'}') => {
                *pos += 1;
                return Ok(Value::Obj(pairs));
            }
            _ => {
                return Err(ParseError {
                    message: "expected ',' or '}' in object",
                    offset: *pos,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_round_trip() {
        assert_eq!(parse("null").unwrap(), Value::Null);
        assert_eq!(parse("true").unwrap(), Value::Bool(true));
        assert_eq!(parse("false").unwrap(), Value::Bool(false));
        assert_eq!(parse(" 42 ").unwrap(), Value::Num(42.0));
        assert_eq!(parse("-1.5e2").unwrap(), Value::Num(-150.0));
        assert_eq!(parse("\"x\"").unwrap(), Value::Str("x".into()));
    }

    #[test]
    fn objects_keep_insertion_order() {
        let v = parse(r#"{"a":1,"b":2,"a":3}"#).unwrap();
        let Value::Obj(pairs) = &v else {
            panic!("not an object");
        };
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b", "a"]);
        // The *last* member wins for `get`, like a real JSON reader.
        assert_eq!(v.get("a"), Some(&Value::Num(3.0)));
    }

    #[test]
    fn nested_structures_round_trip() {
        let v = parse(r#"{"reason":"compiler-artifact","profile":{"test":true},"k":[null,2]}"#)
            .unwrap();
        assert_eq!(
            v.get("reason").and_then(Value::as_str),
            Some("compiler-artifact")
        );
        assert_eq!(
            v.get("profile")
                .and_then(|p| p.get("test"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            v.get("k").and_then(Value::as_arr).map(<[Value]>::len),
            Some(2)
        );
    }

    #[test]
    fn string_escapes_decode() {
        let v = parse(r#""a\\b\"c\/d\be\ff\ng\rh\ti""#).unwrap();
        assert_eq!(v, Value::Str("a\\b\"c/d\u{0008}e\u{000C}f\ng\rh\ti".into()));
    }

    #[test]
    fn unicode_escapes_decode_with_surrogate_pairs() {
        // \u0041 = 'A'; \uD83D\uDE00 is the 😀 pair; a lone surrogate degrades.
        let v = parse(r#""A\uD83D\uDE00""#).unwrap();
        assert_eq!(v, Value::Str("A😀".into()));
        let lone = parse(r#""\uD83D""#).unwrap();
        assert_eq!(lone, Value::Str("\u{FFFD}".into()));
        let pair_without_low = parse(r#""\uD83Dx""#).unwrap();
        assert_eq!(pair_without_low, Value::Str("\u{FFFD}x".into()));
    }

    #[test]
    fn windows_paths_unescape() {
        let v = parse(r#""C:\\Users\\z\\target\\deps\\ow_sim-abc""#).unwrap();
        assert_eq!(
            v,
            Value::Str("C:\\Users\\z\\target\\deps\\ow_sim-abc".into())
        );
    }

    #[test]
    fn raw_utf8_passes_through() {
        let v = parse("\"héllo → 你好\"").unwrap();
        assert_eq!(v, Value::Str("héllo → 你好".into()));
    }

    #[test]
    fn malformed_input_is_rejected() {
        for bad in [
            "",         // empty
            "{\"a\":}", // missing value
            "[1,]",     // trailing comma
            "\"unterminated",
            "{\"a\":1}x", // trailing garbage
            "tru",        // truncated literal
            "\"\\q\"",    // unknown escape
            "\"\\u12\"",  // truncated \u escape
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_cargo_artifact_line_parses() {
        // Shape trimmed from real `cargo test --no-run --message-format=json`
        // output (fields reordered here on purpose: order must not matter).
        let line = r#"{"target":{"kind":["test"],"name":"index_find_real","src_path":"/x/tests/index_find_real.rs"},"profile":{"debug":0,"test":true},"executable":"/x/target/release/deps/index_find_real-9f2b","reason":"compiler-artifact","fresh":true}"#;
        let v = parse(line).unwrap();
        assert_eq!(
            v.get("reason").and_then(Value::as_str),
            Some("compiler-artifact")
        );
        assert_eq!(
            v.get("target")
                .and_then(|t| t.get("name"))
                .and_then(Value::as_str),
            Some("index_find_real")
        );
        assert_eq!(
            v.get("profile")
                .and_then(|p| p.get("test"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            v.get("executable").and_then(Value::as_str),
            Some("/x/target/release/deps/index_find_real-9f2b")
        );
    }
}
