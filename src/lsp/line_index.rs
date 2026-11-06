use std::ops;

use lsp_types::{Position, Range};

/// Converts between byte offsets into a text and LSP positions, whose characters are counted in
/// UTF-16 code units, the encoding every client supports
pub(crate) struct LineIndex<'a> {
    text: &'a str,
    /// Byte offset of the start of each line
    line_starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();

        LineIndex { text, line_starts }
    }

    /// Position of byte `offset`, clamped to the end of the text
    pub(crate) fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let start = self.line_starts[line];
        let character = self.text[start..offset]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>();

        Position::new(line as u32, character as u32)
    }

    /// Range of the bytes in `span`
    pub(crate) fn range(&self, span: &ops::Range<usize>) -> Range {
        Range::new(self.position(span.start), self.position(span.end))
    }

    /// Byte offset of `position`. Positions past the end of a line are clamped to its end, as the
    /// protocol requires, and positions past the last line to the end of the text.
    pub(crate) fn offset(&self, position: Position) -> usize {
        let Some(&start) = self.line_starts.get(position.line as usize) else {
            return self.text.len();
        };
        let end = self
            .line_starts
            .get(position.line as usize + 1)
            .map_or(self.text.len(), |&next| next - 1);

        let mut units = 0;
        for (i, c) in self.text[start..end].char_indices() {
            if units >= position.character as usize {
                return start + i;
            }
            units += c.len_utf16();
        }

        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ascii() {
        let index = LineIndex::new("ab\ncd\n");

        assert_eq!(index.position(0), Position::new(0, 0));
        assert_eq!(index.position(4), Position::new(1, 1));
        assert_eq!(index.position(6), Position::new(2, 0));
        assert_eq!(index.offset(Position::new(1, 1)), 4);
        assert_eq!(index.offset(Position::new(0, 99)), 2);
        assert_eq!(index.offset(Position::new(9, 0)), 6);
    }

    #[test]
    fn counts_utf16_code_units() {
        // `ä` is 2 bytes and 1 code unit, `🦀` 4 bytes and 2 code units
        let index = LineIndex::new("ä🦀x");

        assert_eq!(index.position(2), Position::new(0, 1));
        assert_eq!(index.position(6), Position::new(0, 3));
        assert_eq!(index.offset(Position::new(0, 1)), 2);
        assert_eq!(index.offset(Position::new(0, 3)), 6);
    }

    #[test]
    fn handles_last_line_without_newline() {
        let index = LineIndex::new("a\nbc");

        assert_eq!(index.position(4), Position::new(1, 2));
        assert_eq!(index.position(99), Position::new(1, 2));
        assert_eq!(index.offset(Position::new(1, 2)), 4);
    }
}
