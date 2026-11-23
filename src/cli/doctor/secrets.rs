//! Secret patterns and the per-line scan `doctor` runs over payload overrides.

/// `_DOCTOR_SECRET_PATTERNS`, as a prefix and what must follow it.
const SECRET_PATTERNS: [Secret; 7] = [
    Secret::Run("sk-", CharClass::Base64Url, 20),
    Secret::Run("ghp_", CharClass::Alnum, 30),
    Secret::Run("github_pat_", CharClass::AlnumUnderscore, 30),
    Secret::Run("AKIA", CharClass::UpperDigit, 16),
    Secret::Slack,
    Secret::Run("AIza", CharClass::Base64Url, 35),
    Secret::Jwt,
];

#[derive(Clone, Copy)]
enum CharClass {
    /// `[A-Za-z0-9_-]`
    Base64Url,
    /// `[A-Za-z0-9]`
    Alnum,
    /// `[A-Za-z0-9_]`
    AlnumUnderscore,
    /// `[0-9A-Z]`
    UpperDigit,
}

impl CharClass {
    fn matches(self, b: u8) -> bool {
        match self {
            CharClass::Base64Url => b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
            CharClass::Alnum => b.is_ascii_alphanumeric(),
            CharClass::AlnumUnderscore => b.is_ascii_alphanumeric() || b == b'_',
            CharClass::UpperDigit => b.is_ascii_uppercase() || b.is_ascii_digit(),
        }
    }
}

enum Secret {
    /// `<prefix>[class]{min,}`.
    Run(&'static str, CharClass, usize),
    /// `xox[baprs]-[A-Za-z0-9-]{10,}`.
    Slack,
    /// `eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}`.
    Jwt,
}

fn run_len(bytes: &[u8], class: CharClass) -> usize {
    bytes.iter().take_while(|b| class.matches(**b)).count()
}

impl Secret {
    fn found_in(&self, line: &[u8]) -> bool {
        match self {
            Secret::Run(prefix, class, min) => (0..line.len()).any(|i| {
                line[i..].starts_with(prefix.as_bytes())
                    && run_len(&line[i + prefix.len()..], *class) >= *min
            }),
            Secret::Slack => (0..line.len()).any(|i| {
                let rest = &line[i..];
                rest.len() > 5
                    && rest.starts_with(b"xox")
                    && b"baprs".contains(&rest[3])
                    && rest[4] == b'-'
                    && rest[5..]
                        .iter()
                        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
                        .count()
                        >= 10
            }),
            Secret::Jwt => (0..line.len()).any(|i| {
                let rest = &line[i..];
                if !rest.starts_with(b"eyJ") {
                    return false;
                }
                let mut at = 0;
                for part in 0..3 {
                    let len = run_len(&rest[at..], CharClass::Base64Url);
                    if len < if part == 0 { 13 } else { 10 } {
                        return false;
                    }
                    at += len;
                    if part < 2 {
                        if rest.get(at) != Some(&b'.') {
                            return false;
                        }
                        at += 1;
                    }
                }
                true
            }),
        }
    }
}

/// Every `${...}` span and every `<...>` span without `sk-` removed, so a
/// pattern reads what is left. A name in braces or angle brackets is a
/// placeholder; a secret standing next to one is still a secret.
fn without_placeholders(text: &str) -> String {
    let text = without_spans(text, "${", '}', |_| true);
    without_spans(&text, "<", '>', |span| !span.contains("sk-"))
}

fn without_spans(
    text: &str,
    open: &str,
    close: char,
    placeholder: impl Fn(&str) -> bool,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        // An unclosed span opens nothing: the text after it stays readable.
        let Some(len) = rest[start..].find(close) else {
            break;
        };
        let end = start + len + close.len_utf8();
        out.push_str(&rest[..start]);
        if !placeholder(&rest[start..end]) {
            out.push_str(&rest[start..end]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `_doctor_scan_file`: the `grep -n` lines that hold a secret, one entry per
/// line whatever matched it; empty for a clean or binary file.
pub(crate) fn scan_secrets(bytes: &[u8]) -> Vec<String> {
    if bytes.contains(&0) {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if lines.last() == Some(&&b""[..]) {
        lines.pop();
    }
    let mut hits = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let text = String::from_utf8_lossy(line);
        let readable = without_placeholders(&text);
        if SECRET_PATTERNS
            .iter()
            .any(|pattern| pattern.found_in(readable.as_bytes()))
        {
            hits.push(format!("{}:{text}", index + 1));
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_line_holding_a_secret_is_listed_whichever_pattern_found_it() {
        let mcp = br#"{"mcpServers":{"gh":{"env":{"TOKEN":"ghp_abcdefghijklmnopqrstuvwxyz012345678901"}}}}"#;
        let expected = format!("1:{}", String::from_utf8_lossy(mcp));
        assert_eq!(
            scan_secrets(&[mcp.as_slice(), b"\n"].concat()),
            vec![expected]
        );
        let many = br#"{"aws":{"key":"AKIAIOSFODNN7EXAMPLE"},"slack":"xoxb-1234567890-abc","g":"AIzaSyA1234567890abcdefghijklmnopqrstuv","jwt":"eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn","pat":"github_pat_abcdefghijklmnopqrstuvwxyz0123456789"}"#;
        assert_eq!(scan_secrets(many).len(), 1);
        // Every match is reported, whichever pattern found it: Bash returned
        // the first pattern's hits and hid the rest.
        assert_eq!(
            scan_secrets(b"AKIAIOSFODNN7EXAMPLE\nsk-abcdefghijklmnopqrstuvwxyz\n"),
            vec![
                "1:AKIAIOSFODNN7EXAMPLE".to_string(),
                "2:sk-abcdefghijklmnopqrstuvwxyz".to_string()
            ]
        );
        assert_eq!(
            scan_secrets(
                b"first xoxp-abcdefghij-k\nsecond eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn\n"
            ),
            vec![
                "1:first xoxp-abcdefghij-k".to_string(),
                "2:second eyJabcdefghijk.eyJabcdefghijk.abcdefghijklmn".to_string()
            ]
        );
        // A `${VAR}` beside a real token no longer hides it.
        assert_eq!(
            scan_secrets(
                b"a ${X} ghp_abcdefghijklmnopqrstuvwxyz012345678901\nb AKIAIOSFODNN7EXAMPLE\n"
            ),
            vec![
                "1:a ${X} ghp_abcdefghijklmnopqrstuvwxyz012345678901".to_string(),
                "2:b AKIAIOSFODNN7EXAMPLE".to_string()
            ]
        );
        assert_eq!(
            scan_secrets(b"token: ${GITHUB_TOKEN} ghp_abcdefghijklmnopqrstuvwxyz012345678901\n"),
            vec!["1:token: ${GITHUB_TOKEN} ghp_abcdefghijklmnopqrstuvwxyz012345678901".to_string()]
        );
        // A name in braces on its own is still a placeholder.
        assert!(scan_secrets(b"token: ${GITHUB_TOKEN}\n").is_empty());
        // An unclosed `${` opens no placeholder and hides nothing after it.
        assert_eq!(
            scan_secrets(b"${oops sk-abcdefghijklmnopqrstuvwxyz\n"),
            vec!["1:${oops sk-abcdefghijklmnopqrstuvwxyz".to_string()]
        );
        assert!(scan_secrets(b"<ghp_abcdefghijklmnopqrstuvwxyz012345678901>\n").is_empty());
        assert_eq!(
            scan_secrets(b"<sk-abcdefghijklmnopqrstuvwxyz>\n"),
            vec!["1:<sk-abcdefghijklmnopqrstuvwxyz>".to_string()]
        );
        assert_eq!(
            scan_secrets(b"host: <your-host> token: ghp_abcdefghijklmnopqrstuvwxyz012345678901\n"),
            vec![
                "1:host: <your-host> token: ghp_abcdefghijklmnopqrstuvwxyz012345678901".to_string()
            ]
        );
        assert!(
            scan_secrets(b"<your-host> <ghp_abcdefghijklmnopqrstuvwxyz012345678901>\n").is_empty()
        );
        assert!(scan_secrets(b"\0binary sk-abcdefghijklmnopqrstuvwxyz\n").is_empty());
        assert!(scan_secrets(b"sk-short\nxoxb-123\nAKIA1234\n").is_empty());
    }
}
