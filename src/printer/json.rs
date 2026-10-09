pub(crate) fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if c.is_ascii_control() => {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn json_payload_bytes(bytes: &[u8]) -> u64 {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|record| {
            memchr::memmem::find(record, b"\"type\":\"end\"").is_none()
                && memchr::memmem::find(record, b"\"type\":\"summary\"").is_none()
        })
        .map(|record| record.len() as u64)
        .sum()
}
