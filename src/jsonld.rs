//! Structured data: a <script type="application/ld+json"> is a template.
//!
//! Inside it, {{ expr }} inserts text escaped for a JSON string (the author
//! writes the quotes, as in an attribute), <template each|if> repeats or
//! drops a piece, and a comma left before ] or } is forgiven, because a loop
//! cannot avoid leaving one. The result has to parse as JSON, and is written
//! compact. Every other <script> and <style> stays untouched.

/// A string's content as it goes inside JSON quotes.
pub fn escape_string(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap_or_default();
    quoted[1..quoted.len() - 1].to_string()
}

/// Remove commas that sit right before ] or }, outside strings.
pub fn drop_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == ',' {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == ']' || chars[j] == '}') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Validate rendered JSON-LD and write it compact. Err carries the parser's
/// message. `</` is written as `<\/` so no string can end the script early.
pub fn finish(text: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(&drop_trailing_commas(text)).map_err(|e| e.to_string())?;
    Ok(serde_json::to_string(&value).unwrap_or_default().replace("</", "<\\/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_escaped_without_quotes() {
        assert_eq!(escape_string("a \"b\"\n<c>"), "a \\\"b\\\"\\n<c>");
    }

    #[test]
    fn trailing_commas_go_but_strings_stay() {
        assert_eq!(drop_trailing_commas("[1, 2, ]"), "[1, 2 ]");
        assert_eq!(drop_trailing_commas("{\"a\": \"x, ]\", \"b\": [ ], }"), "{\"a\": \"x, ]\", \"b\": [ ] }");
        assert_eq!(drop_trailing_commas("{\"a\": \"q\\\", ]\",}"), "{\"a\": \"q\\\", ]\"}");
    }

    #[test]
    fn output_is_valid_compact_and_script_safe() {
        assert_eq!(finish("{ \"a\": [1,\n 2,\n ], \"b\": \"</script>\", }").unwrap(), "{\"a\":[1,2],\"b\":\"<\\/script>\"}");
        assert!(finish("{ \"a\": }").is_err());
    }
}
