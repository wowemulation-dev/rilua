//! Lexer: tokenizes Lua source code.
//!
//! Converts a source string into a sequence of tokens. Handles all Lua 5.1.1
//! lexical elements: reserved words, operators, identifiers, numbers (decimal,
//! hex, scientific), strings (short with escapes, long), and comments.
//!
//! Line tracking handles all four newline variants: `\n`, `\r`, `\r\n`, `\n\r`.
//!
//! Supports two input modes:
//! - **Byte slice**: the entire source is available upfront (`Lexer::new`)
//! - **Reader function**: data is pulled on demand (`Lexer::from_reader`),
//!   matching PUC-Rio's ZIO model where `luaZ_fill` calls the reader when
//!   the buffer is exhausted.

use crate::error::{LuaError, LuaResult, SyntaxError};
use crate::vm::execute::{libc_strtod, locale_decimal_point};

use super::token::{Span, Token, lookup_keyword};

/// Reader function type for streaming input.
///
/// Called when the lexer's buffer is exhausted and more data is needed.
/// Returns `Ok(Some(data))` for new data, `Ok(None)` for end-of-input,
/// or `Err(e)` for reader errors.
type ReaderFn<'a> = Box<dyn FnMut() -> Result<Option<Vec<u8>>, LuaError> + 'a>;

/// Lexer state for tokenizing Lua source code.
pub struct Lexer<'a> {
    /// Source bytes (grows when using a reader function).
    source: Vec<u8>,
    /// Current read position.
    pos: usize,
    /// Current line number (1-based).
    line: u32,
    /// Current column number (1-based).
    column: u32,
    /// Source name for error messages.
    source_name: String,
    /// Lookahead token (if peeked).
    lookahead: Option<(Token, Span)>,
    /// Raw source text of the last-scanned token (PUC-Rio: `luaZ_buffer(ls->buff)`).
    /// Populated for Number tokens where the lexeme differs from Display.
    last_token_text: String,
    /// Reader function for streaming input (None for byte slice mode).
    reader: Option<ReaderFn<'a>>,
    /// Error stored from a failed reader call.
    /// Checked at the Eos point in `scan()` to propagate reader errors.
    reader_error: Option<LuaError>,
    /// Whether the reader has signaled end-of-input.
    reader_exhausted: bool,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given source bytes.
    ///
    /// Source is accepted as `&[u8]` because Lua files may contain arbitrary
    /// byte sequences (e.g. `\0`, `\255` in string literals).
    pub fn new(source: &[u8], name: &str) -> Self {
        // Skip shebang line (PUC-Rio's luaL_loadfile skips leading `#` line).
        let (start, start_line) = if source.starts_with(b"#") {
            let end = source
                .iter()
                .position(|&b| b == b'\n' || b == b'\r')
                .map_or(source.len(), |p| p + 1);
            (end, 2)
        } else {
            (0, 1)
        };
        Self {
            source: source.to_vec(),
            pos: start,
            line: start_line,
            column: 1,
            source_name: name.to_string(),
            lookahead: None,
            last_token_text: String::new(),
            reader: None,
            reader_error: None,
            reader_exhausted: false,
        }
    }

    /// Creates a new lexer that pulls data on demand from a reader function.
    ///
    /// The reader is called whenever the internal buffer is exhausted,
    /// matching PUC-Rio's ZIO model. The initial buffer contains `first_chunk`
    /// (which may be empty).
    ///
    /// No shebang handling is performed for reader-based lexers, matching
    /// PUC-Rio where shebang skipping is done by `luaL_loadfile`, not
    /// `lua_load`.
    pub fn from_reader(
        first_chunk: Vec<u8>,
        reader: impl FnMut() -> Result<Option<Vec<u8>>, LuaError> + 'a,
        name: &str,
    ) -> Self {
        Self {
            source: first_chunk,
            pos: 0,
            line: 1,
            column: 1,
            source_name: name.to_string(),
            lookahead: None,
            last_token_text: String::new(),
            reader: Some(Box::new(reader)),
            reader_error: None,
            reader_exhausted: false,
        }
    }

    /// Returns the current line number.
    #[must_use]
    pub fn line(&self) -> u32 {
        self.line
    }

    /// Returns the source name.
    #[must_use]
    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    /// Returns the raw source text of the last scanned token.
    /// Used by the parser for error messages (PUC-Rio: `txtToken`).
    #[must_use]
    pub fn last_token_text(&self) -> &str {
        &self.last_token_text
    }

    // -- Buffer management --

    /// Attempts to refill the buffer from the reader function.
    ///
    /// Returns `true` if data was added, `false` if the reader returned
    /// None (end-of-input) or an error. Errors are stored in `reader_error`
    /// for later retrieval.
    fn refill(&mut self) -> bool {
        if self.reader_exhausted {
            return false;
        }
        let Some(reader) = self.reader.as_mut() else {
            return false;
        };
        match reader() {
            Ok(Some(data)) => {
                if data.is_empty() {
                    // Empty chunk signals end of input (PUC-Rio behavior).
                    self.reader_exhausted = true;
                    false
                } else {
                    self.source.extend_from_slice(&data);
                    true
                }
            }
            Ok(None) => {
                self.reader_exhausted = true;
                false
            }
            Err(e) => {
                self.reader_error = Some(e);
                self.reader_exhausted = true;
                false
            }
        }
    }

    /// Ensures at least `needed` bytes are available from `self.pos`.
    ///
    /// Calls `refill()` repeatedly until enough data is buffered or the
    /// reader is exhausted. Returns `true` if the requested amount is
    /// available.
    fn ensure_available(&mut self, needed: usize) -> bool {
        while self.pos + needed > self.source.len() {
            if !self.refill() {
                return false;
            }
        }
        true
    }

    // -- Character primitives --

    #[inline]
    fn peek(&mut self) -> Option<u8> {
        if self.pos < self.source.len() {
            return Some(self.source[self.pos]);
        }
        // Try to get more data from reader.
        if self.refill() && self.pos < self.source.len() {
            Some(self.source[self.pos])
        } else {
            None
        }
    }

    fn peek_ahead(&mut self, offset: usize) -> Option<u8> {
        let target = self.pos + offset;
        if target < self.source.len() {
            return Some(self.source[target]);
        }
        // Try to get enough data from reader.
        if self.ensure_available(offset + 1) {
            Some(self.source[self.pos + offset])
        } else {
            None
        }
    }

    #[inline]
    fn advance(&mut self) -> Option<u8> {
        // Ensure at least one byte is available.
        if self.pos >= self.source.len() && !self.refill() {
            return None;
        }
        let ch = self.source[self.pos];
        self.pos += 1;
        if ch == b'\n' || ch == b'\r' {
            // Don't update column here; newlines are handled by inc_line()
        } else {
            self.column += 1;
        }
        // Eagerly refill when buffer is exhausted (matches PUC-Rio's next(ls)
        // which always reads the next char after consuming one).
        if self.reader.is_some() && self.pos >= self.source.len() {
            self.refill();
        }
        Some(ch)
    }

    fn inc_line(&mut self) {
        let old = self.peek();
        self.pos += 1; // consume the newline char
        // Handle \r\n and \n\r pairs
        if let Some(next) = self.peek()
            && (next == b'\n' || next == b'\r')
            && next != old.unwrap_or(0)
        {
            self.pos += 1;
        }
        self.line += 1;
        self.column = 1;
    }

    fn current_span(&self) -> Span {
        Span::new(self.line, self.column)
    }

    fn syntax_error(&self, msg: &str) -> LuaError {
        LuaError::Syntax(SyntaxError {
            message: msg.to_string(),
            source: self.source_name.clone(),
            line: self.line,
            raw_message: None,
        })
    }

    fn syntax_error_near(&self, msg: &str, near: &str) -> LuaError {
        LuaError::Syntax(SyntaxError {
            message: format!("{msg} near '{near}'"),
            source: self.source_name.clone(),
            line: self.line,
            raw_message: None,
        })
    }

    // -- Main scanning --

    /// Returns the next token and its source span.
    ///
    /// If a lookahead token was peeked, returns that instead.
    #[allow(clippy::should_implement_trait)] // Lexer::next is not an Iterator
    pub fn next(&mut self) -> LuaResult<(Token, Span)> {
        if let Some(la) = self.lookahead.take() {
            return Ok(la);
        }
        self.scan()
    }

    /// Peeks at the next token without consuming it.
    pub fn lookahead(&mut self) -> LuaResult<&Token> {
        if self.lookahead.is_none() {
            self.lookahead = Some(self.scan()?);
        }
        // The option is always Some after the line above.
        Ok(&self
            .lookahead
            .as_ref()
            .ok_or_else(|| self.syntax_error("unexpected error"))?
            .0)
    }

    /// Main scan loop: skip whitespace/comments, then dispatch on character.
    fn scan(&mut self) -> LuaResult<(Token, Span)> {
        loop {
            // Check for stored reader error before attempting to scan.
            if let Some(err) = self.reader_error.take() {
                return Err(err);
            }

            self.skip_whitespace();

            // Check again after whitespace skipping (may have triggered refill).
            if let Some(err) = self.reader_error.take() {
                return Err(err);
            }

            let span = self.current_span();
            let token_start = self.pos;

            let Some(ch) = self.peek() else {
                // End of input. If a reader error caused this, propagate it.
                if let Some(err) = self.reader_error.take() {
                    return Err(err);
                }
                return Ok((Token::Eos, span));
            };

            match ch {
                b'\n' | b'\r' => {
                    self.inc_line();
                }

                b'-' => {
                    self.advance();
                    if self.peek() != Some(b'-') {
                        return Ok((Token::Char(b'-'), span));
                    }
                    // Comment
                    self.advance(); // consume second '-'
                    if self.peek() == Some(b'[') {
                        let sep = self.count_sep();
                        if sep >= 0 {
                            self.read_long_string(sep, true)?;
                            continue;
                        }
                    }
                    // Short comment: skip to end of line
                    while let Some(c) = self.peek() {
                        if c == b'\n' || c == b'\r' {
                            break;
                        }
                        self.advance();
                    }
                }

                b'[' => {
                    let sep = self.count_sep();
                    if sep >= 0 {
                        let s = self.read_long_string(sep, false)?;
                        self.last_token_text =
                            String::from_utf8_lossy(&self.source[token_start..self.pos]).into();
                        return Ok((Token::Str(s), self.current_span()));
                    }
                    if sep == -1 {
                        return Err(self.syntax_error_near("invalid long string delimiter", "["));
                    }
                    // Just a single '[', not a long string delimiter
                    self.advance();
                    return Ok((Token::Char(b'['), span));
                }

                b'=' => {
                    self.advance();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        return Ok((Token::Eq, span));
                    }
                    return Ok((Token::Char(b'='), span));
                }

                b'<' => {
                    self.advance();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        return Ok((Token::Le, span));
                    }
                    return Ok((Token::Char(b'<'), span));
                }

                b'>' => {
                    self.advance();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        return Ok((Token::Ge, span));
                    }
                    return Ok((Token::Char(b'>'), span));
                }

                b'~' => {
                    self.advance();
                    if self.peek() == Some(b'=') {
                        self.advance();
                        return Ok((Token::Ne, span));
                    }
                    return Err(self.syntax_error_near("unexpected symbol", "~"));
                }

                b'.' => {
                    self.advance();
                    if self.peek() == Some(b'.') {
                        self.advance();
                        if self.peek() == Some(b'.') {
                            self.advance();
                            return Ok((Token::Dots, span));
                        }
                        return Ok((Token::Concat, span));
                    }
                    if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        let num = self.read_number(b'.')?;
                        return Ok((Token::Number(num), span));
                    }
                    return Ok((Token::Char(b'.'), span));
                }

                b'"' | b'\'' => {
                    let s = self.read_short_string(ch)?;
                    self.last_token_text =
                        String::from_utf8_lossy(&self.source[token_start..self.pos]).into();
                    return Ok((Token::Str(s), self.current_span()));
                }

                _ if ch.is_ascii_digit() => {
                    let num = self.read_number(ch)?;
                    return Ok((Token::Number(num), span));
                }

                _ if ch.is_ascii_alphabetic() || ch == b'_' => {
                    let (start, end) = self.scan_name_extent();
                    let name_bytes = &self.source[start..end];
                    // O(1) keyword check on raw bytes -- no allocation needed for keywords.
                    if let Some(kw) = lookup_keyword(name_bytes) {
                        return Ok((kw, span));
                    }
                    // Identifiers are ASCII, so from_utf8 cannot fail.
                    let name = String::from_utf8(name_bytes.to_vec())
                        .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                    return Ok((Token::Name(name), span));
                }

                _ => {
                    self.advance();
                    return Ok((Token::Char(ch), span));
                }
            }
        }
    }

    #[inline]
    fn skip_whitespace(&mut self) {
        // Fast path: scan directly in the byte slice without per-character peek/advance.
        if self.reader.is_none() {
            let src = &self.source;
            let mut p = self.pos;
            while p < src.len() {
                let c = src[p];
                if c == b' ' || c == b'\t' || c == 0x0C || c == 0x0B {
                    p += 1;
                } else {
                    break;
                }
            }
            let skipped = p - self.pos;
            self.column += skipped as u32;
            self.pos = p;
            return;
        }
        // Slow path: reader mode, per-character.
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == 0x0C /* form feed */ || c == 0x0B
            /* vertical tab */
            {
                self.advance();
            } else {
                break;
            }
        }
    }

    // -- Names and keywords --

    /// Scans the extent of an identifier (alphanumeric + underscore) and
    /// returns `(start, end)` byte positions. Advances the lexer position.
    /// Does NOT allocate a String.
    #[inline]
    fn scan_name_extent(&mut self) -> (usize, usize) {
        let start = self.pos;
        // Fast path: scan directly in the byte slice.
        if self.reader.is_none() {
            let src = &self.source;
            let mut p = self.pos;
            while p < src.len() {
                let c = src[p];
                if c.is_ascii_alphanumeric() || c == b'_' {
                    p += 1;
                } else {
                    break;
                }
            }
            let len = p - self.pos;
            self.column += len as u32;
            self.pos = p;
            return (start, p);
        }
        // Slow path: reader mode.
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        (start, self.pos)
    }

    // -- Numbers --

    /// Reads a number token, matching PUC-Rio's `read_numeral`.
    ///
    /// Greedily collects digits and dots in a single loop (matching
    /// PUC-Rio's `do { save_and_next; } while (isdigit || '.')`), then
    /// exponent, then trailing alphanumeric. Uses libc `strtod` for
    /// locale-aware parsing with `trydecpoint` fallback.
    fn read_number(&mut self, first: u8) -> LuaResult<f64> {
        let start = if first == b'.' {
            self.pos - 1
        } else {
            self.pos
        };

        if first != b'.' {
            self.advance();
        }

        // PUC-Rio: greedily consume digits and dots in one loop.
        // This means "4.5." consumes both dots, producing "malformed number".
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
            self.advance();
        }

        // Check for hex prefix (0x/0X) -- hex digits may include a-f.
        let is_hex = self.pos - start >= 2
            && self.source[start] == b'0'
            && (self.source[start + 1] == b'x' || self.source[start + 1] == b'X');
        if is_hex {
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.advance();
            }
        }

        // Exponent (e/E for decimal, p/P not in Lua 5.1).
        if self.peek().is_some_and(|c| c == b'e' || c == b'E') {
            self.advance();
            if self.peek().is_some_and(|c| c == b'+' || c == b'-') {
                self.advance();
            }
        }

        // PUC-Rio: consume trailing alphanumeric/underscore (produces
        // "malformed number" for things like "1abc").
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            self.advance();
        }

        let num_bytes = &self.source[start..self.pos];
        let num_str = String::from_utf8_lossy(num_bytes);
        self.last_token_text = num_str.to_string();

        // Hex: parse with Rust (strtod doesn't handle 0x on all platforms).
        if is_hex {
            let hex_str = &num_str[2..];
            return match u64::from_str_radix(hex_str, 16) {
                Ok(v) => Ok(v as f64),
                Err(_) => Err(self.syntax_error_near("malformed number", &num_str)),
            };
        }

        // Decimal: use strtod (locale-aware).
        if let Some((val, consumed)) = libc_strtod(num_bytes)
            && consumed == num_bytes.len()
        {
            return Ok(val);
        }

        // trydecpoint: replace '.' with the locale's decimal point and retry.
        let decpoint = locale_decimal_point();
        if decpoint != b'.' {
            let mut buf: Vec<u8> = num_bytes.to_vec();
            for b in &mut buf {
                if *b == b'.' {
                    *b = decpoint;
                }
            }
            if let Some((val, consumed)) = libc_strtod(&buf)
                && consumed == buf.len()
            {
                return Ok(val);
            }
        }

        Err(self.syntax_error_near("malformed number", &num_str))
    }

    // -- Strings --

    fn read_short_string(&mut self, delimiter: u8) -> LuaResult<Vec<u8>> {
        self.advance(); // consume opening delimiter
        let mut buf = Vec::new();

        loop {
            match self.peek() {
                None => {
                    return Err(self.syntax_error_near("unfinished string", "<eof>"));
                }
                Some(b'\n') | Some(b'\r') => {
                    return Err(self.syntax_error_near("unfinished string", "<string>"));
                }
                Some(c) if c == delimiter => {
                    self.advance(); // consume closing delimiter
                    break;
                }
                Some(b'\\') => {
                    self.advance(); // consume backslash
                    match self.peek() {
                        Some(b'a') => {
                            self.advance();
                            buf.push(0x07);
                        }
                        Some(b'b') => {
                            self.advance();
                            buf.push(0x08);
                        }
                        Some(b'f') => {
                            self.advance();
                            buf.push(0x0C);
                        }
                        Some(b'n') => {
                            self.advance();
                            buf.push(b'\n');
                        }
                        Some(b'r') => {
                            self.advance();
                            buf.push(b'\r');
                        }
                        Some(b't') => {
                            self.advance();
                            buf.push(b'\t');
                        }
                        Some(b'v') => {
                            self.advance();
                            buf.push(0x0B);
                        }
                        Some(b'\\') => {
                            self.advance();
                            buf.push(b'\\');
                        }
                        Some(b'"') => {
                            self.advance();
                            buf.push(b'"');
                        }
                        Some(b'\'') => {
                            self.advance();
                            buf.push(b'\'');
                        }
                        Some(b'\n') | Some(b'\r') => {
                            self.inc_line();
                            buf.push(b'\n');
                        }
                        Some(c) if c.is_ascii_digit() => {
                            // \ddd decimal escape (up to 3 digits, max 255)
                            let mut val: u32 = 0;
                            for _ in 0..3 {
                                if let Some(d) = self.peek() {
                                    if d.is_ascii_digit() {
                                        val = val * 10 + u32::from(d - b'0');
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                            if val > 255 {
                                return Err(
                                    self.syntax_error_near("escape sequence too large", "<string>")
                                );
                            }
                            buf.push(val as u8);
                        }
                        Some(c) => {
                            return Err(self.syntax_error_near(
                                &format!("invalid escape sequence '\\{}'", char::from(c)),
                                "<string>",
                            ));
                        }
                        None => {
                            return Err(self.syntax_error_near("unfinished string", "<eof>"));
                        }
                    }
                }
                Some(c) => {
                    self.advance();
                    buf.push(c);
                }
            }
        }

        Ok(buf)
    }

    // -- Long strings and comments --

    /// Counts the separator level for long strings/comments.
    /// Returns >= 0 for valid `[=*[` (number of `=` signs),
    /// -1 for `[=*` not followed by `[` (invalid delimiter),
    /// -2 for just `[` (single bracket, not a long string).
    fn count_sep(&mut self) -> i32 {
        debug_assert_eq!(self.source.get(self.pos).copied(), Some(b'['));
        let mut i = 1;
        let mut count = 0;
        while self.peek_ahead(i) == Some(b'=') {
            count += 1;
            i += 1;
        }
        if self.peek_ahead(i) == Some(b'[') {
            count
        } else if count > 0 {
            -1 // Had '=' signs but no closing '['
        } else {
            -2 // Just a single '['
        }
    }

    /// Reads a long string `[=*[...]=*]` or long comment.
    /// `sep` is the number of `=` signs. If `is_comment`, discards the content.
    fn read_long_string(&mut self, sep: i32, is_comment: bool) -> LuaResult<Vec<u8>> {
        // Consume opening delimiter: '[' '='*sep '['
        let count = 2 + sep as usize;
        for _ in 0..count {
            self.pos += 1;
            self.column += 1;
        }

        // Skip first newline if present (per Lua spec)
        if let Some(c) = self.peek()
            && (c == b'\n' || c == b'\r')
        {
            self.inc_line();
        }

        let mut buf = Vec::new();

        loop {
            match self.peek() {
                None => {
                    let what = if is_comment { "comment" } else { "string" };
                    return Err(self.syntax_error_near(&format!("unfinished long {what}"), "<eof>"));
                }
                Some(b'\n') | Some(b'\r') => {
                    buf.push(b'\n'); // normalize all newlines to \n
                    self.inc_line();
                }
                Some(b']') => {
                    if self.check_closing_long_bracket(sep) {
                        // Consume closing delimiter: ']' '='*sep ']'
                        let close_count = 2 + sep as usize;
                        for _ in 0..close_count {
                            self.pos += 1;
                            self.column += 1;
                        }
                        if is_comment {
                            return Ok(Vec::new());
                        }
                        return Ok(buf);
                    }
                    self.advance();
                    if !is_comment {
                        buf.push(b']');
                    }
                }
                Some(c) => {
                    self.advance();
                    if !is_comment {
                        buf.push(c);
                    }
                }
            }
        }
    }

    /// Checks if the current position has a closing long bracket `]=*]` with `sep` equals signs.
    fn check_closing_long_bracket(&mut self, sep: i32) -> bool {
        if self.peek() != Some(b']') {
            return false;
        }
        let mut i = 1;
        for _ in 0..sep {
            if self.peek_ahead(i) != Some(b'=') {
                return false;
            }
            i += 1;
        }
        self.peek_ahead(i) == Some(b']')
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::approx_constant,
    clippy::items_after_statements,
    clippy::useless_vec,
    clippy::if_not_else
)]
mod tests {
    use super::*;

    fn lex_all(source: &str) -> LuaResult<Vec<(Token, Span)>> {
        let mut lexer = Lexer::new(source.as_bytes(), "test");
        let mut tokens = Vec::new();
        loop {
            let (tok, span) = lexer.next()?;
            if tok == Token::Eos {
                tokens.push((tok, span));
                break;
            }
            tokens.push((tok, span));
        }
        Ok(tokens)
    }

    fn lex_tokens(source: &str) -> LuaResult<Vec<Token>> {
        Ok(lex_all(source)?.into_iter().map(|(t, _)| t).collect())
    }

    // -- Keyword tests --

    #[test]
    fn all_keywords() {
        let tokens = lex_tokens(
            "and break do else elseif end false for function if in local nil not or repeat return then true until while"
        ).unwrap();
        let expected = vec![
            Token::And,
            Token::Break,
            Token::Do,
            Token::Else,
            Token::ElseIf,
            Token::End,
            Token::False,
            Token::For,
            Token::Function,
            Token::If,
            Token::In,
            Token::Local,
            Token::Nil,
            Token::Not,
            Token::Or,
            Token::Repeat,
            Token::Return,
            Token::Then,
            Token::True,
            Token::Until,
            Token::While,
            Token::Eos,
        ];
        assert_eq!(tokens, expected);
    }

    #[test]
    fn keywords_case_sensitive() {
        let tokens = lex_tokens("And AND aNd").unwrap();
        assert_eq!(tokens[0], Token::Name("And".into()));
        assert_eq!(tokens[1], Token::Name("AND".into()));
        assert_eq!(tokens[2], Token::Name("aNd".into()));
    }

    // -- Identifier tests --

    #[test]
    fn identifiers() {
        let tokens = lex_tokens("foo _bar baz123 _").unwrap();
        assert_eq!(tokens[0], Token::Name("foo".into()));
        assert_eq!(tokens[1], Token::Name("_bar".into()));
        assert_eq!(tokens[2], Token::Name("baz123".into()));
        assert_eq!(tokens[3], Token::Name("_".into()));
    }

    // -- Operator tests --

    #[test]
    fn multi_char_operators() {
        let tokens = lex_tokens(".. ... == >= <= ~=").unwrap();
        assert_eq!(tokens[0], Token::Concat);
        assert_eq!(tokens[1], Token::Dots);
        assert_eq!(tokens[2], Token::Eq);
        assert_eq!(tokens[3], Token::Ge);
        assert_eq!(tokens[4], Token::Le);
        assert_eq!(tokens[5], Token::Ne);
    }

    #[test]
    fn single_char_operators() {
        let tokens = lex_tokens("+ - * / % ^ # < > = ( ) { } ; : , [ ]").unwrap();
        let expected_chars: Vec<u8> = b"+-*/%^#<>=(){};:,[]".to_vec();
        for (i, &ch) in expected_chars.iter().enumerate() {
            assert_eq!(tokens[i], Token::Char(ch), "mismatch at index {i}");
        }
    }

    #[test]
    fn tilde_alone_is_error() {
        assert!(lex_tokens("~").is_err());
    }

    // -- Number tests --

    #[test]
    fn integers() {
        let tokens = lex_tokens("0 42 1000000").unwrap();
        assert_eq!(tokens[0], Token::Number(0.0));
        assert_eq!(tokens[1], Token::Number(42.0));
        assert_eq!(tokens[2], Token::Number(1_000_000.0));
    }

    #[test]
    fn floats() {
        let tokens = lex_tokens("3.0 .5 5. 0.001").unwrap();
        assert_eq!(tokens[0], Token::Number(3.0));
        assert_eq!(tokens[1], Token::Number(0.5));
        assert_eq!(tokens[2], Token::Number(5.0));
        assert_eq!(tokens[3], Token::Number(0.001));
    }

    #[test]
    fn scientific_notation() {
        let tokens = lex_tokens("1e10 1.5E-3 2e+4").unwrap();
        assert_eq!(tokens[0], Token::Number(1e10));
        assert_eq!(tokens[1], Token::Number(1.5e-3));
        assert_eq!(tokens[2], Token::Number(2e4));
    }

    #[test]
    fn hex_numbers() {
        let tokens = lex_tokens("0xFF 0x10 0XAB").unwrap();
        assert_eq!(tokens[0], Token::Number(255.0));
        assert_eq!(tokens[1], Token::Number(16.0));
        assert_eq!(tokens[2], Token::Number(171.0));
    }

    // -- String tests --

    #[test]
    fn simple_strings() {
        let tokens = lex_tokens(r#""hello" 'world'"#).unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello".to_vec()));
        assert_eq!(tokens[1], Token::Str(b"world".to_vec()));
    }

    #[test]
    fn string_escapes() {
        let tokens = lex_tokens(r#""\a\b\f\n\r\t\v\\\"\'""#).unwrap();
        let expected = b"\x07\x08\x0C\n\r\t\x0B\\\"'";
        assert_eq!(tokens[0], Token::Str(expected.to_vec()));
    }

    #[test]
    fn string_decimal_escape() {
        let tokens = lex_tokens(r#""\65\066\127""#).unwrap();
        assert_eq!(tokens[0], Token::Str(b"AB\x7F".to_vec()));
    }

    #[test]
    fn string_decimal_escape_max() {
        let tokens = lex_tokens(r#""\255""#).unwrap();
        // \255 is a valid byte
        assert_ne!(tokens[0], Token::Eos);
    }

    #[test]
    fn string_decimal_escape_too_large() {
        assert!(lex_tokens(r#""\256""#).is_err());
    }

    #[test]
    fn string_newline_escape() {
        let source = "\"line1\\\nline2\"";
        let tokens = lex_tokens(source).unwrap();
        assert_eq!(tokens[0], Token::Str(b"line1\nline2".to_vec()));
    }

    #[test]
    fn unfinished_string() {
        assert!(lex_tokens("\"hello").is_err());
    }

    #[test]
    fn bare_newline_in_string() {
        assert!(lex_tokens("\"hello\nworld\"").is_err());
    }

    // -- Long string tests --

    #[test]
    fn long_string_level_0() {
        let tokens = lex_tokens("[[hello]]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello".to_vec()));
    }

    #[test]
    fn long_string_level_1() {
        let tokens = lex_tokens("[=[hello]=]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello".to_vec()));
    }

    #[test]
    fn long_string_level_2() {
        let tokens = lex_tokens("[==[hello]==]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello".to_vec()));
    }

    #[test]
    fn long_string_skips_first_newline() {
        let tokens = lex_tokens("[[\nhello]]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello".to_vec()));
    }

    #[test]
    fn long_string_normalizes_newlines() {
        let tokens = lex_tokens("[[\r\nhello\r\nworld]]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello\nworld".to_vec()));
    }

    #[test]
    fn long_string_no_escapes() {
        let tokens = lex_tokens(r"[[hello\n]]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello\\n".to_vec()));
    }

    #[test]
    fn long_string_nested_brackets() {
        let tokens = lex_tokens("[=[hello]world]=]").unwrap();
        assert_eq!(tokens[0], Token::Str(b"hello]world".to_vec()));
    }

    #[test]
    fn unfinished_long_string() {
        assert!(lex_tokens("[[hello").is_err());
    }

    #[test]
    fn invalid_long_string_delimiter() {
        assert!(lex_tokens("[=hello").is_err());
    }

    // -- Comment tests --

    #[test]
    fn short_comment() {
        let tokens = lex_tokens("x -- comment\ny").unwrap();
        assert_eq!(tokens[0], Token::Name("x".into()));
        assert_eq!(tokens[1], Token::Name("y".into()));
    }

    #[test]
    fn long_comment() {
        let tokens = lex_tokens("x --[[comment]] y").unwrap();
        assert_eq!(tokens[0], Token::Name("x".into()));
        assert_eq!(tokens[1], Token::Name("y".into()));
    }

    #[test]
    fn long_comment_multiline() {
        let tokens = lex_tokens("x --[[\ncomment\n]] y").unwrap();
        assert_eq!(tokens[0], Token::Name("x".into()));
        assert_eq!(tokens[1], Token::Name("y".into()));
    }

    #[test]
    fn long_comment_with_level() {
        let tokens = lex_tokens("x --[==[comment]==] y").unwrap();
        assert_eq!(tokens[0], Token::Name("x".into()));
        assert_eq!(tokens[1], Token::Name("y".into()));
    }

    // -- Line tracking tests --

    #[test]
    fn line_tracking() {
        let tokens = lex_all("x\ny\nz").unwrap();
        assert_eq!(tokens[0].1.line, 1);
        assert_eq!(tokens[1].1.line, 2);
        assert_eq!(tokens[2].1.line, 3);
    }

    #[test]
    fn line_tracking_crlf() {
        let tokens = lex_all("x\r\ny").unwrap();
        assert_eq!(tokens[0].1.line, 1);
        assert_eq!(tokens[1].1.line, 2);
    }

    #[test]
    fn line_tracking_cr() {
        let tokens = lex_all("x\ry").unwrap();
        assert_eq!(tokens[0].1.line, 1);
        assert_eq!(tokens[1].1.line, 2);
    }

    // -- Lookahead test --

    #[test]
    fn lookahead_does_not_consume() {
        let mut lexer = Lexer::new(b"x y", "test");
        let la = lexer.lookahead().unwrap().clone();
        assert_eq!(la, Token::Name("x".into()));
        let (tok, _) = lexer.next().unwrap();
        assert_eq!(tok, Token::Name("x".into()));
        let (tok2, _) = lexer.next().unwrap();
        assert_eq!(tok2, Token::Name("y".into()));
    }

    // -- EOF tests --

    #[test]
    fn empty_source() {
        let tokens = lex_tokens("").unwrap();
        assert_eq!(tokens, vec![Token::Eos]);
    }

    #[test]
    fn whitespace_only() {
        let tokens = lex_tokens("   \t  ").unwrap();
        assert_eq!(tokens, vec![Token::Eos]);
    }

    // -- Dot disambiguation --

    #[test]
    fn dot_disambiguation() {
        // Single dot
        let tokens = lex_tokens("a.b").unwrap();
        assert_eq!(tokens[0], Token::Name("a".into()));
        assert_eq!(tokens[1], Token::Char(b'.'));
        assert_eq!(tokens[2], Token::Name("b".into()));
    }

    #[test]
    fn dot_before_number() {
        // .5 is a number, not dot + 5
        let tokens = lex_tokens(".5").unwrap();
        assert_eq!(tokens[0], Token::Number(0.5));
    }

    // -- Shebang --

    #[test]
    fn shebang_line() {
        // Lua 5.1 skips the first line if it starts with '#'.
        // The lexer skips from '#' to end of line, then resumes on the next line.
        let tokens = lex_tokens("#!/usr/bin/lua\nreturn 1").unwrap();
        assert_eq!(tokens[0], Token::Return);
        assert_eq!(tokens[1], Token::Number(1.0));
    }

    #[test]
    fn shebang_line_only() {
        // A file containing only a shebang line produces just EOS.
        let tokens = lex_tokens("#!/usr/bin/env lua").unwrap();
        assert!(tokens.is_empty() || tokens[0] == Token::Eos);
    }

    // -- Mixed tokens --

    #[test]
    fn mixed_expression() {
        let tokens = lex_tokens("x = 1 + 2.5").unwrap();
        assert_eq!(tokens[0], Token::Name("x".into()));
        assert_eq!(tokens[1], Token::Char(b'='));
        assert_eq!(tokens[2], Token::Number(1.0));
        assert_eq!(tokens[3], Token::Char(b'+'));
        assert_eq!(tokens[4], Token::Number(2.5));
    }

    #[test]
    fn function_call() {
        let tokens = lex_tokens("print(\"hello\")").unwrap();
        assert_eq!(tokens[0], Token::Name("print".into()));
        assert_eq!(tokens[1], Token::Char(b'('));
        assert_eq!(tokens[2], Token::Str(b"hello".to_vec()));
        assert_eq!(tokens[3], Token::Char(b')'));
    }

    #[test]
    fn invalid_escape() {
        assert!(lex_tokens(r#""\z""#).is_err());
    }

    // -- Reader-based lexer tests --

    #[test]
    fn reader_basic() {
        let chunks = vec![b"x ".to_vec(), b"= ".to_vec(), b"1".to_vec()];
        let mut idx = 0;
        let reader = move || -> Result<Option<Vec<u8>>, LuaError> {
            if idx < chunks.len() {
                let chunk = chunks[idx].clone();
                idx += 1;
                Ok(Some(chunk))
            } else {
                Ok(None)
            }
        };
        let mut lexer = Lexer::from_reader(Vec::new(), reader, "test");
        let (t1, _) = lexer.next().unwrap();
        assert_eq!(t1, Token::Name("x".into()));
        let (t2, _) = lexer.next().unwrap();
        assert_eq!(t2, Token::Char(b'='));
        let (t3, _) = lexer.next().unwrap();
        assert_eq!(t3, Token::Number(1.0));
        let (t4, _) = lexer.next().unwrap();
        assert_eq!(t4, Token::Eos);
    }

    #[test]
    fn reader_one_byte_at_a_time() {
        let source = b"x = 1";
        let mut pos = 0;
        let reader = move || -> Result<Option<Vec<u8>>, LuaError> {
            if pos < source.len() {
                let byte = vec![source[pos]];
                pos += 1;
                Ok(Some(byte))
            } else {
                Ok(None)
            }
        };
        let mut lexer = Lexer::from_reader(Vec::new(), reader, "test");
        let (t1, _) = lexer.next().unwrap();
        assert_eq!(t1, Token::Name("x".into()));
        let (t2, _) = lexer.next().unwrap();
        assert_eq!(t2, Token::Char(b'='));
        let (t3, _) = lexer.next().unwrap();
        assert_eq!(t3, Token::Number(1.0));
    }

    #[test]
    fn reader_error_propagates() {
        let reader = move || -> Result<Option<Vec<u8>>, LuaError> {
            Err(LuaError::Runtime(crate::error::RuntimeError {
                message: "reader failed".to_string(),
                level: 0,
                traceback: vec![],
            }))
        };
        let mut lexer = Lexer::from_reader(Vec::new(), reader, "test");
        let result = lexer.next();
        assert!(result.is_err());
    }

    #[test]
    fn reader_empty_chunk_signals_eof() {
        let mut called = false;
        let reader = move || -> Result<Option<Vec<u8>>, LuaError> {
            if !called {
                called = true;
                Ok(Some(b"x".to_vec()))
            } else {
                Ok(Some(Vec::new())) // empty chunk = eof
            }
        };
        let mut lexer = Lexer::from_reader(Vec::new(), reader, "test");
        let (t1, _) = lexer.next().unwrap();
        assert_eq!(t1, Token::Name("x".into()));
        let (t2, _) = lexer.next().unwrap();
        assert_eq!(t2, Token::Eos);
    }
}
