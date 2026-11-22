//! NFC 标签容量估算（NDEF URI 记录）。

/// NDEF URI 记录的前缀缩写（NFC Forum URI RTD）：常见前缀用 1 个字节表示。
const PREFIXES: [(&str, u8); 4] = [("https://www.", 0x02), ("http://www.", 0x01), ("https://", 0x04), ("http://", 0x03)];

/// 写入 URL 后占用的字节数：TLV 头 2 + 记录头 4（短记录）+ 前缀码 1 + 去掉前缀的 URL + TLV 结束符 1。
pub fn ndef_uri_size(url: &str) -> usize {
    let rest = PREFIXES
        .iter()
        .find_map(|(prefix, _)| url.strip_prefix(prefix))
        .unwrap_or(url);
    let payload = 1 + rest.len();
    let tlv_len = if payload + 4 > 254 { 4 } else { 2 };
    tlv_len + 4 + payload + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_tags() {
        // https:// 缩写为 1 字节：2 + 4 + (1 + 11) + 1
        assert_eq!(ndef_uri_size("https://a.test/abc"), 2 + 4 + 1 + 10 + 1);
        assert_eq!(ndef_uri_size("https://www.a.test"), 2 + 4 + 1 + 6 + 1);
        let long = format!("https://x.test/{}", "a".repeat(400));
        assert_eq!(ndef_uri_size(&long), 4 + 4 + 1 + 407 + 1);
    }
}
