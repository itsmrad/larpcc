use std::path::Path;
pub fn text(value: &str) -> String {
    value.chars().flat_map(|c| {
  if c.is_control() || matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}') { c.escape_default().collect::<Vec<_>>() } else { vec![c] }
 }).collect()
}
pub fn path(value: &Path) -> String {
    text(&value.to_string_lossy())
}

#[cfg(test)]
mod tests {
    #[test]
    fn controls_and_bidi_escape_while_other_unicode_is_preserved() {
        assert_eq!(
            super::text("é\n\u{202e}\u{200b}\x1b"),
            "é\\n\\u{202e}\\u{200b}\\u{1b}"
        );
    }
}
