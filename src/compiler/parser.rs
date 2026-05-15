//! Parser: transforms a token stream into an AST.
//!
//! Recursive descent parser following PUC-Rio's `lparser.c` structure.
//! Uses Pratt parsing for expressions with operator precedence.
//! Error messages match PUC-Rio's format for compatibility.

use crate::error::{LuaError, LuaResult, SyntaxError, chunkid};
use crate::vm::instructions::{LUAI_MAXUPVALUES, LUAI_MAXVARS};

use super::ast::{BinOp, Block, Expr, FuncBody, FuncName, Stat, TableField, UnOp};
use super::lexer::Lexer;
use super::token::{Span, Token};

/// Per-function scope tracking for local/upvalue limit checking.
/// Mirrors PUC-Rio's FuncState fields (nactvar, upvalues[]).
/// Needed because PUC-Rio's single-pass design catches these limits
/// before encountering missing `end` keywords in intentionally
/// incomplete code (e.g., the PUC-Rio test suite).
struct FuncScope {
    /// Line where this function was defined (0 for top-level chunk).
    line_defined: u32,
    /// Current count of registered local variables in this function.
    local_count: u32,
    /// Names of currently active local variables in this function.
    /// Used for upvalue resolution: when a name isn't found here,
    /// it's looked up in parent scopes.
    local_names: Vec<String>,
    /// Names of upvalues already resolved for this function.
    /// Prevents double-counting the same upvalue from multiple references.
    upvalue_names: Vec<String>,
}

/// Parser state.
pub struct Parser<'a> {
    /// Lexer providing the token stream.
    lexer: Lexer<'a>,
    /// Current token.
    current: Token,
    /// Span of the current token.
    span: Span,
    /// Line of the last consumed token (PUC-Rio: `ls->lastline`).
    /// Set in `advance()` before reading the next token.
    lastline: u32,
    /// Nesting depth of loop constructs (while, repeat, for).
    /// Used to validate that `break` only appears inside a loop.
    loop_depth: u32,
    /// Syntax nesting depth (PUC-Rio: `L->nCcalls` in `enterlevel`/`leavelevel`).
    /// Prevents stack overflow from deeply nested syntax constructs.
    syntax_depth: u32,
    /// Stack of function scopes for local variable limit checking.
    /// Each function (including the top-level chunk) gets its own scope.
    func_scopes: Vec<FuncScope>,
}

/// Maximum syntax nesting depth before "too many syntax levels" error.
/// PUC-Rio uses `LUAI_MAXCCALLS` (200) for this.
const MAX_SYNTAX_LEVELS: u32 = 200;

impl<'a> Parser<'a> {
    /// Creates a new parser for the given source bytes.
    pub fn new(source: &[u8], name: &str) -> LuaResult<Self> {
        let lexer = Lexer::new(source, name);
        Self::from_lexer(lexer)
    }

    /// Creates a new parser from a pre-built lexer.
    ///
    /// Used by `compile_with_reader` to pass a reader-based lexer.
    pub fn from_lexer(mut lexer: Lexer<'a>) -> LuaResult<Self> {
        let (current, span) = lexer.next()?;
        Ok(Self {
            lexer,
            current,
            span,
            lastline: 1,
            loop_depth: 0,
            syntax_depth: 0,
            // Top-level chunk is itself a function scope (line 0).
            func_scopes: vec![FuncScope {
                line_defined: 0,
                local_count: 0,
                local_names: Vec::new(),
                upvalue_names: Vec::new(),
            }],
        })
    }

    // -- Nesting depth (PUC-Rio: enterlevel/leavelevel) --

    /// Increments syntax nesting depth, erroring if too deep.
    fn enter_level(&mut self) -> LuaResult<()> {
        self.syntax_depth += 1;
        if self.syntax_depth > MAX_SYNTAX_LEVELS {
            return Err(self.syntax_error("chunk has too many syntax levels"));
        }
        Ok(())
    }

    /// Decrements syntax nesting depth.
    fn leave_level(&mut self) {
        self.syntax_depth -= 1;
    }

    // -- Function scope tracking (PUC-Rio: FuncState.nactvar) --

    /// Registers `count` new local variables in the current function scope.
    /// Returns an error if the total exceeds `LUAI_MAXVARS` (200).
    /// Matches PUC-Rio's `new_localvar` limit check.
    fn register_locals(&mut self, count: u32) -> LuaResult<()> {
        if let Some(scope) = self.func_scopes.last_mut() {
            if scope.local_count + count > LUAI_MAXVARS {
                let msg = format!(
                    "function at line {} has more than {} local variables",
                    scope.line_defined, LUAI_MAXVARS
                );
                return Err(self.syntax_error(&msg));
            }
            scope.local_count += count;
        }
        Ok(())
    }

    /// Registers named local variables (tracks both count and names).
    /// Used when names are available (local declarations, function params).
    fn register_locals_named(&mut self, names: &[String]) -> LuaResult<()> {
        self.register_locals(names.len() as u32)?;
        if let Some(scope) = self.func_scopes.last_mut() {
            scope.local_names.extend(names.iter().cloned());
        }
        Ok(())
    }

    /// Checks if a name reference creates upvalues in the function scope chain.
    /// Mirrors PUC-Rio's `singlevar` -> `indexupvalue` cascade.
    /// When a name is found in a parent scope, all intermediate function
    /// scopes gain an upvalue entry.
    fn check_name_upvalue(&mut self, name: &str) -> LuaResult<()> {
        let n_scopes = self.func_scopes.len();
        if n_scopes < 2 {
            return Ok(()); // top-level scope has no upvalues
        }

        // Check if name is a local in the current (innermost) function scope.
        let current = n_scopes - 1;
        if self.func_scopes[current]
            .local_names
            .contains(&name.to_string())
        {
            return Ok(()); // local variable, not an upvalue
        }
        // Already tracked as an upvalue of the current scope?
        if self.func_scopes[current]
            .upvalue_names
            .contains(&name.to_string())
        {
            return Ok(()); // already counted
        }

        // Walk up the chain to find where this name is defined.
        let mut found_at = None;
        for i in (0..current).rev() {
            if self.func_scopes[i].local_names.contains(&name.to_string())
                || self.func_scopes[i]
                    .upvalue_names
                    .contains(&name.to_string())
            {
                found_at = Some(i);
                break;
            }
        }

        // If not found anywhere, it's a global -- no upvalue created.
        let Some(found_level) = found_at else {
            return Ok(());
        };

        // Cascade: add upvalue to all intermediate scopes from found_level+1
        // up to and including the current scope.
        let name_owned = name.to_string();
        for i in (found_level + 1)..=current {
            if !self.func_scopes[i].upvalue_names.contains(&name_owned) {
                let scope = &self.func_scopes[i];
                if scope.upvalue_names.len() >= LUAI_MAXUPVALUES as usize {
                    let msg = format!(
                        "function at line {} has more than {} upvalues",
                        scope.line_defined, LUAI_MAXUPVALUES
                    );
                    return Err(self.syntax_error(&msg));
                }
                self.func_scopes[i].upvalue_names.push(name_owned.clone());
            }
        }
        Ok(())
    }

    // -- Token helpers --

    /// Advances to the next token, returning the previous one.
    fn advance(&mut self) -> LuaResult<(Token, Span)> {
        // Move the current token out instead of cloning. Token::Eos is a
        // zero-cost sentinel that is immediately overwritten below.
        let prev_token = std::mem::replace(&mut self.current, Token::Eos);
        let prev_span = self.span;
        // PUC-Rio: ls->lastline = ls->linenumber (before reading next token)
        self.lastline = prev_span.line;
        let (tok, span) = self.lexer.next()?;
        self.current = tok;
        self.span = span;
        Ok((prev_token, prev_span))
    }

    /// Returns true if the current token matches the expected token.
    fn check(&self, expected: &Token) -> bool {
        self.current == *expected
    }

    /// Returns true if the current token is a `Char` with the given byte.
    fn check_char(&self, ch: u8) -> bool {
        self.current == Token::Char(ch)
    }

    /// Consumes the current token if it matches, returns true if consumed.
    fn test_next(&mut self, expected: &Token) -> LuaResult<bool> {
        if self.check(expected) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Consumes the current token if it's a `Char` matching `ch`.
    fn test_next_char(&mut self, ch: u8) -> LuaResult<bool> {
        self.test_next(&Token::Char(ch))
    }

    /// Expects the current token to match, or returns an error.
    fn expect(&mut self, expected: &Token) -> LuaResult<Span> {
        if self.check(expected) {
            let span = self.span;
            self.advance()?;
            Ok(span)
        } else {
            Err(self.error_expected(&expected.token2str()))
        }
    }

    /// Expects a `Char` token with the given byte.
    fn expect_char(&mut self, ch: u8) -> LuaResult<Span> {
        self.expect(&Token::Char(ch))
    }

    /// Expects a `Name` token and returns the name string.
    fn expect_name(&mut self) -> LuaResult<(String, Span)> {
        let span = self.span;
        match &self.current {
            Token::Name(_) => {
                let (tok, _) = self.advance()?;
                if let Token::Name(name) = tok {
                    Ok((name, span))
                } else {
                    // Unreachable: we checked above
                    Err(self.error_expected("<name>"))
                }
            }
            _ => Err(self.error_expected("<name>")),
        }
    }

    /// Expects a closing delimiter and produces PUC-Rio style error messages.
    ///
    /// If the opening and closing tokens are on the same line, emits a simple
    /// `"'X' expected"` error. Otherwise, emits
    /// `"'X' expected (to close 'Y' at line N)"`.
    fn check_match(&mut self, close: &Token, open: &Token, open_line: u32) -> LuaResult<()> {
        if !self.check(close) {
            if open_line == self.lexer.line() {
                return Err(self.error_expected(&close.token2str()));
            }
            // PUC-Rio's check_match calls luaX_syntaxerror which always
            // appends "near <token>". We must include this for the REPL's
            // incomplete chunk detection (checks for "<eof>" at end).
            return Err(self.syntax_error_near(&format!(
                "'{}' expected (to close '{}' at line {open_line})",
                close.token2str(),
                open.token2str(),
            )));
        }
        self.advance()?;
        Ok(())
    }

    // -- Error helpers --

    /// Creates a syntax error without a `near` token suffix.
    /// Use `syntax_error_near` for PUC-Rio's `luaX_syntaxerror` equivalent.
    fn syntax_error(&self, msg: &str) -> LuaError {
        LuaError::Syntax(SyntaxError {
            message: msg.to_string(),
            source: self.lexer.source_name().to_string(),
            line: self.span.line,
            raw_message: None,
        })
    }

    /// Creates a syntax error with `near '<current_token>'` appended.
    /// Matches PUC-Rio's `luaX_syntaxerror` -> `luaX_lexerror(msg, token)`.
    fn syntax_error_near(&self, msg: &str) -> LuaError {
        // PUC-Rio's txtToken uses the raw lexer buffer for Name/Number/String,
        // preserving the original lexeme (e.g. "1.000" instead of "1").
        let near = match self.current {
            Token::Number(_) | Token::Str(_) => {
                let text = self.lexer.last_token_text();
                format!("'{text}'")
            }
            _ => self.current.txt_token(),
        };

        // For non-ASCII byte characters, build a raw_message to preserve the
        // actual byte value. Rust's char::from(0xFF) produces U+00FF which is
        // 2 bytes in UTF-8 (0xC3, 0xBF), but PUC-Rio's C strings keep the
        // raw byte. We build the full "source:line: msg near 'BYTE'" as raw
        // bytes so that loadstring can push it onto the Lua stack verbatim.
        let raw_message = if let Token::Char(b) = self.current {
            if b.is_ascii() {
                None
            } else {
                let source = chunkid(self.lexer.source_name());
                let mut raw = Vec::new();
                raw.extend_from_slice(source.as_bytes());
                raw.push(b':');
                raw.extend_from_slice(self.span.line.to_string().as_bytes());
                raw.extend_from_slice(b": ");
                raw.extend_from_slice(msg.as_bytes());
                raw.extend_from_slice(b" near '");
                raw.push(b);
                raw.push(b'\'');
                Some(raw)
            }
        } else {
            None
        };

        LuaError::Syntax(SyntaxError {
            message: format!("{msg} near {near}"),
            source: self.lexer.source_name().to_string(),
            line: self.span.line,
            raw_message,
        })
    }

    fn error_expected(&self, what: &str) -> LuaError {
        // PUC-Rio: luaO_pushfstring(LUA_QS " expected", token2str(token))
        // then luaX_syntaxerror adds "near" LUA_QS with txtToken.
        self.syntax_error_near(&format!("'{what}' expected"))
    }

    // -- Parsing entry point --

    /// Parses a complete Lua chunk (the top-level block).
    pub fn parse_chunk(&mut self) -> LuaResult<Block> {
        let block = self.parse_block()?;
        if !self.check(&Token::Eos) {
            return Err(self.error_expected("<eof>"));
        }
        Ok(block)
    }

    /// Parses a block (sequence of statements).
    fn parse_block(&mut self) -> LuaResult<Block> {
        // chunk -> { stat [`;'] }
        // Lua 5.1: semicolons are optional separators after statements,
        // NOT empty statements. PUC-Rio: testnext(ls, ';') only after stat.
        self.enter_level()?;

        // Save local variable state for block scoping (PUC-Rio: enterblock).
        // When the block ends, locals declared inside go out of scope.
        let saved_locals = self
            .func_scopes
            .last()
            .map(|s| (s.local_count, s.local_names.len()));

        let mut stmts = Vec::new();
        loop {
            if self.current.is_block_follow() {
                break;
            }

            let stmt = self.parse_stat()?;
            let is_last = matches!(stmt, Stat::Return { .. } | Stat::Break { .. });
            stmts.push(stmt);

            // Optional semicolon after statement
            self.test_next_char(b';')?;

            if is_last {
                break;
            }
        }

        // Restore local variable state (PUC-Rio: leaveblock -> removevars).
        if let Some((count, names_len)) = saved_locals
            && let Some(scope) = self.func_scopes.last_mut()
        {
            scope.local_count = count;
            scope.local_names.truncate(names_len);
        }

        self.leave_level();
        Ok(stmts)
    }

    // -- Statement parsing --

    fn parse_stat(&mut self) -> LuaResult<Stat> {
        let span = self.span;
        match &self.current {
            Token::If => self.parse_if(span),
            Token::While => self.parse_while(span),
            Token::Do => self.parse_do(span),
            Token::For => self.parse_for(span),
            Token::Repeat => self.parse_repeat(span),
            Token::Function => self.parse_func_decl(span),
            Token::Local => self.parse_local(span),
            Token::Return => self.parse_return(span),
            Token::Break => self.parse_break(span),
            _ => self.parse_expr_stat(span),
        }
    }

    fn parse_if(&mut self, span: Span) -> LuaResult<Stat> {
        // if expr then block {elseif expr then block} [else block] end
        self.advance()?; // consume 'if'
        let open_line = span.line;

        let mut conditions = Vec::new();
        let mut bodies = Vec::new();

        // First condition + body
        conditions.push(self.parse_expr()?);
        self.expect(&Token::Then)?;
        bodies.push(self.parse_block()?);

        // elseif chains
        while self.check(&Token::ElseIf) {
            self.advance()?; // consume 'elseif'
            conditions.push(self.parse_expr()?);
            self.expect(&Token::Then)?;
            bodies.push(self.parse_block()?);
        }

        // Optional else
        let else_body = if self.test_next(&Token::Else)? {
            Some(self.parse_block()?)
        } else {
            None
        };

        // Capture the `end` keyword's line before consuming it.
        // PUC-Rio: lastline is updated when check_match consumes `end`.
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::If, open_line)?;

        Ok(Stat::If {
            conditions,
            bodies,
            else_body,
            span,
            end_line,
        })
    }

    fn parse_while(&mut self, span: Span) -> LuaResult<Stat> {
        // while expr do block end
        self.advance()?; // consume 'while'
        let open_line = span.line;

        let condition = self.parse_expr()?;
        self.expect(&Token::Do)?;
        self.loop_depth += 1;
        let body = self.parse_block()?;
        self.loop_depth -= 1;
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::While, open_line)?;

        Ok(Stat::While {
            condition,
            body,
            span,
            end_line,
        })
    }

    fn parse_do(&mut self, span: Span) -> LuaResult<Stat> {
        // do block end
        self.advance()?; // consume 'do'
        let open_line = span.line;

        let body = self.parse_block()?;
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::Do, open_line)?;

        Ok(Stat::Do {
            body,
            span,
            end_line,
        })
    }

    fn parse_for(&mut self, span: Span) -> LuaResult<Stat> {
        // for Name '=' ... (numeric) or for namelist in ... (generic)
        self.advance()?; // consume 'for'
        let open_line = span.line;

        let (name, _) = self.expect_name()?;

        match &self.current {
            Token::Char(b'=') => self.parse_numeric_for(name, open_line, span),
            Token::Char(b',') | Token::In => self.parse_generic_for(name, open_line, span),
            _ => Err(self.syntax_error_near("'=' or 'in' expected")),
        }
    }

    fn parse_numeric_for(&mut self, name: String, open_line: u32, span: Span) -> LuaResult<Stat> {
        // for name = start, stop [, step] do block end
        // PUC-Rio registers 4 locals: (for index), (for limit), (for step), name.
        // Track the user-visible name for upvalue resolution; the 3 implicit
        // locals have internal names that user code can't reference.
        self.register_locals(3)?;
        self.register_locals_named(std::slice::from_ref(&name))?;
        self.expect_char(b'=')?;
        let start = self.parse_expr()?;
        self.expect_char(b',')?;
        let stop = self.parse_expr()?;
        let step = if self.test_next_char(b',')? {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&Token::Do)?;
        self.loop_depth += 1;
        let body = self.parse_block()?;
        self.loop_depth -= 1;
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::For, open_line)?;

        Ok(Stat::NumericFor {
            name,
            start,
            stop,
            step,
            body,
            span,
            end_line,
        })
    }

    fn parse_generic_for(
        &mut self,
        first_name: String,
        open_line: u32,
        span: Span,
    ) -> LuaResult<Stat> {
        // for namelist in explist do block end
        let mut names = vec![first_name];
        while self.test_next_char(b',')? {
            let (name, _) = self.expect_name()?;
            names.push(name);
        }
        // PUC-Rio registers 3 implicit locals + user names:
        // (for generator), (for state), (for control), name1, name2, ...
        self.register_locals(3)?;
        self.register_locals_named(&names)?;
        self.expect(&Token::In)?;
        let iter_line = self.span.line;
        let iterators = self.parse_expr_list()?;
        self.expect(&Token::Do)?;
        self.loop_depth += 1;
        let body = self.parse_block()?;
        self.loop_depth -= 1;
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::For, open_line)?;

        Ok(Stat::GenericFor {
            names,
            iterators,
            body,
            iter_line,
            end_line,
            span,
        })
    }

    fn parse_repeat(&mut self, span: Span) -> LuaResult<Stat> {
        // repeat block until expr
        self.advance()?; // consume 'repeat'
        let open_line = span.line;

        self.loop_depth += 1;
        let body = self.parse_block()?;
        self.check_match(&Token::Until, &Token::Repeat, open_line)?;
        let condition = self.parse_expr()?;
        self.loop_depth -= 1;

        Ok(Stat::Repeat {
            body,
            condition,
            span,
        })
    }

    fn parse_func_decl(&mut self, span: Span) -> LuaResult<Stat> {
        // function funcname funcbody
        self.advance()?; // consume 'function'

        let name = self.parse_func_name()?;
        let body = self.parse_func_body(span)?;

        Ok(Stat::FuncDecl { name, body, span })
    }

    fn parse_func_name(&mut self) -> LuaResult<FuncName> {
        let span = self.span;
        let (first, _) = self.expect_name()?;
        let mut parts = vec![first];

        // foo.bar.baz
        while self.test_next_char(b'.')? {
            let (name, _) = self.expect_name()?;
            parts.push(name);
        }

        // Optional :method
        let method = if self.test_next_char(b':')? {
            let (name, _) = self.expect_name()?;
            Some(name)
        } else {
            None
        };

        Ok(FuncName {
            parts,
            method,
            span,
        })
    }

    fn parse_local(&mut self, span: Span) -> LuaResult<Stat> {
        // local function Name funcbody
        // local namelist ['=' explist]
        self.advance()?; // consume 'local'

        if self.test_next(&Token::Function)? {
            let (name, _) = self.expect_name()?;
            // The function name is a local variable.
            self.register_locals_named(std::slice::from_ref(&name))?;
            let body = self.parse_func_body(span)?;
            return Ok(Stat::LocalFunc { name, body, span });
        }

        // local namelist ['=' explist]
        let (first_name, _) = self.expect_name()?;
        let mut names = vec![first_name];
        while self.test_next_char(b',')? {
            let (name, _) = self.expect_name()?;
            names.push(name);
        }

        // Check local variable limit (PUC-Rio: new_localvar checks nactvar).
        self.register_locals_named(&names)?;

        let values = if self.test_next_char(b'=')? {
            self.parse_expr_list()?
        } else {
            Vec::new()
        };

        Ok(Stat::LocalDecl {
            names,
            values,
            span,
        })
    }

    fn parse_return(&mut self, span: Span) -> LuaResult<Stat> {
        // stat -> RETURN explist
        // PUC-Rio's retstat does NOT consume a trailing ';'.
        // The single optional ';' is consumed by chunk() after the stat.
        self.advance()?; // consume 'return'

        let values = if self.current.is_block_follow() || self.check_char(b';') {
            Vec::new()
        } else {
            self.parse_expr_list()?
        };

        Ok(Stat::Return { values, span })
    }

    fn parse_break(&mut self, span: Span) -> LuaResult<Stat> {
        self.advance()?; // consume 'break'
        if self.loop_depth == 0 {
            // PUC-Rio's breakstat validates that break appears inside a loop.
            // The error includes "near <current_token>" matching luaX_syntaxerror.
            return Err(self.syntax_error_near("no loop to break"));
        }
        Ok(Stat::Break { span })
    }

    fn parse_expr_stat(&mut self, span: Span) -> LuaResult<Stat> {
        // Assignment or function call
        let expr = self.parse_suffixed_expr()?;

        if self.check_char(b'=') || self.check_char(b',') {
            // Assignment: target1, target2, ... = expr1, expr2, ...
            let mut targets = vec![expr];
            while self.test_next_char(b',')? {
                targets.push(self.parse_suffixed_expr()?);
            }
            self.expect_char(b'=')?;
            let values = self.parse_expr_list()?;
            Ok(Stat::Assign {
                targets,
                values,
                span,
            })
        } else {
            // Function call used as statement.
            // PUC-Rio's exprstat only accepts calls; anything else
            // (bare name, literal, binop, etc.) is a syntax error.
            match &expr {
                super::ast::Expr::Call { .. } | super::ast::Expr::MethodCall { .. } => {
                    Ok(Stat::ExprStat { expr, span })
                }
                _ => Err(self.error_expected("=")),
            }
        }
    }

    // -- Expression parsing --
    // Full Pratt parsing is in chunk 2f. This provides the minimal
    // infrastructure needed for statement parsing.

    /// Parses a comma-separated list of one or more expressions.
    fn parse_expr_list(&mut self) -> LuaResult<Vec<Expr>> {
        let mut exprs = vec![self.parse_expr()?];
        while self.test_next_char(b',')? {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    /// Parses an expression using Pratt parsing.
    pub(crate) fn parse_expr(&mut self) -> LuaResult<Expr> {
        self.parse_sub_expr(0)
    }

    /// Pratt parser: parse sub-expression with minimum precedence `limit`.
    fn parse_sub_expr(&mut self, limit: u8) -> LuaResult<Expr> {
        self.enter_level()?;
        let span = self.span;

        // Unary prefix operators
        let mut expr = if let Some(op) = self.get_unary_op() {
            self.advance()?;
            let operand = self.parse_sub_expr(UNARY_PRIORITY)?;
            Expr::UnOp {
                op,
                operand: Box::new(operand),
                span,
            }
        } else {
            self.parse_simple_expr()?
        };

        // Binary operators (loop while priority > limit)
        while let Some(op) = self.get_binary_op() {
            let (left_prio, right_prio) = binary_priority(op);
            if left_prio <= limit {
                break;
            }
            self.advance()?; // consume operator
            let right = self.parse_sub_expr(right_prio)?;
            let new_span = span;
            expr = Expr::BinOp {
                op,
                left: Box::new(expr),
                right: Box::new(right),
                span: new_span,
            };
        }

        self.leave_level();
        Ok(expr)
    }

    /// Parse a simple (non-operator) expression.
    fn parse_simple_expr(&mut self) -> LuaResult<Expr> {
        let span = self.span;
        match &self.current {
            Token::Number(_) => {
                let (tok, _) = self.advance()?;
                if let Token::Number(n) = tok {
                    Ok(Expr::Number(n, span))
                } else {
                    Err(self.syntax_error("unexpected token"))
                }
            }
            Token::Str(_) => {
                let (tok, _) = self.advance()?;
                if let Token::Str(s) = tok {
                    Ok(Expr::Str(s, span))
                } else {
                    Err(self.syntax_error("unexpected token"))
                }
            }
            Token::Nil => {
                self.advance()?;
                Ok(Expr::Nil(span))
            }
            Token::True => {
                self.advance()?;
                Ok(Expr::True(span))
            }
            Token::False => {
                self.advance()?;
                Ok(Expr::False(span))
            }
            Token::Dots => {
                self.advance()?;
                Ok(Expr::VarArg(span))
            }
            Token::Function => {
                self.advance()?; // consume 'function'
                let body = self.parse_func_body(span)?;
                Ok(Expr::FuncDef { body, span })
            }
            Token::Char(b'{') => self.parse_table_ctor(),
            _ => self.parse_suffixed_expr(),
        }
    }

    /// Parse primary expression with suffix chain (calls, indexing, fields).
    fn parse_suffixed_expr(&mut self) -> LuaResult<Expr> {
        let mut expr = self.parse_primary_expr()?;

        loop {
            match &self.current {
                Token::Char(b'.') => {
                    // field access: expr.name
                    self.advance()?;
                    let (field, _) = self.expect_name()?;
                    let span = expr.span();
                    expr = Expr::Field {
                        table: Box::new(expr),
                        field,
                        span,
                    };
                }
                Token::Char(b'[') => {
                    // index: expr[key]
                    let span = expr.span();
                    self.advance()?;
                    let key = self.parse_expr()?;
                    self.expect_char(b']')?;
                    expr = Expr::Index {
                        table: Box::new(expr),
                        key: Box::new(key),
                        span,
                    };
                }
                Token::Char(b':') => {
                    // method call: expr:name(args)
                    let span = expr.span();
                    self.advance()?;
                    let (method, _) = self.expect_name()?;
                    let args = self.parse_func_args()?;
                    expr = Expr::MethodCall {
                        table: Box::new(expr),
                        method,
                        args,
                        span,
                    };
                }
                Token::Char(b'(') | Token::Str(_) | Token::Char(b'{') => {
                    // function call: expr(args) or expr"str" or expr{table}
                    let span = expr.span();
                    let args = self.parse_func_args()?;
                    expr = Expr::Call {
                        func: Box::new(expr),
                        args,
                        span,
                    };
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    /// Parse a primary expression: Name or '(' expr ')'.
    fn parse_primary_expr(&mut self) -> LuaResult<Expr> {
        let span = self.span;
        match &self.current {
            Token::Name(_) => {
                let (tok, _) = self.advance()?;
                if let Token::Name(name) = tok {
                    // Check for upvalue limit (PUC-Rio: singlevar -> indexupvalue).
                    self.check_name_upvalue(&name)?;
                    Ok(Expr::Name(name, span))
                } else {
                    Err(self.syntax_error("unexpected token"))
                }
            }
            Token::Char(b'(') => {
                let paren_span = self.span;
                self.advance()?;
                let expr = self.parse_expr()?;
                self.expect_char(b')')?;
                Ok(Expr::Paren(Box::new(expr), paren_span))
            }
            _ => Err(self.syntax_error_near("unexpected symbol")),
        }
    }

    /// Parse function arguments: '(' [exprlist] ')' | tableconstructor | string
    fn parse_func_args(&mut self) -> LuaResult<Vec<Expr>> {
        match &self.current {
            Token::Char(b'(') => {
                // PUC-Rio: if (line != ls->lastline)
                //   luaX_syntaxerror("ambiguous syntax (function call x new statement)")
                if self.span.line != self.lastline {
                    return Err(
                        self.syntax_error_near("ambiguous syntax (function call x new statement)")
                    );
                }
                let open_line = self.span.line;
                self.advance()?;
                let args = if self.check_char(b')') {
                    Vec::new()
                } else {
                    self.parse_expr_list()?
                };
                self.check_match(&Token::Char(b')'), &Token::Char(b'('), open_line)?;
                Ok(args)
            }
            Token::Char(b'{') => {
                let table = self.parse_table_ctor()?;
                Ok(vec![table])
            }
            Token::Str(_) => {
                let span = self.span;
                let (tok, _) = self.advance()?;
                if let Token::Str(s) = tok {
                    Ok(vec![Expr::Str(s, span)])
                } else {
                    Err(self.syntax_error("unexpected token"))
                }
            }
            _ => Err(self.syntax_error_near("function arguments expected")),
        }
    }

    /// Parse function body: '(' [parlist] ')' block 'end'
    fn parse_func_body(&mut self, def_span: Span) -> LuaResult<FuncBody> {
        let open_line = self.span.line;
        self.expect_char(b'(')?;

        let mut params = Vec::new();
        let mut has_varargs = false;

        if !self.check_char(b')') {
            loop {
                match &self.current {
                    Token::Name(_) => {
                        let (name, _) = self.expect_name()?;
                        params.push(name);
                    }
                    Token::Dots => {
                        self.advance()?;
                        has_varargs = true;
                        break;
                    }
                    _ => {
                        // PUC-Rio: "<name> or '...' expected" via luaX_syntaxerror
                        return Err(self.syntax_error_near("<name> or '...' expected"));
                    }
                }
                if !self.test_next_char(b',')? {
                    break;
                }
            }
        }

        self.expect_char(b')')?;

        // Push a new function scope for local variable tracking.
        // Parameters count as locals (PUC-Rio: new_localvar for each param).
        self.func_scopes.push(FuncScope {
            line_defined: def_span.line,
            local_count: 0,
            local_names: Vec::new(),
            upvalue_names: Vec::new(),
        });
        self.register_locals_named(&params)?;

        // Reset loop depth inside function bodies -- break can't cross
        // function boundaries (PUC-Rio creates a new FuncState).
        let saved_loop_depth = self.loop_depth;
        self.loop_depth = 0;
        let body = self.parse_block()?;
        self.loop_depth = saved_loop_depth;

        // Pop function scope.
        self.func_scopes.pop();

        // Capture the `end` keyword's line before consuming it.
        // PUC-Rio: f->lastlinedefined = ls->linenumber (at `end`)
        let end_line = self.span.line;
        self.check_match(&Token::End, &Token::Function, open_line)?;

        Ok(FuncBody {
            params,
            has_varargs,
            body,
            span: def_span,
            end_line,
        })
    }

    /// Parse table constructor: '{' [fieldlist] '}'
    fn parse_table_ctor(&mut self) -> LuaResult<Expr> {
        let span = self.span;
        let open_line = span.line;
        self.expect_char(b'{')?;

        let mut fields = Vec::new();

        while !self.check_char(b'}') {
            let field = self.parse_field()?;
            fields.push(field);

            // Field separator: ',' or ';'
            if !self.test_next_char(b',')? && !self.test_next_char(b';')? {
                break;
            }
        }

        self.check_match(&Token::Char(b'}'), &Token::Char(b'{'), open_line)?;

        Ok(Expr::TableCtor { fields, span })
    }

    /// Parse a single table field.
    fn parse_field(&mut self) -> LuaResult<TableField> {
        let span = self.span;
        match &self.current {
            // [expr] = expr
            Token::Char(b'[') => {
                self.advance()?;
                let key = self.parse_expr()?;
                self.expect_char(b']')?;
                self.expect_char(b'=')?;
                let value = self.parse_expr()?;
                Ok(TableField::IndexField { key, value, span })
            }
            // name = expr (need lookahead to distinguish from positional expr)
            Token::Name(_) if self.lexer.lookahead()? == &Token::Char(b'=') => {
                let (name, _) = self.expect_name()?;
                self.expect_char(b'=')?;
                let value = self.parse_expr()?;
                Ok(TableField::NameField { name, value, span })
            }
            // Positional value (covers Token::Name(_) without `=` lookahead too)
            _ => {
                let value = self.parse_expr()?;
                Ok(TableField::ValueField { value, span })
            }
        }
    }

    // -- Operator helpers --

    fn get_unary_op(&self) -> Option<UnOp> {
        match &self.current {
            Token::Not => Some(UnOp::Not),
            Token::Char(b'-') => Some(UnOp::Neg),
            Token::Char(b'#') => Some(UnOp::Len),
            _ => None,
        }
    }

    fn get_binary_op(&self) -> Option<BinOp> {
        match &self.current {
            Token::Char(b'+') => Some(BinOp::Add),
            Token::Char(b'-') => Some(BinOp::Sub),
            Token::Char(b'*') => Some(BinOp::Mul),
            Token::Char(b'/') => Some(BinOp::Div),
            Token::Char(b'%') => Some(BinOp::Mod),
            Token::Char(b'^') => Some(BinOp::Pow),
            Token::Concat => Some(BinOp::Concat),
            Token::Ne => Some(BinOp::Ne),
            Token::Eq => Some(BinOp::Eq),
            Token::Char(b'<') => Some(BinOp::Lt),
            Token::Le => Some(BinOp::Le),
            Token::Char(b'>') => Some(BinOp::Gt),
            Token::Ge => Some(BinOp::Ge),
            Token::And => Some(BinOp::And),
            Token::Or => Some(BinOp::Or),
            _ => None,
        }
    }
}

/// Unary operator priority (higher than all binary operators).
const UNARY_PRIORITY: u8 = 8;

/// Returns (left priority, right priority) for a binary operator.
///
/// Right-associative operators (`..` and `^`) have right < left.
/// PUC-Rio priority table from `lparser.c`.
fn binary_priority(op: BinOp) -> (u8, u8) {
    match op {
        BinOp::Or => (1, 1),
        BinOp::And => (2, 2),
        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Ne | BinOp::Eq => (3, 3),
        BinOp::Concat => (5, 4), // right-associative
        BinOp::Add | BinOp::Sub => (6, 6),
        BinOp::Mul | BinOp::Div | BinOp::Mod => (7, 7),
        BinOp::Pow => (10, 9), // right-associative
    }
}

/// Parses a Lua source string into an AST block.
pub fn parse(source: &[u8], name: &str) -> LuaResult<Block> {
    let mut parser = Parser::new(source, name)?;
    parser.parse_chunk()
}

/// Parses Lua source from a pre-built lexer into an AST block.
pub fn parse_with_lexer(lexer: Lexer<'_>) -> LuaResult<Block> {
    let mut parser = Parser::from_lexer(lexer)?;
    parser.parse_chunk()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::items_after_statements,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    // -- Helper --

    fn parse_ok(source: &str) -> Block {
        parse(source.as_bytes(), "test").unwrap()
    }

    fn parse_err(source: &str) -> String {
        parse(source.as_bytes(), "test").unwrap_err().to_string()
    }

    // -- Block and empty program --

    #[test]
    fn empty_program() {
        let block = parse_ok("");
        assert!(block.is_empty());
    }

    #[test]
    fn semicolons_only_is_error() {
        // Lua 5.1: semicolons are optional separators after statements,
        // NOT empty statements. ";;;" alone is a syntax error.
        let err = parse_err(";;;");
        assert!(err.contains("expected"), "error: {err}");
    }

    // -- Do block --

    #[test]
    fn do_end() {
        let block = parse_ok("do end");
        assert_eq!(block.len(), 1);
        assert!(matches!(block[0], Stat::Do { .. }));
    }

    #[test]
    fn nested_do() {
        let block = parse_ok("do do end end");
        assert_eq!(block.len(), 1);
        if let Stat::Do { body, .. } = &block[0] {
            assert_eq!(body.len(), 1);
            assert!(matches!(body[0], Stat::Do { .. }));
        } else {
            panic!("expected Do");
        }
    }

    // -- While --

    #[test]
    fn while_loop() {
        let block = parse_ok("while true do end");
        assert_eq!(block.len(), 1);
        if let Stat::While {
            condition, body, ..
        } = &block[0]
        {
            assert!(matches!(condition, Expr::True(_)));
            assert!(body.is_empty());
        } else {
            panic!("expected While");
        }
    }

    // -- Repeat --

    #[test]
    fn repeat_until() {
        let block = parse_ok("repeat until false");
        assert_eq!(block.len(), 1);
        if let Stat::Repeat {
            condition, body, ..
        } = &block[0]
        {
            assert!(matches!(condition, Expr::False(_)));
            assert!(body.is_empty());
        } else {
            panic!("expected Repeat");
        }
    }

    // -- If --

    #[test]
    fn if_then_end() {
        let block = parse_ok("if true then end");
        assert_eq!(block.len(), 1);
        if let Stat::If {
            conditions,
            bodies,
            else_body,
            ..
        } = &block[0]
        {
            assert_eq!(conditions.len(), 1);
            assert_eq!(bodies.len(), 1);
            assert!(else_body.is_none());
        } else {
            panic!("expected If");
        }
    }

    #[test]
    fn if_elseif_else() {
        let block = parse_ok("if true then elseif false then else end");
        if let Stat::If {
            conditions,
            bodies,
            else_body,
            ..
        } = &block[0]
        {
            assert_eq!(conditions.len(), 2);
            assert_eq!(bodies.len(), 2);
            assert!(else_body.is_some());
        } else {
            panic!("expected If");
        }
    }

    // -- Numeric for --

    #[test]
    fn numeric_for() {
        let block = parse_ok("for i = 1, 10 do end");
        if let Stat::NumericFor {
            name, step, body, ..
        } = &block[0]
        {
            assert_eq!(name, "i");
            assert!(step.is_none());
            assert!(body.is_empty());
        } else {
            panic!("expected NumericFor");
        }
    }

    #[test]
    fn numeric_for_with_step() {
        let block = parse_ok("for i = 1, 10, 2 do end");
        if let Stat::NumericFor { step, .. } = &block[0] {
            assert!(step.is_some());
        } else {
            panic!("expected NumericFor");
        }
    }

    // -- Generic for --

    #[test]
    fn generic_for() {
        let block = parse_ok("for k, v in pairs(t) do end");
        if let Stat::GenericFor { names, .. } = &block[0] {
            assert_eq!(names, &["k", "v"]);
        } else {
            panic!("expected GenericFor");
        }
    }

    // -- Local --

    #[test]
    fn local_decl() {
        let block = parse_ok("local x");
        if let Stat::LocalDecl { names, values, .. } = &block[0] {
            assert_eq!(names, &["x"]);
            assert!(values.is_empty());
        } else {
            panic!("expected LocalDecl");
        }
    }

    #[test]
    fn local_decl_with_init() {
        let block = parse_ok("local x, y = 1, 2");
        if let Stat::LocalDecl { names, values, .. } = &block[0] {
            assert_eq!(names, &["x", "y"]);
            assert_eq!(values.len(), 2);
        } else {
            panic!("expected LocalDecl");
        }
    }

    #[test]
    fn local_function() {
        let block = parse_ok("local function foo() end");
        if let Stat::LocalFunc { name, .. } = &block[0] {
            assert_eq!(name, "foo");
        } else {
            panic!("expected LocalFunc");
        }
    }

    // -- Function declaration --

    #[test]
    fn func_decl_simple() {
        let block = parse_ok("function foo() end");
        if let Stat::FuncDecl { name, .. } = &block[0] {
            assert_eq!(name.parts, vec!["foo"]);
            assert!(name.method.is_none());
        } else {
            panic!("expected FuncDecl");
        }
    }

    #[test]
    fn func_decl_dotted() {
        let block = parse_ok("function a.b.c() end");
        if let Stat::FuncDecl { name, .. } = &block[0] {
            assert_eq!(name.parts, vec!["a", "b", "c"]);
            assert!(name.method.is_none());
        } else {
            panic!("expected FuncDecl");
        }
    }

    #[test]
    fn func_decl_method() {
        let block = parse_ok("function a.b:c() end");
        if let Stat::FuncDecl { name, .. } = &block[0] {
            assert_eq!(name.parts, vec!["a", "b"]);
            assert_eq!(name.method.as_deref(), Some("c"));
        } else {
            panic!("expected FuncDecl");
        }
    }

    // -- Return --

    #[test]
    fn return_no_values() {
        let block = parse_ok("return");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(values.is_empty());
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn return_values() {
        let block = parse_ok("return 1, 2, 3");
        if let Stat::Return { values, .. } = &block[0] {
            assert_eq!(values.len(), 3);
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn return_must_be_last() {
        // Return followed by more statements is invalid
        let block = parse_ok("return 1");
        assert_eq!(block.len(), 1);
    }

    // -- Break --

    #[test]
    fn break_statement() {
        // Break must appear inside a loop; test inside while.
        let block = parse_ok("while true do break end");
        assert_eq!(block.len(), 1);
        if let Stat::While { body, .. } = &block[0] {
            assert_eq!(body.len(), 1);
            assert!(matches!(body[0], Stat::Break { .. }));
        } else {
            panic!("expected while statement");
        }
    }

    #[test]
    fn break_outside_loop() {
        // PUC-Rio: "no loop to break near '<eof>'"
        let err = parse_err("break");
        assert!(err.contains("no loop to break"), "got: {err}");
    }

    // -- Assignment --

    #[test]
    fn simple_assignment() {
        let block = parse_ok("x = 1");
        assert!(matches!(block[0], Stat::Assign { .. }));
    }

    #[test]
    fn multi_assignment() {
        let block = parse_ok("x, y = 1, 2");
        if let Stat::Assign {
            targets, values, ..
        } = &block[0]
        {
            assert_eq!(targets.len(), 2);
            assert_eq!(values.len(), 2);
        } else {
            panic!("expected Assign");
        }
    }

    // -- Expression statements (function calls) --

    #[test]
    fn call_statement() {
        let block = parse_ok("print(1)");
        assert!(matches!(block[0], Stat::ExprStat { .. }));
    }

    #[test]
    fn method_call_statement() {
        let block = parse_ok("obj:method()");
        if let Stat::ExprStat { expr, .. } = &block[0] {
            assert!(matches!(expr, Expr::MethodCall { .. }));
        } else {
            panic!("expected ExprStat");
        }
    }

    // -- Expression parsing --

    #[test]
    fn binary_precedence() {
        // 1 + 2 * 3 should parse as 1 + (2 * 3)
        let block = parse_ok("return 1 + 2 * 3");
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::BinOp {
                op: BinOp::Add,
                right,
                ..
            } = &values[0]
            {
                assert!(matches!(right.as_ref(), Expr::BinOp { op: BinOp::Mul, .. }));
            } else {
                panic!("expected Add at top");
            }
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn right_associative_pow() {
        // 2^3^4 should parse as 2^(3^4)
        let block = parse_ok("return 2^3^4");
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::BinOp {
                op: BinOp::Pow,
                right,
                ..
            } = &values[0]
            {
                assert!(matches!(right.as_ref(), Expr::BinOp { op: BinOp::Pow, .. }));
            } else {
                panic!("expected Pow at top");
            }
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn right_associative_concat() {
        // "a".."b".."c" should parse as "a"..("b".."c")
        let block = parse_ok(r#"return "a".."b".."c""#);
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::BinOp {
                op: BinOp::Concat,
                right,
                ..
            } = &values[0]
            {
                assert!(matches!(
                    right.as_ref(),
                    Expr::BinOp {
                        op: BinOp::Concat,
                        ..
                    }
                ));
            } else {
                panic!("expected Concat at top");
            }
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn unary_operators() {
        let block = parse_ok("return -1, not true, #t");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::UnOp { op: UnOp::Neg, .. }));
            assert!(matches!(values[1], Expr::UnOp { op: UnOp::Not, .. }));
            assert!(matches!(values[2], Expr::UnOp { op: UnOp::Len, .. }));
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn all_binary_ops() {
        let block = parse_ok(
            "return 1+2, 1-2, 1*2, 1/2, 1%2, 1^2, \"a\"..\"b\", 1<2, 1<=2, 1>2, 1>=2, 1==2, 1~=2, true and false, true or false",
        );
        if let Stat::Return { values, .. } = &block[0] {
            assert_eq!(values.len(), 15);
        } else {
            panic!("expected Return");
        }
    }

    // -- Suffixed expressions --

    #[test]
    fn field_access() {
        let block = parse_ok("return a.b.c");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::Field { .. }));
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn index_access() {
        let block = parse_ok("return a[1]");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::Index { .. }));
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn function_call_expr() {
        let block = parse_ok("return f(1, 2)");
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::Call { args, .. } = &values[0] {
                assert_eq!(args.len(), 2);
            } else {
                panic!("expected Call");
            }
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn call_with_string_arg() {
        let block = parse_ok(r#"print "hello""#);
        if let Stat::ExprStat { expr, .. } = &block[0] {
            if let Expr::Call { args, .. } = expr {
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::Str(s, _) if s == b"hello"));
            } else {
                panic!("expected Call");
            }
        } else {
            panic!("expected ExprStat");
        }
    }

    #[test]
    fn call_with_table_arg() {
        let block = parse_ok("f{1, 2}");
        if let Stat::ExprStat { expr, .. } = &block[0] {
            if let Expr::Call { args, .. } = expr {
                assert_eq!(args.len(), 1);
                assert!(matches!(&args[0], Expr::TableCtor { .. }));
            } else {
                panic!("expected Call");
            }
        } else {
            panic!("expected ExprStat");
        }
    }

    // -- Table constructors --

    #[test]
    fn empty_table() {
        let block = parse_ok("return {}");
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::TableCtor { fields, .. } = &values[0] {
                assert!(fields.is_empty());
            } else {
                panic!("expected TableCtor");
            }
        } else {
            panic!("expected Return");
        }
    }

    #[test]
    fn table_with_fields() {
        let block = parse_ok("return {x = 1, [2] = 3, 4}");
        if let Stat::Return { values, .. } = &block[0] {
            if let Expr::TableCtor { fields, .. } = &values[0] {
                assert_eq!(fields.len(), 3);
                assert!(matches!(fields[0], TableField::NameField { .. }));
                assert!(matches!(fields[1], TableField::IndexField { .. }));
                assert!(matches!(fields[2], TableField::ValueField { .. }));
            } else {
                panic!("expected TableCtor");
            }
        } else {
            panic!("expected Return");
        }
    }

    // -- Function body --

    #[test]
    fn func_with_params() {
        let block = parse_ok("function foo(a, b, c) end");
        if let Stat::FuncDecl { body, .. } = &block[0] {
            assert_eq!(body.params, vec!["a", "b", "c"]);
            assert!(!body.has_varargs);
        } else {
            panic!("expected FuncDecl");
        }
    }

    #[test]
    fn func_with_varargs() {
        let block = parse_ok("function foo(a, ...) end");
        if let Stat::FuncDecl { body, .. } = &block[0] {
            assert_eq!(body.params, vec!["a"]);
            assert!(body.has_varargs);
        } else {
            panic!("expected FuncDecl");
        }
    }

    #[test]
    fn anonymous_func() {
        let block = parse_ok("return function(x) return x end");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::FuncDef { .. }));
        } else {
            panic!("expected Return");
        }
    }

    // -- Parenthesized expressions --

    #[test]
    fn paren_expr() {
        let block = parse_ok("return (1 + 2) * 3");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::BinOp { op: BinOp::Mul, .. }));
        } else {
            panic!("expected Return");
        }
    }

    // -- Vararg --

    #[test]
    fn vararg_expr() {
        let block = parse_ok("return ...");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::VarArg(_)));
        } else {
            panic!("expected Return");
        }
    }

    // -- Literals --

    #[test]
    fn literal_exprs() {
        let block = parse_ok("return nil, true, false, 42, \"hello\"");
        if let Stat::Return { values, .. } = &block[0] {
            assert!(matches!(values[0], Expr::Nil(_)));
            assert!(matches!(values[1], Expr::True(_)));
            assert!(matches!(values[2], Expr::False(_)));
            assert!(matches!(values[3], Expr::Number(n, _) if n == 42.0));
            assert!(matches!(&values[4], Expr::Str(s, _) if s == b"hello"));
        } else {
            panic!("expected Return");
        }
    }

    // -- Error cases --

    #[test]
    fn missing_end() {
        let err = parse_err("if true then");
        assert!(err.contains("'end' expected"));
    }

    #[test]
    fn missing_end_multiline() {
        let err = parse_err("if true then\n  x = 1\n");
        assert!(err.contains("'end' expected"));
        assert!(err.contains("to close"));
    }

    #[test]
    fn missing_then() {
        let err = parse_err("if true end");
        assert!(err.contains("'then' expected"));
    }

    #[test]
    fn for_missing_in_or_eq() {
        let err = parse_err("for x do end");
        assert!(err.contains("'=' or 'in' expected"));
    }

    #[test]
    fn unexpected_symbol() {
        let err = parse_err("+");
        assert!(err.contains("unexpected symbol"));
    }

    #[test]
    fn missing_closing_paren() {
        let err = parse_err("return (1 + 2");
        assert!(err.contains("')' expected"));
    }
}
