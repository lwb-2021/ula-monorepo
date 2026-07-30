const ULA_PTC_START: &str = "```ula-ptc\n";
const ULA_PTC_END: &str = "\n```";

pub fn extract(content: &str) -> Option<Vec<&str>> {
    content.contains(ULA_PTC_START).then(|| {
        content
            .split(ULA_PTC_START)
            .skip(1)
            .filter_map(|seg| seg.split_once(ULA_PTC_END).map(|splits| splits.0))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::extract;

    #[test]
    fn extractor_parse_plain_text() {
        assert_eq!(extract("plain markdown"), None);
    }

    #[test]
    fn extractor_parse_irrevalant_code_block() {
        assert_eq!(extract("```bash\nls\n```"), None);
    }

    #[test]
    fn extractor_parse_complex_block() {
        let content = [
            "text1",
            "```ula-ptc",
            "a()",
            "```",
            "text2",
            "```bash",
            "skip()",
            "```",
            "```ula-ptc",
            "b()",
            "```",
        ]
        .join("\n");
        assert_eq!(extract(&content), Some(vec!["a()", "b()"]));

        assert_eq!(extract("plain markdown"), None);
    }
}
