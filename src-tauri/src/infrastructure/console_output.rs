use crate::domain::error::redact_diagnostics;

#[derive(Default)]
enum Escape {
    #[default]
    Text,
    Start,
    Intermediate,
    Csi,
    Osc,
    OscTerminator,
    String,
    StringTerminator,
}
/// Buffers a logical line until it can be redacted atomically. The runner caps
/// raw input BEFORE this stage, so even newline-free output stays bounded.
#[derive(Default)]
pub struct ConsoleOutput {
    line: String,
    pending_utf8: Vec<u8>,
    escape: Escape,
}
impl ConsoleOutput {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        let mut output = String::new();
        let mut input = std::mem::take(&mut self.pending_utf8);
        input.extend_from_slice(bytes);
        let mut offset = 0;
        while offset < input.len() {
            match std::str::from_utf8(&input[offset..]) {
                Ok(text) => {
                    for character in text.chars() {
                        self.character(character, &mut output);
                    }
                    break;
                }
                Err(error) => {
                    let end = offset + error.valid_up_to();
                    for character in std::str::from_utf8(&input[offset..end])
                        .expect("validated UTF-8 prefix")
                        .chars()
                    {
                        self.character(character, &mut output);
                    }
                    offset = end;
                    if let Some(invalid_bytes) = error.error_len() {
                        self.character('\u{fffd}', &mut output);
                        offset += invalid_bytes;
                    } else {
                        // At most three bytes of an incomplete scalar cross a read boundary.
                        self.pending_utf8.extend_from_slice(&input[offset..]);
                        break;
                    }
                }
            }
        }
        output
    }
    fn character(&mut self, character: char, output: &mut String) {
        // Decode first: a 0x9B continuation byte inside Chinese text is not CSI.
        match self.escape {
            Escape::Text => match character {
                '\u{1b}' => self.escape = Escape::Start,
                '\u{9b}' => self.escape = Escape::Csi,
                '\u{9d}' => self.escape = Escape::Osc,
                '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}' => self.escape = Escape::String,
                '\n' => {
                    output.push_str(&self.flush_line());
                    output.push('\n');
                }
                '\r' | '\t' => self.line.push(character),
                value if !value.is_control() => self.line.push(value),
                _ => {}
            },
            Escape::Start => {
                self.escape = match character {
                    '[' => Escape::Csi,
                    ']' => Escape::Osc,
                    'P' | 'X' | '^' | '_' => Escape::String,
                    '\u{20}'..='\u{2f}' => Escape::Intermediate,
                    _ => Escape::Text,
                }
            }
            Escape::Intermediate => {
                if !('\u{20}'..='\u{2f}').contains(&character) {
                    self.escape = Escape::Text;
                }
            }
            Escape::Csi => {
                if character == '\u{1b}' {
                    self.escape = Escape::Start;
                } else if ('\u{40}'..='\u{7e}').contains(&character) {
                    self.escape = Escape::Text;
                }
            }
            Escape::Osc => match character {
                '\u{7}' | '\u{9c}' => self.escape = Escape::Text,
                '\u{1b}' => self.escape = Escape::OscTerminator,
                _ => {}
            },
            Escape::OscTerminator => {
                self.escape = match character {
                    '\\' | '\u{7}' | '\u{9c}' => Escape::Text,
                    '\u{1b}' => Escape::OscTerminator,
                    _ => Escape::Osc,
                }
            }
            Escape::String => match character {
                '\u{9c}' => self.escape = Escape::Text,
                '\u{1b}' => self.escape = Escape::StringTerminator,
                _ => {}
            },
            Escape::StringTerminator => {
                self.escape = match character {
                    '\\' | '\u{9c}' => Escape::Text,
                    '\u{1b}' => Escape::StringTerminator,
                    _ => Escape::String,
                }
            }
        }
    }
    pub fn finish(&mut self, truncated: bool) -> String {
        if truncated {
            self.line.clear();
            self.pending_utf8.clear();
            String::new()
        } else {
            let mut output = String::new();
            if !self.pending_utf8.is_empty() {
                self.pending_utf8.clear();
                self.character('\u{fffd}', &mut output);
            }
            output.push_str(&self.flush_line());
            output
        }
    }
    fn flush_line(&mut self) -> String {
        // redact_diagnostics drops CR through lines(); restore the original line ending.
        let has_cr = self.line.ends_with('\r');
        let mut redacted = redact_diagnostics(self.line.trim_end_matches('\r'));
        self.line.clear();
        if has_cr {
            redacted.push('\r');
        }
        redacted
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_designation_and_unicode_c1_sequences_are_stripped_without_corrupting_utf8() {
        let source = "\x1b(B繁磛\u{009b}31m红\u{009b}0m\u{009d}8;;https://hidden.test\u{009c}link\u{0090}hidden\x07still hidden\u{009c}\n";
        for size in 1..=source.len() {
            let mut stream = ConsoleOutput::default();
            let mut output = String::new();
            for chunk in source.as_bytes().chunks(size) {
                output.push_str(&stream.push(chunk));
            }
            output.push_str(&stream.finish(false));
            assert_eq!(output, "繁磛红link\n", "chunk size {size}");
        }
        let mut stream = ConsoleOutput::default();
        assert_eq!(stream.push(b"\xc2X\n\xc2"), "�X\n");
        assert_eq!(stream.finish(false), "�");
    }
    #[test]
    fn utf8_controls_credentials_and_empty_lines_survive_arbitrary_chunking() {
        let source = "\x1b[31m中文\x1b[0m\n\n\x1b]8;;https://evil.test\x07link\x1b]8;;\x1b\\\nhttps://user:secret@example.test/repo?token=secret&name=safe\nAuthorization: Bearer secret\n{\"apiKey\":\"secret\"}\nlast\u{fffd}";
        let expected = "中文\n\nlink\nhttps://[REDACTED]@example.test/repo?token=[REDACTED]&name=safe\nAuthorization: Bearer [REDACTED]\n{\"apiKey\":\"[REDACTED]\"}\nlast�";
        for size in 1..=source.len() {
            let mut stream = ConsoleOutput::default();
            let mut output = String::new();
            for chunk in source.as_bytes().chunks(size) {
                output.push_str(&stream.push(chunk));
            }
            output.push_str(&stream.finish(false));
            assert_eq!(output, expected, "chunk size {size}");
        }
    }
    #[test]
    fn truncated_partial_lines_never_publish_partial_secrets() {
        let mut stream = ConsoleOutput::default();
        let output = stream.push(b"safe\nhttps://user:secret");
        assert_eq!(output, "safe\n");
        assert_eq!(stream.finish(true), "");
    }
    #[test]
    fn invalid_utf8_is_replaced_and_crlf_and_tabs_are_preserved() {
        let mut stream = ConsoleOutput::default();
        assert_eq!(stream.push(b"a\xff\t\r\n\r\n"), "a�\t\r\n\r\n");
        assert_eq!(stream.finish(false), "");
    }
}
