//! Code generator: compiles an AST into a Proto (bytecode).
//!
//! Follows PUC-Rio's `lcode.c` and `lparser.c` compiler structure.
//! Key data structures: `FuncState` tracks per-function compilation state,
//! `ExprContext` (maps to PUC-Rio's `expdesc`) tracks expression state
//! during code generation, and `BlockContext` tracks lexical scopes.

use std::collections::HashMap;

use crate::error::{LuaError, LuaResult, SyntaxError};

use super::ast::Block;
use super::lexer::Lexer;
use super::parser;

use crate::vm::instructions::{
    BITRK, Instruction, LFIELDS_PER_FLUSH, LUAI_MAXUPVALUES, LUAI_MAXVARS, MAXARG_BX, MAXARG_C,
    MAXINDEXRK, MAXSTACK, NO_JUMP, NO_REG, OpCode, is_k,
};
use crate::vm::proto::{
    LocalVar, Proto, ProtoRef, VARARG_HASARG, VARARG_ISVARARG, VARARG_NEEDSARG,
};
use crate::vm::value::Val;

// ---------------------------------------------------------------------------
// ExprContext (maps to PUC-Rio's expdesc)
// ---------------------------------------------------------------------------

/// Expression kind: what state is this expression in?
///
/// Maps directly to PUC-Rio's `expkind` enum. The expression kind determines
/// how `info` and `aux` are interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Variants used in chunk 2i (full code generation)
pub(crate) enum ExprKind {
    /// No value (void expression).
    Void,
    /// Nil constant.
    Nil,
    /// True constant.
    True,
    /// False constant.
    False,
    /// Constant in pool: `info` = index into `Proto.constants`.
    K,
    /// Numeric constant: stored in `nval`.
    KNum,
    /// Local variable: `info` = register index.
    Local,
    /// Upvalue: `info` = upvalue index.
    Upval,
    /// Global variable: `info` = constant index for name string.
    Global,
    /// Indexed: `info` = table register, `aux` = index RK.
    Indexed,
    /// Jump instruction: `info` = pc of the jump.
    Jmp,
    /// Relocable instruction: `info` = pc (result register can be patched).
    Relocable,
    /// Non-relocable: `info` = fixed result register.
    NonReloc,
    /// Function call: `info` = pc of the CALL instruction.
    Call,
    /// Vararg expression: `info` = pc of the VARARG instruction.
    VarArg,
}

/// Expression context: tracks the state of an expression during compilation.
///
/// Maps to PUC-Rio's `expdesc`. The `t` and `f` fields are linked lists of
/// pending jump instructions that should be patched when the expression's
/// boolean value is determined.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ExprContext {
    /// What kind of expression this is.
    pub kind: ExprKind,
    /// Primary information (interpretation depends on `kind`).
    pub info: i32,
    /// Auxiliary information (used by Indexed and Global).
    pub aux: i32,
    /// Numeric value (used when kind == KNum).
    pub nval: f64,
    /// Patch list: jumps when expression is true.
    pub t: i32,
    /// Patch list: jumps when expression is false.
    pub f: i32,
}

impl ExprContext {
    fn void() -> Self {
        Self {
            kind: ExprKind::Void,
            info: 0,
            aux: 0,
            nval: 0.0,
            t: NO_JUMP,
            f: NO_JUMP,
        }
    }

    fn new(kind: ExprKind, info: i32) -> Self {
        Self {
            kind,
            info,
            aux: 0,
            nval: 0.0,
            t: NO_JUMP,
            f: NO_JUMP,
        }
    }

    fn number(val: f64) -> Self {
        Self {
            kind: ExprKind::KNum,
            info: 0,
            aux: 0,
            nval: val,
            t: NO_JUMP,
            f: NO_JUMP,
        }
    }

    /// Returns true if the expression has pending jump lists.
    fn has_jumps(&self) -> bool {
        self.t != self.f
    }

    /// Returns true if the expression is a pure numeric constant with no pending
    /// jumps. Matches PUC-Rio's `isnumeral` macro in `lcode.c`.
    fn is_numeral(&self) -> bool {
        self.kind == ExprKind::KNum && self.t == NO_JUMP && self.f == NO_JUMP
    }
}

// ---------------------------------------------------------------------------
// UpvalDesc
// ---------------------------------------------------------------------------

/// Describes how an upvalue is captured from a parent scope.
#[derive(Debug, Clone)]
pub(crate) struct UpvalDesc {
    /// True if captured from parent's locals (VLOCAL), false if from
    /// parent's upvalues (VUPVAL).
    pub in_stack: bool,
    /// Index of the local variable or upvalue in the parent function.
    pub index: u8,
    /// Name of the upvalue (for debug info).
    pub name: String,
}

// ---------------------------------------------------------------------------
// BlockContext
// ---------------------------------------------------------------------------

/// Tracks a lexical block scope during compilation.
///
/// Maps to PUC-Rio's `BlockCnt`.
#[allow(dead_code)] // Fields used in chunk 2i (full code generation)
struct BlockContext {
    /// Number of active locals when this block was entered.
    num_active_vars: u8,
    /// True if any local in this block is captured as an upvalue.
    has_upval: bool,
    /// True if this block is a breakable loop.
    is_breakable: bool,
    /// Linked list of pending break jumps.
    break_list: i32,
}

// ---------------------------------------------------------------------------
// FuncState
// ---------------------------------------------------------------------------

/// Key for constant pool deduplication hash map.
/// Uses bitwise equality for numbers (NaN != NaN, +0.0 != -0.0).
#[derive(Hash, Eq, PartialEq)]
enum ConstantKey {
    /// Number constant keyed by f64 bit pattern.
    Num(u64),
    /// Boolean constant.
    Bool(bool),
    /// String constant keyed by raw bytes.
    Str(Vec<u8>),
}

/// Per-function compilation state.
///
/// Maps to PUC-Rio's `FuncState`. One `FuncState` exists per function
/// being compiled; they form a stack via the `Compiler` struct.
#[allow(dead_code)] // Fields used in chunk 2i (full code generation)
pub(crate) struct FuncState {
    /// The prototype being built.
    pub proto: Proto,
    /// Next free register.
    pub free_reg: u8,
    /// Number of active local variables.
    pub num_active_vars: u8,
    /// Upvalue descriptors for this function.
    pub upvalues: Vec<UpvalDesc>,
    /// Active variable stack: indices into proto.local_vars.
    active_vars: Vec<u16>,
    /// Block scope chain.
    blocks: Vec<BlockContext>,
    /// Pending jumps to current pc.
    pub jpc: i32,
    /// PC of last jump target (avoid bad optimizations).
    pub last_target: i32,
    /// Cached nil constant index (PUC-Rio's `nilK`). We track this
    /// separately because `Val::Nil` is also used as a placeholder for
    /// unresolved string constants in the constant pool.
    nil_k: Option<u32>,
    /// O(1) constant pool dedup index. Maps constant values to their
    /// index in `proto.constants`. Mirrors PUC-Rio's `fs->h` hash table
    /// (`addk` in `lcode.c:224`).
    constant_index: HashMap<ConstantKey, u32>,
}

impl FuncState {
    fn new(source: &str) -> Self {
        Self {
            proto: Proto::new(source),
            free_reg: 0,
            num_active_vars: 0,
            upvalues: Vec::new(),
            active_vars: Vec::new(),
            blocks: Vec::new(),
            jpc: NO_JUMP,
            last_target: -1,
            nil_k: None,
            constant_index: HashMap::new(),
        }
    }

    /// Returns the current pc (next instruction index).
    pub(crate) fn pc(&self) -> usize {
        self.proto.code.len()
    }
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

/// Top-level compiler state: manages the function state stack.
pub struct Compiler {
    /// Stack of function states (innermost is last).
    func_states: Vec<FuncState>,
    /// Source name for error messages.
    source_name: String,
    /// Current line number (for instruction debug info).
    pub(crate) current_line: u32,
}

#[allow(dead_code)] // Methods used in chunk 2i (full code generation)
impl Compiler {
    /// Creates a new compiler for the given source.
    fn new(source_name: &str) -> Self {
        let fs = FuncState::new(source_name);
        Self {
            func_states: vec![fs],
            source_name: source_name.to_string(),
            current_line: 1,
        }
    }

    /// Returns the current (innermost) function state.
    ///
    /// # Panics
    /// Panics if no function state exists (invariant: always >= 1).
    #[allow(clippy::expect_used)]
    pub(crate) fn fs(&self) -> &FuncState {
        self.func_states
            .last()
            .expect("compiler must have at least one function state")
    }

    /// Returns a mutable reference to the current function state.
    ///
    /// # Panics
    /// Panics if no function state exists (invariant: always >= 1).
    #[allow(clippy::expect_used)]
    pub(crate) fn fs_mut(&mut self) -> &mut FuncState {
        self.func_states
            .last_mut()
            .expect("compiler must have at least one function state")
    }

    fn syntax_error(&self, msg: &str) -> LuaError {
        LuaError::Syntax(SyntaxError {
            message: msg.to_string(),
            source: self.source_name.clone(),
            line: self.current_line,
            raw_message: None,
        })
    }

    // -- Instruction emission --

    /// Emits an instruction and records line info. Returns the pc.
    pub(crate) fn emit(&mut self, instr: Instruction, line: u32) -> usize {
        self.discharge_jpc();
        let fs = self.fs_mut();
        let pc = fs.proto.code.len();
        fs.proto.code.push(instr.raw());
        fs.proto.line_info.push(line);
        pc
    }

    /// Emits an iABC instruction. Returns the pc.
    pub(crate) fn emit_abc(&mut self, op: OpCode, a: u32, b: u32, c: u32, line: u32) -> usize {
        self.emit(Instruction::abc(op, a, b, c), line)
    }

    /// Emits LOADNIL for registers `from..from+n-1` with coalescing.
    ///
    /// Matches PUC-Rio's `luaK_nil` (lcode.c:35-51):
    /// - At function start (pc==0), locals are already nil, so skip.
    /// - If the previous instruction is LOADNIL and covers adjacent registers,
    ///   extend its B operand instead of emitting a new instruction.
    /// - Only coalesces when no jump target has been set at the current pc.
    pub(crate) fn emit_nil(&mut self, from: u32, n: u32, line: u32) {
        let fs = self.fs();
        let pc = fs.pc();
        let last_target = fs.last_target;
        if (pc as i32) > last_target {
            // No jump targets at current position — safe to optimize.
            if pc == 0 {
                // Function start: all registers are already nil.
                return;
            }
            let prev = self.get_instruction(pc - 1);
            if prev.opcode() == OpCode::LoadNil {
                let pfrom = prev.a();
                let pto = prev.b();
                if pfrom <= from && from <= pto + 1 {
                    // Ranges overlap or are adjacent — extend.
                    if from + n - 1 > pto {
                        let mut updated = prev;
                        updated.set_b(from + n - 1);
                        self.set_instruction(pc - 1, updated);
                    }
                    return;
                }
            }
        }
        self.emit_abc(OpCode::LoadNil, from, from + n - 1, 0, line);
    }

    /// Emits an iABx instruction. Returns the pc.
    pub(crate) fn emit_abx(&mut self, op: OpCode, a: u32, bx: u32, line: u32) -> usize {
        self.emit(Instruction::a_bx(op, a, bx), line)
    }

    /// Emits an iAsBx instruction. Returns the pc.
    #[allow(dead_code)]
    pub(crate) fn emit_asbx(&mut self, op: OpCode, a: u32, sbx: i32, line: u32) -> usize {
        self.emit(Instruction::a_sbx(op, a, sbx), line)
    }

    /// Returns the instruction at a given pc.
    pub(crate) fn get_instruction(&self, pc: usize) -> Instruction {
        Instruction::from_raw(self.fs().proto.code[pc])
    }

    /// Modifies the instruction at a given pc.
    pub(crate) fn set_instruction(&mut self, pc: usize, instr: Instruction) {
        self.fs_mut().proto.code[pc] = instr.raw();
    }

    // -- Jump management --

    /// Emits a JMP instruction. Returns the pc of the jump.
    pub(crate) fn emit_jump(&mut self, line: u32) -> usize {
        let jpc = self.fs().jpc;
        let fs = self.fs_mut();
        fs.jpc = NO_JUMP;
        let pc = self.emit_asbx(OpCode::Jmp, 0, NO_JUMP, line);
        self.concat_jumps_result(pc, jpc)
    }

    /// Patches jump at `pc` to target `target`.
    pub(crate) fn patch_jump(&mut self, pc: usize, target: usize) {
        let offset = target as i32 - pc as i32 - 1;
        let mut instr = self.get_instruction(pc);
        instr.set_sbx(offset);
        self.set_instruction(pc, instr);
    }

    /// Returns the target of a jump instruction at `pc`.
    fn get_jump_target(&self, pc: usize) -> i32 {
        let offset = self.get_instruction(pc).sbx();
        if offset == NO_JUMP {
            return NO_JUMP;
        }
        (pc as i32) + 1 + offset
    }

    /// Concatenates jump list `l2` onto `l1`. Returns the merged list head.
    fn concat_jumps_result(&mut self, l1: usize, l2: i32) -> usize {
        if l2 == NO_JUMP {
            return l1;
        }
        let l1_i32 = l1 as i32;
        if l1_i32 == NO_JUMP {
            return l2 as usize;
        }
        // Walk l1 to its end, then link l2
        let mut list = l1_i32;
        loop {
            let next = self.get_jump_target(list as usize);
            if next == NO_JUMP {
                self.patch_jump(list as usize, l2 as usize);
                break;
            }
            list = next;
        }
        l1
    }

    /// Concatenates jump list `l2` onto `l1` (both as i32).
    pub(crate) fn concat_jumps(&mut self, l1: &mut i32, l2: i32) {
        if l2 == NO_JUMP {
            return;
        }
        if *l1 == NO_JUMP {
            *l1 = l2;
        } else {
            let mut list = *l1;
            loop {
                let next = self.get_jump_target(list as usize);
                if next == NO_JUMP {
                    self.patch_jump(list as usize, l2 as usize);
                    break;
                }
                list = next;
            }
        }
    }

    /// Patches all jumps in `list` to target `target`.
    /// If target is the current pc, delegates to `patch_to_here` (lazy patching).
    /// Otherwise uses `patch_list_aux` with NO_REG.
    /// Maps to PUC-Rio's `luaK_patchlist`.
    pub(crate) fn patch_list(&mut self, list: i32, target: usize) {
        if target == self.fs().pc() {
            self.patch_to_here(list);
        } else {
            self.patch_list_aux(list, target, NO_REG, target);
        }
    }

    /// Patches all jumps in `list` to target the current pc.
    pub(crate) fn patch_to_here(&mut self, list: i32) {
        let jpc = self.fs().jpc;
        let mut merged = jpc;
        self.concat_jumps(&mut merged, list);
        self.fs_mut().jpc = merged;
    }

    /// Discharges pending jumps to the current pc.
    fn discharge_jpc(&mut self) {
        let jpc = self.fs().jpc;
        if jpc != NO_JUMP {
            let pc = self.fs().pc();
            self.patch_list_aux(jpc, pc, NO_REG, pc);
            self.fs_mut().jpc = NO_JUMP;
        }
    }

    /// Checks whether any jump in `list` requires a value (i.e., the control
    /// instruction is NOT a TESTSET). Maps to PUC-Rio's `need_value`.
    fn need_value(&self, mut list: i32) -> bool {
        while list != NO_JUMP {
            let ctrl = self.get_jump_control(list as usize);
            let instr = self.get_instruction(ctrl);
            if instr.opcode() != OpCode::TestSet {
                return true;
            }
            list = self.get_jump_target(list as usize);
        }
        false
    }

    /// If the control instruction at `node` is TESTSET, patches it:
    /// - If `reg != NO_REG` and `reg != B`, sets A = reg (value destination)
    /// - Otherwise, converts TESTSET to TEST (discards the value)
    ///
    /// Returns true if the instruction was TESTSET.
    /// Maps to PUC-Rio's `patchtestreg`.
    fn patch_test_reg(&mut self, node: usize, reg: u32) -> bool {
        let ctrl = self.get_jump_control(node);
        let instr = self.get_instruction(ctrl);
        if instr.opcode() != OpCode::TestSet {
            return false;
        }
        if reg != NO_REG && reg != instr.b() {
            let mut patched = instr;
            patched.set_a(reg);
            self.set_instruction(ctrl, patched);
        } else {
            // Convert TESTSET to TEST: TEST B _ C
            let replacement = Instruction::abc(OpCode::Test, instr.b(), 0, instr.c());
            self.set_instruction(ctrl, replacement);
        }
        true
    }

    /// Converts all TESTSET instructions in a jump list to TEST.
    /// Maps to PUC-Rio's `removevalues`: walks the list and calls
    /// `patchtestreg(node, NO_REG)` which replaces TESTSET with TEST.
    /// Called after negating an expression (`code_not`) to ensure the
    /// swapped jump lists don't carry stale value-assignment semantics.
    fn remove_values(&mut self, mut list: i32) {
        while list != NO_JUMP {
            self.patch_test_reg(list as usize, NO_REG);
            list = self.get_jump_target(list as usize);
        }
    }

    /// Patches a jump list with separate targets for TESTSET jumps (`vtarget`)
    /// and other jumps (`dtarget`). Maps to PUC-Rio's `patchlistaux`.
    fn patch_list_aux(&mut self, mut list: i32, vtarget: usize, reg: u32, dtarget: usize) {
        while list != NO_JUMP {
            let next = self.get_jump_target(list as usize);
            if self.patch_test_reg(list as usize, reg) {
                self.patch_jump(list as usize, vtarget);
            } else {
                self.patch_jump(list as usize, dtarget);
            }
            list = next;
        }
    }

    /// Emits `LOADBOOL reg, b, jump` after calling `get_label`.
    /// Maps to PUC-Rio's `code_label`.
    fn code_label(&mut self, reg: u32, b: u32, jump: u32, line: u32) -> usize {
        self.get_label();
        self.emit_abc(OpCode::LoadBool, reg, b, jump, line)
    }

    // -- Constant pool --

    /// Adds a value to the constant pool, deduplicating via hash map.
    /// Returns the constant index.
    ///
    /// Uses `constant_index` for O(1) lookup, mirroring PUC-Rio's `addk`
    /// which uses `luaH_set` on `fs->h` (`lcode.c:224`).
    pub(crate) fn add_constant(&mut self, val: Val) -> LuaResult<u32> {
        let key = match val {
            Val::Num(n) => ConstantKey::Num(n.to_bits()),
            Val::Bool(b) => ConstantKey::Bool(b),
            // Nil placeholders are never deduped here; nil_constant()
            // handles its own caching.
            _ => {
                let fs = self.fs_mut();
                let idx = fs.proto.constants.len();
                if idx > MAXARG_BX as usize {
                    return Err(self.syntax_error("constant table overflow"));
                }
                fs.proto.constants.push(val);
                #[allow(clippy::cast_possible_truncation)]
                return Ok(idx as u32);
            }
        };
        let fs = self.fs_mut();
        if let Some(&idx) = fs.constant_index.get(&key) {
            return Ok(idx);
        }
        let idx = fs.proto.constants.len();
        if idx > MAXARG_BX as usize {
            return Err(self.syntax_error("constant table overflow"));
        }
        fs.proto.constants.push(val);
        #[allow(clippy::cast_possible_truncation)]
        let idx = idx as u32;
        fs.constant_index.insert(key, idx);
        Ok(idx)
    }

    /// Adds a string constant to the pool. Returns the constant index.
    ///
    /// Deduplicates via hash map lookup on raw byte content.
    /// Stores `Val::Nil` as a placeholder in the constant pool; the real
    /// `Val::Str` is patched in by `patch_string_constants` before execution.
    pub(crate) fn string_constant(&mut self, s: &[u8]) -> LuaResult<u32> {
        let key = ConstantKey::Str(s.to_vec());
        let fs = self.fs_mut();
        if let Some(&idx) = fs.constant_index.get(&key) {
            return Ok(idx);
        }
        let idx = fs.proto.constants.len();
        if idx > MAXARG_BX as usize {
            return Err(self.syntax_error("constant table overflow"));
        }
        fs.proto.constants.push(Val::Nil);
        #[allow(clippy::cast_possible_truncation)]
        let idx = idx as u32;
        fs.proto.string_pool.push((idx, s.to_vec()));
        fs.constant_index.insert(key, idx);
        Ok(idx)
    }

    /// Adds a number constant to the pool. Returns the constant index.
    pub(crate) fn number_constant(&mut self, n: f64) -> LuaResult<u32> {
        self.add_constant(Val::Num(n))
    }

    /// Returns the constant index for nil, creating one if needed.
    /// Matches PUC-Rio's `nilK` (lcode.c:266-272).
    ///
    /// Uses a cache to avoid creating duplicate nil constants, since
    /// `add_constant` cannot deduplicate nil (Val::Nil is used as a
    /// placeholder for unresolved string constants).
    pub(crate) fn nil_constant(&mut self) -> LuaResult<u32> {
        if let Some(idx) = self.fs().nil_k {
            return Ok(idx);
        }
        let fs = self.fs_mut();
        let idx = fs.proto.constants.len();
        if idx > MAXARG_BX as usize {
            return Err(self.syntax_error("constant table overflow"));
        }
        fs.proto.constants.push(Val::Nil);
        #[allow(clippy::cast_possible_truncation)]
        let idx = idx as u32;
        self.fs_mut().nil_k = Some(idx);
        Ok(idx)
    }

    // -- Register allocation --

    /// Allocates one register. Returns its index.
    pub(crate) fn alloc_reg(&mut self) -> LuaResult<u32> {
        self.check_stack(1)?;
        let reg = u32::from(self.fs().free_reg);
        self.fs_mut().free_reg += 1;
        Ok(reg)
    }

    /// Reserves `n` consecutive registers.
    pub(crate) fn reserve_regs(&mut self, n: u32) -> LuaResult<()> {
        self.check_stack(n)?;
        self.fs_mut().free_reg += n as u8;
        Ok(())
    }

    /// Frees a register if it's a temporary (not a local variable).
    ///
    /// Matches PUC-Rio `freereg` in `lcode.c`: only decrements `freereg`
    /// when `reg` is the top temporary register. If `reg` is below `freereg`
    /// it is silently ignored (the register is already effectively free).
    pub(crate) fn free_reg(&mut self, reg: u32) {
        let fs = self.fs();
        if reg >= u32::from(fs.num_active_vars) && !is_k(reg) {
            let fs = self.fs_mut();
            if u32::from(fs.free_reg) > 0 && reg == u32::from(fs.free_reg) - 1 {
                fs.free_reg -= 1;
            }
        }
    }

    /// Ensures the stack has room for `n` more registers.
    fn check_stack(&mut self, n: u32) -> LuaResult<()> {
        let new_stack = u32::from(self.fs().free_reg) + n;
        if new_stack > MAXSTACK {
            return Err(self.syntax_error("function or expression too complex"));
        }
        let fs = self.fs_mut();
        if new_stack > u32::from(fs.proto.max_stack_size) {
            fs.proto.max_stack_size = new_stack as u8;
        }
        Ok(())
    }

    // -- Variable resolution --

    /// Searches for a local variable by name in the current function.
    /// Returns the register index if found.
    fn search_local(&self, name: &str) -> Option<u8> {
        let fs = self.fs();
        for i in (0..fs.num_active_vars).rev() {
            let var_idx = fs.active_vars[i as usize];
            if fs.proto.local_vars[var_idx as usize].name == name {
                return Some(i);
            }
        }
        None
    }

    /// Searches for an upvalue by name in the current function.
    /// Returns the upvalue index if found.
    fn search_upvalue(&self, name: &str) -> Option<u8> {
        let fs = self.fs();
        for (i, uv) in fs.upvalues.iter().enumerate() {
            if uv.name == name {
                #[allow(clippy::cast_possible_truncation)]
                return Some(i as u8);
            }
        }
        None
    }

    /// Registers a new upvalue. Returns the upvalue index.
    fn add_upvalue(
        &mut self,
        fs_idx: usize,
        name: &str,
        in_stack: bool,
        index: u8,
    ) -> LuaResult<u8> {
        let fs = &mut self.func_states[fs_idx];
        // Check for duplicate
        for (i, uv) in fs.upvalues.iter().enumerate() {
            if uv.in_stack == in_stack && uv.index == index {
                #[allow(clippy::cast_possible_truncation)]
                return Ok(i as u8);
            }
        }
        let idx = fs.upvalues.len();
        if idx >= LUAI_MAXUPVALUES as usize {
            return Err(self.syntax_error("too many upvalues"));
        }
        fs.upvalues.push(UpvalDesc {
            in_stack,
            index,
            name: name.to_string(),
        });
        fs.proto.num_upvalues = fs.upvalues.len() as u8;
        #[allow(clippy::cast_possible_truncation)]
        Ok(idx as u8)
    }

    /// Resolves a variable name: local, upvalue, or global.
    pub(crate) fn resolve_var(&mut self, name: &str) -> LuaResult<ExprContext> {
        // 1. Search locals in current function
        if let Some(reg) = self.search_local(name) {
            return Ok(ExprContext::new(ExprKind::Local, i32::from(reg)));
        }

        // 2. Search upvalues in current function
        if let Some(idx) = self.search_upvalue(name) {
            return Ok(ExprContext::new(ExprKind::Upval, i32::from(idx)));
        }

        // 3. Search parent functions (build upvalue chain)
        let current_idx = self.func_states.len() - 1;
        if current_idx > 0
            && let Some(uv_idx) = self.resolve_var_aux(current_idx, name)?
        {
            return Ok(ExprContext::new(ExprKind::Upval, i32::from(uv_idx)));
        }

        // 4. Global: add name to constants, return Global kind
        let k = self.string_constant(name.as_bytes())?;
        Ok(ExprContext {
            kind: ExprKind::Global,
            info: k as i32,
            aux: 0,
            nval: 0.0,
            t: NO_JUMP,
            f: NO_JUMP,
        })
    }

    /// Recursive upvalue resolution through parent function states.
    /// Returns the upvalue index in `func_states[fs_idx]` if found.
    fn resolve_var_aux(&mut self, fs_idx: usize, name: &str) -> LuaResult<Option<u8>> {
        if fs_idx == 0 {
            return Ok(None); // Reached global scope
        }

        let parent_idx = fs_idx - 1;

        // Search parent's locals
        let parent_fs = &self.func_states[parent_idx];
        for i in (0..parent_fs.num_active_vars).rev() {
            let var_idx = parent_fs.active_vars[i as usize];
            if parent_fs.proto.local_vars[var_idx as usize].name == name {
                // Found in parent's locals: capture as upvalue.
                // Mark the enclosing block so CLOSE is emitted when it ends.
                // PUC-Rio: markupval(fs, level) in lparser.c.
                Self::mark_upval(&mut self.func_states[parent_idx], i);
                let uv_idx = self.add_upvalue(fs_idx, name, true, i)?;
                return Ok(Some(uv_idx));
            }
        }

        // Search parent's upvalues
        let parent_fs = &self.func_states[parent_idx];
        for (i, uv) in parent_fs.upvalues.iter().enumerate() {
            if uv.name == name {
                // Found in parent's upvalues: chain it
                #[allow(clippy::cast_possible_truncation)]
                let uv_idx = self.add_upvalue(fs_idx, name, false, i as u8)?;
                return Ok(Some(uv_idx));
            }
        }

        // Recurse to grandparent
        if let Some(parent_uv) = self.resolve_var_aux(parent_idx, name)? {
            let uv_idx = self.add_upvalue(fs_idx, name, false, parent_uv)?;
            return Ok(Some(uv_idx));
        }

        Ok(None) // Not found, will be global
    }

    // -- Local variable management --

    /// Creates a new local variable entry and registers it (pushes to
    /// `active_vars`). The variable is not yet activated (visible) until
    /// `activate_locals` is called.
    pub(crate) fn new_local(&mut self, name: &str) -> LuaResult<u16> {
        let fs = self.fs_mut();
        if fs.active_vars.len() >= LUAI_MAXVARS as usize {
            return Err(self.syntax_error("too many local variables"));
        }
        let idx = fs.proto.local_vars.len();
        fs.proto.local_vars.push(LocalVar {
            name: name.to_string(),
            start_pc: 0,
            end_pc: 0,
        });
        #[allow(clippy::cast_possible_truncation)]
        let idx16 = idx as u16;
        fs.active_vars.push(idx16);
        Ok(idx16)
    }

    /// Activates `n` local variables (makes them visible).
    ///
    /// The `n` newest entries in `active_vars` (pushed by prior `new_local`
    /// calls) have their `start_pc` set to the current code position.
    /// Matches PUC-Rio's `adjustlocalvars`.
    pub(crate) fn activate_locals(&mut self, n: u32) {
        let fs = self.fs_mut();
        let pc = fs.proto.code.len();
        // The n new vars are at the end of active_vars.
        let start_idx = fs.active_vars.len() - n as usize;
        for i in 0..n as usize {
            let var_idx = fs.active_vars[start_idx + i] as usize;
            if var_idx < fs.proto.local_vars.len() {
                fs.proto.local_vars[var_idx].start_pc = pc as u32;
            }
        }
        fs.num_active_vars += n as u8;
    }

    /// Removes locals down to `to_level`, closing their debug info.
    pub(crate) fn remove_locals(&mut self, to_level: u8) {
        let fs = self.fs_mut();
        let pc = fs.proto.code.len() as u32;
        while fs.num_active_vars > to_level {
            fs.num_active_vars -= 1;
            if let Some(var_idx) = fs.active_vars.pop()
                && (var_idx as usize) < fs.proto.local_vars.len()
            {
                fs.proto.local_vars[var_idx as usize].end_pc = pc;
            }
        }
    }

    // -- Block management --

    /// Enters a new block scope.
    pub(crate) fn enter_block(&mut self, is_breakable: bool) {
        let num_active = self.fs().num_active_vars;
        self.fs_mut().blocks.push(BlockContext {
            num_active_vars: num_active,
            has_upval: false,
            is_breakable,
            break_list: NO_JUMP,
        });
    }

    /// Leaves a block scope, closing locals and upvalues.
    pub(crate) fn leave_block(&mut self) {
        if let Some(block) = self.fs_mut().blocks.pop() {
            self.remove_locals(block.num_active_vars);
            // Reset free register to match active vars (PUC-Rio: lparser.c:305)
            self.fs_mut().free_reg = self.fs().num_active_vars;
            if block.has_upval {
                // Emit OP_CLOSE to close upvalues
                let level = u32::from(block.num_active_vars);
                self.emit_abc(OpCode::Close, level, 0, 0, self.current_line);
            }
            if block.is_breakable {
                let pc = self.fs().pc();
                self.patch_list(block.break_list, pc);
            }
        }
    }

    /// Marks the block that needs CLOSE when a local at `level` is captured.
    ///
    /// Walks the block stack from innermost to outermost, stopping at the
    /// first block whose `num_active_vars <= level`. That block is the one
    /// entered just before the local was created, so it must emit CLOSE when
    /// it ends to capture the upvalue.
    ///
    /// Reference: `markupval(fs, level)` in PUC-Rio `lparser.c`.
    fn mark_upval(fs: &mut FuncState, level: u8) {
        for block in fs.blocks.iter_mut().rev() {
            if block.num_active_vars <= level {
                block.has_upval = true;
                return;
            }
        }
    }

    /// Records a break jump in the innermost breakable block.
    pub(crate) fn add_break_jump(&mut self, jump_pc: i32) -> LuaResult<()> {
        let fs = self.fs_mut();
        for block in fs.blocks.iter_mut().rev() {
            if block.is_breakable {
                let mut bl = block.break_list;
                // We need to concat jump_pc onto block.break_list
                // but we can't call self.concat_jumps here due to borrow.
                // Simple approach: just link directly
                if bl == NO_JUMP {
                    block.break_list = jump_pc;
                } else {
                    // Walk to end of break_list and patch
                    loop {
                        let instr = Instruction::from_raw(fs.proto.code[bl as usize]);
                        let next_offset = instr.sbx();
                        if next_offset == NO_JUMP {
                            // Patch this jump to point to jump_pc
                            let offset = jump_pc - bl - 1;
                            let mut patched = instr;
                            patched.set_sbx(offset);
                            fs.proto.code[bl as usize] = patched.raw();
                            break;
                        }
                        bl = bl + 1 + next_offset;
                    }
                }
                return Ok(());
            }
        }
        Err(self.syntax_error("no loop to break"))
    }

    // -- Expression discharge --

    /// Converts variable expressions to values by emitting load instructions.
    pub(crate) fn discharge_vars(&mut self, e: &mut ExprContext, line: u32) {
        match e.kind {
            ExprKind::Local => {
                e.kind = ExprKind::NonReloc;
                // info already has the register
            }
            ExprKind::Upval => {
                let pc = self.emit_abc(OpCode::GetUpval, 0, e.info as u32, 0, line);
                e.info = pc as i32;
                e.kind = ExprKind::Relocable;
            }
            ExprKind::Global => {
                let pc = self.emit_abx(OpCode::GetGlobal, 0, e.info as u32, line);
                e.info = pc as i32;
                e.kind = ExprKind::Relocable;
            }
            ExprKind::Indexed => {
                let table_reg = e.info as u32;
                let key_rk = e.aux as u32;
                self.free_reg(key_rk);
                self.free_reg(table_reg);
                let pc = self.emit_abc(OpCode::GetTable, 0, table_reg, key_rk, line);
                e.info = pc as i32;
                e.kind = ExprKind::Relocable;
            }
            ExprKind::Call | ExprKind::VarArg => {
                self.set_one_ret(e);
            }
            _ => {} // Constants and voids need no discharge
        }
    }

    /// Sets a call or vararg expression to return exactly one value.
    fn set_one_ret(&mut self, e: &mut ExprContext) {
        if e.kind == ExprKind::Call {
            let mut instr = self.get_instruction(e.info as usize);
            // Set C to 2 (1 result + 1)
            instr.set_c(2);
            self.set_instruction(e.info as usize, instr);
            e.kind = ExprKind::NonReloc;
            #[allow(clippy::cast_possible_wrap)]
            {
                e.info = instr.a() as i32;
            }
        } else if e.kind == ExprKind::VarArg {
            let mut instr = self.get_instruction(e.info as usize);
            instr.set_b(2); // 1 result
            self.set_instruction(e.info as usize, instr);
            e.kind = ExprKind::Relocable;
        }
    }

    /// Places an expression into the next free register.
    pub(crate) fn exp2nextreg(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<()> {
        self.discharge_vars(e, line);
        self.free_expr(e);
        let reg = self.alloc_reg()?;
        self.exp2reg(e, reg, line);
        Ok(())
    }

    /// Places an expression into any register (reuses if already in one).
    pub(crate) fn exp2anyreg(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<u32> {
        self.discharge_vars(e, line);
        if e.kind == ExprKind::NonReloc {
            if !e.has_jumps() {
                return Ok(e.info as u32);
            }
            if e.info as u32 >= u32::from(self.fs().num_active_vars) {
                // Reuse temporary register
                self.exp2reg(e, e.info as u32, line);
                return Ok(e.info as u32);
            }
        }
        self.exp2nextreg(e, line)?;
        Ok(e.info as u32)
    }

    /// Emits code to place an expression value into register `reg`.
    /// When the expression has jump lists (e.g., comparison results),
    /// emits LOADBOOL pair to materialize the boolean value.
    /// Maps to PUC-Rio's `exp2reg`.
    #[allow(clippy::cast_sign_loss)]
    pub(crate) fn exp2reg(&mut self, e: &mut ExprContext, reg: u32, line: u32) {
        self.discharge2reg(e, reg, line);
        if e.kind == ExprKind::Jmp {
            let mut e_t = e.t;
            self.concat_jumps(&mut e_t, e.info);
            e.t = e_t;
        }
        if e.has_jumps() {
            let mut p_f = NO_JUMP; // position of eventual LOADBOOL false
            let mut p_t = NO_JUMP; // position of eventual LOADBOOL true
            if self.need_value(e.t) || self.need_value(e.f) {
                let fj = if e.kind == ExprKind::Jmp {
                    NO_JUMP
                } else {
                    self.emit_jump(line) as i32
                };
                p_f = self.code_label(reg, 0, 1, line) as i32; // LOADBOOL reg 0 1 (false, skip)
                p_t = self.code_label(reg, 1, 0, line) as i32; // LOADBOOL reg 1 0 (true)
                self.patch_to_here(fj);
            }
            let final_pc = self.get_label();
            // When p_f/p_t are NO_JUMP, all jumps in that list must be
            // TESTSET (need_value was false), so dtarget is never reached.
            // Use final_pc as a safe fallback.
            let dt_f = if p_f == NO_JUMP {
                final_pc
            } else {
                p_f as usize
            };
            let dt_t = if p_t == NO_JUMP {
                final_pc
            } else {
                p_t as usize
            };
            self.patch_list_aux(e.f, final_pc, reg, dt_f);
            self.patch_list_aux(e.t, final_pc, reg, dt_t);
        }
        e.f = NO_JUMP;
        e.t = NO_JUMP;
        e.info = reg as i32;
        e.kind = ExprKind::NonReloc;
    }

    /// Discharges expression directly to a specific register.
    fn discharge2reg(&mut self, e: &mut ExprContext, reg: u32, line: u32) {
        self.discharge_vars(e, line);
        match e.kind {
            ExprKind::Nil => {
                self.emit_nil(reg, 1, line);
            }
            ExprKind::False | ExprKind::True => {
                let bool_val = u32::from(e.kind == ExprKind::True);
                self.emit_abc(OpCode::LoadBool, reg, bool_val, 0, line);
            }
            ExprKind::K => {
                self.emit_abx(OpCode::LoadK, reg, e.info as u32, line);
            }
            ExprKind::KNum => {
                let k = self.number_constant(e.nval).unwrap_or(0); // Should not fail in practice
                self.emit_abx(OpCode::LoadK, reg, k, line);
            }
            ExprKind::Relocable => {
                // Patch the A field of the relocable instruction
                let mut instr = self.get_instruction(e.info as usize);
                instr.set_a(reg);
                self.set_instruction(e.info as usize, instr);
            }
            ExprKind::NonReloc => {
                if reg != e.info as u32 {
                    self.emit_abc(OpCode::Move, reg, e.info as u32, 0, line);
                }
            }
            _ => {
                // Void, Jmp — no direct value to move
                return;
            }
        }
        e.info = reg as i32;
        e.kind = ExprKind::NonReloc;
    }

    /// Ensures expression is in any register. If already in a register
    /// (NonReloc), does nothing. Otherwise reserves a new register and
    /// discharges to it. Maps to PUC-Rio's `discharge2anyreg`.
    fn discharge2anyreg(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<()> {
        if e.kind != ExprKind::NonReloc {
            self.reserve_regs(1)?;
            let reg = u32::from(self.fs().free_reg) - 1;
            self.discharge2reg(e, reg, line);
        }
        Ok(())
    }

    /// Converts expression to RK format (register or constant).
    pub(crate) fn exp2rk(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<u32> {
        self.exp2val(e, line);
        match e.kind {
            ExprKind::True | ExprKind::False | ExprKind::Nil
                if self.fs().proto.constants.len() <= MAXINDEXRK as usize =>
            {
                // Small enough to encode: use constant
                let k = match e.kind {
                    ExprKind::Nil => self.nil_constant()?,
                    ExprKind::True => self.add_constant(Val::Bool(true))?,
                    _ => self.add_constant(Val::Bool(false))?, // False
                };
                e.info = k as i32;
                e.kind = ExprKind::K;
                return Ok(k | BITRK);
            }
            ExprKind::K if (e.info as u32) <= MAXINDEXRK => {
                return Ok(e.info as u32 | BITRK);
            }
            ExprKind::KNum => {
                let k = self.number_constant(e.nval)?;
                if k <= MAXINDEXRK {
                    e.info = k as i32;
                    e.kind = ExprKind::K;
                    return Ok(k | BITRK);
                }
            }
            _ => {}
        }
        // Fall through: place in register
        let reg = self.exp2anyreg(e, line)?;
        Ok(reg)
    }

    /// Frees the register used by an expression (if temporary).
    pub(crate) fn free_expr(&mut self, e: &ExprContext) {
        if e.kind == ExprKind::NonReloc {
            self.free_reg(e.info as u32);
        }
    }

    // -- Variable store --

    /// Stores expression `ex` into variable `var`.
    /// Maps to PUC-Rio's `luaK_storevar`.
    pub(crate) fn storevar(
        &mut self,
        var: &ExprContext,
        ex: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        match var.kind {
            ExprKind::Local => {
                self.free_expr(ex);
                self.exp2reg(ex, var.info as u32, line);
            }
            ExprKind::Upval => {
                let e = self.exp2anyreg(ex, line)?;
                self.emit_abc(OpCode::SetUpval, e, var.info as u32, 0, line);
            }
            ExprKind::Global => {
                let e = self.exp2anyreg(ex, line)?;
                self.emit_abx(OpCode::SetGlobal, e, var.info as u32, line);
            }
            ExprKind::Indexed => {
                let e = self.exp2rk(ex, line)?;
                self.emit_abc(OpCode::SetTable, var.info as u32, var.aux as u32, e, line);
            }
            _ => {
                // Invalid variable kind — should not happen
                return Err(self.syntax_error("invalid assignment target"));
            }
        }
        self.free_expr(ex);
        Ok(())
    }

    // -- Conditional jumps --

    /// Compiles a boolean condition expression, returning the false-list.
    /// Maps to PUC-Rio's `cond()` (lparser.c:963-970).
    ///
    /// Converts Nil to False before calling `goiftrue`, so that constant
    /// nil conditions generate unconditional jumps (no TEST instruction).
    /// This must NOT be used for `and`/`or` expressions where the actual
    /// value matters.
    pub(crate) fn compile_condition(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<i32> {
        if e.kind == ExprKind::Nil {
            e.kind = ExprKind::False;
        }
        self.goiftrue(e, line)?;
        Ok(e.f)
    }

    /// Converts expression to "go if true" — jumps to the false list on false.
    /// Maps to PUC-Rio's `luaK_goiftrue`.
    pub(crate) fn goiftrue(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<()> {
        self.discharge_vars(e, line);
        let pc = match e.kind {
            ExprKind::K | ExprKind::KNum | ExprKind::True => {
                NO_JUMP // always true, no jump
            }
            ExprKind::False => self.emit_jump(line) as i32,
            ExprKind::Jmp => {
                self.invertjump(e);
                e.info
            }
            _ => self.jumponcond(e, false, line)?,
        };
        // Insert the jump into the false list
        let mut f = e.f;
        self.concat_jumps(&mut f, pc);
        e.f = f;
        self.patch_to_here(e.t);
        e.t = NO_JUMP;
        Ok(())
    }

    /// Converts expression to "go if false" — jumps to the true list on true.
    /// Maps to PUC-Rio's `luaK_goiffalse`.
    pub(crate) fn goiffalse(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<()> {
        self.discharge_vars(e, line);
        let pc = match e.kind {
            ExprKind::Nil | ExprKind::False => {
                NO_JUMP // always false, no jump
            }
            ExprKind::True => self.emit_jump(line) as i32,
            ExprKind::Jmp => e.info,
            _ => self.jumponcond(e, true, line)?,
        };
        // Insert the jump into the true list
        let mut t = e.t;
        self.concat_jumps(&mut t, pc);
        e.t = t;
        self.patch_to_here(e.f);
        e.f = NO_JUMP;
        Ok(())
    }

    /// Returns the pc of the "control" instruction for a given jump.
    ///
    /// If the instruction before the JMP is a test-mode instruction
    /// (EQ, LT, LE, TEST, TESTSET), returns its pc. Otherwise returns
    /// the JMP's own pc.
    ///
    /// Maps to PUC-Rio's `getjumpcontrol`.
    fn get_jump_control(&self, pc: usize) -> usize {
        if pc >= 1 {
            let prev = self.get_instruction(pc - 1);
            if prev.opcode().is_test_mode() {
                return pc - 1;
            }
        }
        pc
    }

    /// Inverts the condition of a comparison/test instruction before a JMP.
    ///
    /// Maps to PUC-Rio's `invertjump`. Uses `get_jump_control` to find the
    /// comparison instruction (at `pc - 1`), not the JMP itself.
    fn invertjump(&mut self, e: &ExprContext) {
        let jmp_pc = e.info as usize;
        let ctrl_pc = self.get_jump_control(jmp_pc);
        let mut instr = self.get_instruction(ctrl_pc);
        let a = instr.a();
        instr.set_a(u32::from(a == 0));
        self.set_instruction(ctrl_pc, instr);
    }

    /// Emits TEST + JMP for conditional expression.
    /// Returns the pc of the JMP instruction.
    fn jumponcond(&mut self, e: &mut ExprContext, cond: bool, line: u32) -> LuaResult<i32> {
        // If expression is a relocable NOT instruction, optimize:
        // remove the NOT, emit TEST with inverted condition + JMP.
        if e.kind == ExprKind::Relocable {
            let instr = self.get_instruction(e.info as usize);
            if instr.opcode() == OpCode::Not {
                self.fs_mut().proto.code.pop();
                self.fs_mut().proto.line_info.pop();
                let cond_val = u32::from(!cond);
                self.emit_abc(OpCode::Test, instr.b(), 0, cond_val, line);
                return Ok(self.emit_jump(line) as i32);
            }
        }
        // General case: discharge to any register, emit TESTSET + JMP.
        // TESTSET with A=NO_REG enables patch_test_reg to later set
        // the target register for and/or value propagation.
        self.discharge2anyreg(e, line)?;
        self.free_expr(e);
        let cond_val = u32::from(cond);
        self.emit_abc(OpCode::TestSet, NO_REG, e.info as u32, cond_val, line);
        Ok(self.emit_jump(line) as i32)
    }

    // -- Expression value conversion --

    /// Discharges expression to a value form (not necessarily a register).
    /// Like `discharge_vars` but also handles jump patching.
    pub(crate) fn exp2val(&mut self, e: &mut ExprContext, line: u32) {
        if e.has_jumps() {
            // exp2anyreg returns Result but we intentionally ignore it here
            // since we're only interested in the side effect
            drop(self.exp2anyreg(e, line));
        } else {
            self.discharge_vars(e, line);
        }
    }

    /// Sets an expression to multi-return mode (B=0 for calls, vararg).
    pub(crate) fn set_multret(&mut self, e: &mut ExprContext) {
        // Directly patch for MULTRET without calling set_one_ret.
        // PUC-Rio's luaK_setmultret calls luaK_setreturns(fs, e, LUA_MULTRET)
        // which sets C=0 for CALL and B=0 for VARARG.
        if e.kind == ExprKind::Call {
            let mut instr = self.get_instruction(e.info as usize);
            instr.set_c(0); // 0 = multi-return
            self.set_instruction(e.info as usize, instr);
        } else if e.kind == ExprKind::VarArg {
            let mut instr = self.get_instruction(e.info as usize);
            instr.set_b(0); // 0 = multi-return
            instr.set_a(u32::from(self.fs().free_reg));
            self.set_instruction(e.info as usize, instr);
            self.reserve_regs(1).ok(); // PUC-Rio reserves one register
            e.kind = ExprKind::Relocable;
        }
    }

    // -- Code generation for operators --

    /// Sets an expression to an indexed form: `info=table_reg, aux=key_rk`.
    pub(crate) fn set_indexed(
        &mut self,
        table: &mut ExprContext,
        key: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        table.aux = self.exp2rk(key, line)? as i32;
        table.kind = ExprKind::Indexed;
        Ok(())
    }

    /// Emits OP_SELF: `R(A+1) := R(B); R(A) := R(B)[RK(C)]`.
    pub(crate) fn code_self(
        &mut self,
        e: &mut ExprContext,
        key: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        self.exp2anyreg(e, line)?;
        self.free_expr(e);
        let func = u32::from(self.fs().free_reg);
        self.reserve_regs(2)?;
        let key_rk = self.exp2rk(key, line)?;
        self.emit_abc(OpCode::OpSelf, func, e.info as u32, key_rk, line);
        self.free_expr(key);
        e.info = func as i32;
        e.kind = ExprKind::NonReloc;
        Ok(())
    }

    /// Attempts constant folding for arithmetic operations.
    /// Matches PUC-Rio's `constfolding` (lcode.c:630-653).
    /// Returns true if the operation was folded (result stored in e1).
    fn const_fold(op: OpCode, e1: &mut ExprContext, e2: &ExprContext) -> bool {
        if !e1.is_numeral() || !e2.is_numeral() {
            return false;
        }
        let v1 = e1.nval;
        let v2 = e2.nval;
        let r = match op {
            OpCode::Add => v1 + v2,
            OpCode::Sub => v1 - v2,
            OpCode::Mul => v1 * v2,
            OpCode::Div => {
                if v2 == 0.0 {
                    return false;
                }
                v1 / v2
            }
            OpCode::Mod => {
                if v2 == 0.0 {
                    return false;
                }
                (v1 / v2).floor().mul_add(-v2, v1)
            }
            OpCode::Pow => v1.powf(v2),
            OpCode::Unm => -v1,
            _ => return false,
        };
        if r.is_nan() {
            return false;
        }
        e1.nval = r;
        true
    }

    /// Emits an arithmetic/general binary operation.
    /// Maps to PUC-Rio's `codearith`.
    pub(crate) fn code_arith(
        &mut self,
        op: OpCode,
        e1: &mut ExprContext,
        e2: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        // Try constant folding (PUC-Rio lcode.c:630-653).
        if Self::const_fold(op, e1, e2) {
            return Ok(());
        }
        if op == OpCode::Concat {
            // Concat is special: operands must be in consecutive registers
            self.exp2nextreg(e2, line)?;
            self.exp2anyreg(e1, line)?;
        }
        let (b, c) = if op == OpCode::Unm || op == OpCode::Not || op == OpCode::Len {
            // Unary ops: B = operand, C = 0
            let b = self.exp2anyreg(e1, line)?;
            (b, 0)
        } else if op == OpCode::Concat {
            let b = e1.info as u32;
            let c = e2.info as u32;
            self.free_expr(e2);
            self.free_expr(e1);
            (b, c)
        } else {
            // Binary ops: both can be RK
            let c = self.exp2rk(e2, line)?;
            let b = self.exp2rk(e1, line)?;
            (b, c)
        };
        if op != OpCode::Concat {
            self.free_expr(e2);
            self.free_expr(e1);
        }
        e1.info = self.emit_abc(op, 0, b, c, line) as i32;
        e1.kind = ExprKind::Relocable;
        Ok(())
    }

    /// Emits a comparison operation (EQ/LT/LE).
    /// Maps to PUC-Rio's `codecomp`.
    pub(crate) fn code_comp(
        &mut self,
        op: OpCode,
        cond: u32,
        e1: &mut ExprContext,
        e2: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        let mut b = self.exp2rk(e1, line)?;
        let mut c = self.exp2rk(e2, line)?;
        self.free_expr(e2);
        self.free_expr(e1);
        let mut cond = cond;
        // For GT/GE (cond=0, non-EQ), swap operands and set cond=1.
        // This matches PUC-Rio's codecomp: the caller passes swapped
        // expression args with cond=0, and we swap the RK indices back
        // while flipping cond to 1.
        if cond == 0 && op != OpCode::Eq {
            std::mem::swap(&mut b, &mut c);
            cond = 1;
        }
        self.emit_abc(op, cond, b, c, line);
        let jmp = self.emit_jump(line);
        e1.info = jmp as i32;
        e1.kind = ExprKind::Jmp;
        e1.t = NO_JUMP;
        e1.f = NO_JUMP;
        Ok(())
    }

    /// Pre-processes the left side of a binary operation.
    /// For `and`/`or`, emits a conditional jump.
    /// Maps to PUC-Rio's `luaK_infix`.
    pub(crate) fn infix(
        &mut self,
        op: super::ast::BinOp,
        e: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        match op {
            super::ast::BinOp::And => {
                self.goiftrue(e, line)?;
            }
            super::ast::BinOp::Or => {
                self.goiffalse(e, line)?;
            }
            super::ast::BinOp::Concat => {
                self.exp2nextreg(e, line)?;
            }
            _ => {
                // Arithmetic and comparisons: use RK form.
                // Skip for pure numerals (no jumps) -- they're already optimal.
                // Matches PUC-Rio's `if (!isnumeral(v)) luaK_exp2RK(fs, v)`.
                if !e.is_numeral() {
                    self.exp2rk(e, line)?;
                }
            }
        }
        Ok(())
    }

    /// Combines left and right sides of a binary operation.
    /// Maps to PUC-Rio's `luaK_posfix`.
    pub(crate) fn postfix(
        &mut self,
        op: super::ast::BinOp,
        e1: &mut ExprContext,
        e2: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        match op {
            super::ast::BinOp::And => {
                debug_assert!(e1.t == NO_JUMP);
                self.discharge_vars(e2, line);
                let mut f = e2.f;
                self.concat_jumps(&mut f, e1.f);
                e2.f = f;
                *e1 = *e2;
            }
            super::ast::BinOp::Or => {
                debug_assert!(e1.f == NO_JUMP);
                self.discharge_vars(e2, line);
                let mut t = e2.t;
                self.concat_jumps(&mut t, e1.t);
                e2.t = t;
                *e1 = *e2;
            }
            super::ast::BinOp::Concat => {
                self.exp2val(e2, line);
                // Check if we can merge with an existing CONCAT
                if e2.kind == ExprKind::Relocable {
                    let instr = self.get_instruction(e2.info as usize);
                    if instr.opcode() == OpCode::Concat {
                        self.free_expr(e1);
                        let mut merged = self.get_instruction(e2.info as usize);
                        merged.set_b(e1.info as u32);
                        self.set_instruction(e2.info as usize, merged);
                        e1.kind = ExprKind::Relocable;
                        e1.info = e2.info;
                        return Ok(());
                    }
                }
                self.exp2nextreg(e2, line)?;
                self.code_arith(OpCode::Concat, e1, e2, line)?;
            }
            super::ast::BinOp::Add => self.code_arith(OpCode::Add, e1, e2, line)?,
            super::ast::BinOp::Sub => self.code_arith(OpCode::Sub, e1, e2, line)?,
            super::ast::BinOp::Mul => self.code_arith(OpCode::Mul, e1, e2, line)?,
            super::ast::BinOp::Div => self.code_arith(OpCode::Div, e1, e2, line)?,
            super::ast::BinOp::Mod => self.code_arith(OpCode::Mod, e1, e2, line)?,
            super::ast::BinOp::Pow => self.code_arith(OpCode::Pow, e1, e2, line)?,
            super::ast::BinOp::Eq => self.code_comp(OpCode::Eq, 1, e1, e2, line)?,
            super::ast::BinOp::Ne => self.code_comp(OpCode::Eq, 0, e1, e2, line)?,
            super::ast::BinOp::Lt => self.code_comp(OpCode::Lt, 1, e1, e2, line)?,
            super::ast::BinOp::Le => self.code_comp(OpCode::Le, 1, e1, e2, line)?,
            // GT and GE: pass cond=0; code_comp swaps the RK indices
            // internally and sets cond=1 (matching PUC-Rio's codecomp).
            // Expression args are NOT swapped — the swap is on o1/o2 inside.
            super::ast::BinOp::Gt => self.code_comp(OpCode::Lt, 0, e1, e2, line)?,
            super::ast::BinOp::Ge => self.code_comp(OpCode::Le, 0, e1, e2, line)?,
        }
        Ok(())
    }

    /// Applies a unary prefix operation.
    /// Maps to PUC-Rio's `luaK_prefix`.
    pub(crate) fn prefix(
        &mut self,
        op: super::ast::UnOp,
        e: &mut ExprContext,
        line: u32,
    ) -> LuaResult<()> {
        let mut e2 = ExprContext::number(0.0);
        match op {
            super::ast::UnOp::Neg => {
                if e.kind == ExprKind::K {
                    self.exp2anyreg(e, line)?;
                }
                self.code_arith(OpCode::Unm, e, &mut e2, line)?;
            }
            super::ast::UnOp::Not => {
                self.code_not(e, line)?;
            }
            super::ast::UnOp::Len => {
                self.exp2anyreg(e, line)?;
                self.code_arith(OpCode::Len, e, &mut e2, line)?;
            }
        }
        Ok(())
    }

    /// Emits logical NOT.
    ///
    /// Matches PUC-Rio's `codenot`: discharges the expression, then for
    /// `Relocable`/`NonReloc` values uses `discharge2anyreg` (NOT
    /// `discharge2reg(e.info)`) so that `Relocable` expressions get a
    /// proper register allocation instead of misusing the instruction
    /// index as a register number.
    fn code_not(&mut self, e: &mut ExprContext, line: u32) -> LuaResult<()> {
        self.discharge_vars(e, line);
        match e.kind {
            ExprKind::Nil | ExprKind::False => {
                e.kind = ExprKind::True;
            }
            ExprKind::K | ExprKind::KNum | ExprKind::True => {
                e.kind = ExprKind::False;
            }
            ExprKind::Jmp => {
                self.invertjump(e);
            }
            ExprKind::Relocable | ExprKind::NonReloc => {
                self.discharge2anyreg(e, line)?;
                self.free_expr(e);
                e.info = self.emit_abc(OpCode::Not, 0, e.info as u32, 0, line) as i32;
                e.kind = ExprKind::Relocable;
            }
            _ => {} // Void — no-op
        }
        // Swap true and false lists
        std::mem::swap(&mut e.t, &mut e.f);
        // PUC-Rio: removevalues on both lists after swap.
        // Converts TESTSET -> TEST so negated expressions don't carry
        // stale value-assignment semantics from the original and/or.
        self.remove_values(e.f);
        self.remove_values(e.t);
        Ok(())
    }

    /// Returns the current pc (for use as a label/loop target).
    pub(crate) fn get_label(&mut self) -> usize {
        let pc = self.fs().pc();
        self.fs_mut().last_target = pc as i32;
        pc
    }

    /// Emits SETLIST for table construction.
    pub(crate) fn code_setlist(&mut self, base: u32, nelems: u32, tostore: u32, line: u32) {
        let c = (nelems - 1) / LFIELDS_PER_FLUSH + 1;
        let b = if tostore == 0 { 0 } else { tostore };
        if c <= MAXARG_C {
            self.emit_abc(OpCode::SetList, base, b, c, line);
        } else {
            self.emit_abc(OpCode::SetList, base, b, 0, line);
            self.emit(Instruction::from_raw(c), line);
        }
        self.fs_mut().free_reg = (base + 1) as u8;
    }

    // -- Function state management --

    /// Pushes a new function state for compiling an inner function.
    pub(crate) fn enter_function(&mut self, source: &str) {
        let fs = FuncState::new(source);
        self.func_states.push(fs);
    }

    /// Pops the current function state and returns its completed Proto.
    ///
    /// # Panics
    /// Panics if called when no function state exists.
    #[allow(clippy::expect_used)]
    pub(crate) fn leave_function(&mut self) -> Proto {
        // Emit final return
        self.emit_abc(OpCode::Return, 0, 1, 0, self.current_line);
        // Close remaining local variable debug info (PUC-Rio: removevars(ls, 0)).
        self.remove_locals(0);
        let mut fs = self
            .func_states
            .pop()
            .expect("cannot leave global function");
        // Copy upvalue names to proto for debug info.
        fs.proto.upvalue_names = fs.upvalues.iter().map(|uv| uv.name.clone()).collect();
        fs.proto
    }

    /// Finalizes the main chunk's Proto.
    ///
    /// # Panics
    /// Panics if called when no function state exists.
    #[allow(clippy::expect_used)]
    fn finish_main(&mut self) -> Proto {
        // Emit final return for main chunk
        self.emit_abc(OpCode::Return, 0, 1, 0, self.current_line);
        // Close remaining local variable debug info (PUC-Rio: removevars(ls, 0)).
        self.remove_locals(0);
        let mut fs = self
            .func_states
            .pop()
            .expect("cannot finish without main function");
        // Copy upvalue names to proto for debug info.
        fs.proto.upvalue_names = fs.upvalues.iter().map(|uv| uv.name.clone()).collect();
        fs.proto
    }
}

/// Compiles a Lua source string into a Proto (function prototype).
pub fn compile(source: &[u8], name: &str) -> LuaResult<ProtoRef> {
    let block = parser::parse(source, name)?;
    let mut compiler = Compiler::new(name);

    // Main chunk: vararg function with 0 params
    compiler.fs_mut().proto.is_vararg = 2; // VARARG_ISVARARG
    compiler.fs_mut().proto.num_params = 0;

    compile_block(&mut compiler, &block)?;

    let proto = compiler.finish_main();
    Ok(ProtoRef::new(proto))
}

/// Compiles Lua source from a reader-based lexer into a Proto.
///
/// The lexer pulls data on demand from its reader function, matching
/// PUC-Rio's ZIO model where the lexer drives I/O.
pub fn compile_with_lexer(lexer: Lexer<'_>, name: &str) -> LuaResult<ProtoRef> {
    let block = parser::parse_with_lexer(lexer)?;
    let mut compiler = Compiler::new(name);

    // Main chunk: vararg function with 0 params
    compiler.fs_mut().proto.is_vararg = 2; // VARARG_ISVARARG
    compiler.fs_mut().proto.num_params = 0;

    compile_block(&mut compiler, &block)?;

    let proto = compiler.finish_main();
    Ok(ProtoRef::new(proto))
}

/// Compiles a block of statements inside a new scope.
fn compile_block_scoped(compiler: &mut Compiler, block: &Block) -> LuaResult<()> {
    compiler.enter_block(false);
    compile_block(compiler, block)?;
    compiler.leave_block();
    Ok(())
}

/// Compiles a sequence of statements.
fn compile_block(compiler: &mut Compiler, block: &Block) -> LuaResult<()> {
    for stat in block {
        compile_stat(compiler, stat)?;
        // After each statement, free temporary registers
        compiler.fs_mut().free_reg = compiler.fs().num_active_vars;
    }
    Ok(())
}

/// Compiles a single statement.
#[allow(clippy::too_many_lines)]
fn compile_stat(compiler: &mut Compiler, stat: &super::ast::Stat) -> LuaResult<()> {
    use super::ast::Stat;
    let line = stat.span().line;
    compiler.current_line = line;

    match stat {
        Stat::Assign {
            targets, values, ..
        } => compile_assign(compiler, targets, values, line),

        Stat::LocalDecl { names, values, .. } => compile_local_decl(compiler, names, values, line),

        Stat::Do { end_line, body, .. } => {
            let result = compile_block_scoped(compiler, body);
            // PUC-Rio: check_match consumes `end`, updating lastline.
            compiler.current_line = *end_line;
            result
        }

        Stat::While {
            condition,
            body,
            end_line,
            ..
        } => compile_while(compiler, condition, body, line, *end_line),

        Stat::Repeat {
            body, condition, ..
        } => compile_repeat(compiler, body, condition, line),

        Stat::If {
            conditions,
            bodies,
            else_body,
            end_line,
            ..
        } => compile_if(compiler, conditions, bodies, else_body.as_ref(), *end_line),

        Stat::NumericFor {
            name,
            start,
            stop,
            step,
            body,
            end_line,
            ..
        } => compile_numeric_for(
            compiler,
            name,
            start,
            stop,
            step.as_ref(),
            body,
            line,
            *end_line,
        ),

        Stat::GenericFor {
            names,
            iterators,
            body,
            iter_line,
            end_line,
            ..
        } => compile_generic_for(
            compiler, names, iterators, body, line, *iter_line, *end_line,
        ),

        Stat::FuncDecl { name, body, .. } => compile_func_decl(compiler, name, body, line),

        Stat::LocalFunc { name, body, .. } => compile_local_func(compiler, name, body, line),

        Stat::Return { values, .. } => compile_return(compiler, values, line),

        Stat::Break { .. } => {
            // PUC-Rio's breakstat: walk up block stack to the breakable
            // loop. If any block along the way has upvalues, emit CLOSE
            // to close them before jumping out of the loop.
            let fs = compiler.fs();
            let mut needs_close = false;
            let mut close_level = 0u32;
            for block in fs.blocks.iter().rev() {
                if block.has_upval {
                    needs_close = true;
                }
                if block.is_breakable {
                    close_level = u32::from(block.num_active_vars);
                    break;
                }
            }
            if needs_close {
                compiler.emit_abc(OpCode::Close, close_level, 0, 0, line);
            }
            let jmp = compiler.emit_jump(line) as i32;
            compiler.add_break_jump(jmp)?;
            Ok(())
        }

        Stat::ExprStat { expr, .. } => compile_expr_stat(compiler, expr, line),
    }
}

// -- Statement compilation helpers --

fn compile_assign(
    compiler: &mut Compiler,
    targets: &[super::ast::Expr],
    values: &[super::ast::Expr],
    line: u32,
) -> LuaResult<()> {
    // Compile all targets to get their variable locations.
    let mut target_exprs: Vec<ExprContext> = Vec::new();
    for target in targets {
        let e = compile_expr(compiler, target)?;
        if e.kind != ExprKind::Local
            && e.kind != ExprKind::Upval
            && e.kind != ExprKind::Global
            && e.kind != ExprKind::Indexed
        {
            return Err(LuaError::Syntax(SyntaxError {
                message: "invalid assignment target".to_string(),
                source: compiler.source_name.clone(),
                line,
                raw_message: None,
            }));
        }
        target_exprs.push(e);
    }

    // check_conflict: when a local is assigned, save it if any earlier
    // INDEXED target references its register (table or key). Matches
    // PUC-Rio's `check_conflict` in `lparser.c`. Without this, the
    // SETTABLE for early targets would use the overwritten register.
    for i in 0..target_exprs.len() {
        if target_exprs[i].kind == ExprKind::Local {
            let local_reg = target_exprs[i].info;
            let extra = i32::from(compiler.fs().free_reg);
            let mut conflict = false;
            for target_expr in &mut target_exprs[..i] {
                if target_expr.kind == ExprKind::Indexed {
                    // Check if table register matches the local.
                    if target_expr.info == local_reg {
                        conflict = true;
                        target_expr.info = extra;
                    }
                    // Check if key is a register (not constant) that matches.
                    let aux = target_expr.aux;
                    if aux & 256 == 0 && aux == local_reg {
                        conflict = true;
                        target_expr.aux = extra;
                    }
                }
            }
            if conflict {
                #[allow(clippy::cast_sign_loss)]
                compiler.emit_abc(OpCode::Move, extra as u32, local_reg as u32, 0, line);
                compiler.reserve_regs(1)?;
            }
        }
    }

    let nvars = targets.len();
    let (nexps, mut last_e) = compile_exprlist(compiler, values, line)?;

    // Following PUC-Rio's `assignment` pattern:
    // When nexps == nvars: store last value directly to last target,
    // then assign remaining targets from free_reg-1 (reverse order).
    // When nexps != nvars: adjust first, then assign all from free_reg-1.
    if nexps == nvars {
        // nexps == nvars: last target gets the last expression directly.
        compiler.set_one_ret(&mut last_e);
        let last_target = target_exprs[nvars - 1];
        compiler.storevar(&last_target, &mut last_e, line)?;
        // Remaining targets in reverse, each using free_reg-1.
        // storevar calls freeexp which decrements free_reg, so each
        // iteration naturally picks up the next value register.
        for i in (0..nvars - 1).rev() {
            let reg = u32::from(compiler.fs().free_reg) - 1;
            let mut val_e = ExprContext::new(ExprKind::NonReloc, reg as i32);
            let t = target_exprs[i];
            compiler.storevar(&t, &mut val_e, line)?;
        }
    } else {
        adjust_assign(compiler, nvars, nexps, &mut last_e, line)?;
        #[allow(clippy::cast_possible_truncation)]
        if nexps > nvars {
            compiler.fs_mut().free_reg -= (nexps - nvars) as u8;
        }
        // All targets assigned from free_reg-1 in reverse.
        for target in target_exprs.iter().rev() {
            let reg = u32::from(compiler.fs().free_reg) - 1;
            let mut val_e = ExprContext::new(ExprKind::NonReloc, reg as i32);
            let t = *target;
            compiler.storevar(&t, &mut val_e, line)?;
        }
    }

    Ok(())
}

fn compile_local_decl(
    compiler: &mut Compiler,
    names: &[String],
    values: &[super::ast::Expr],
    line: u32,
) -> LuaResult<()> {
    let nvars = names.len();

    // Register the local variables (but don't activate yet)
    for name in names {
        compiler.new_local(name)?;
    }

    if values.is_empty() {
        // No initializers: adjust assigns nil values
        adjust_assign(compiler, nvars, 0, &mut ExprContext::void(), line)?;
    } else {
        let (nexps, mut last_e) = compile_exprlist(compiler, values, line)?;
        adjust_assign(compiler, nvars, nexps, &mut last_e, line)?;
    }

    #[allow(clippy::cast_possible_truncation)]
    compiler.activate_locals(nvars as u32);
    Ok(())
}

fn compile_while(
    compiler: &mut Compiler,
    condition: &super::ast::Expr,
    body: &Block,
    _line: u32,
    end_line: u32,
) -> LuaResult<()> {
    let whileinit = compiler.get_label();
    let mut cond_e = compile_expr(compiler, condition)?;
    // PUC-Rio: cond() parses the condition, so TEST/JMP use the condition's
    // lastline. Use condition span line as approximation.
    let cond_line = condition.span().line;
    let condexit = compiler.compile_condition(&mut cond_e, cond_line)?;

    compiler.enter_block(true); // breakable
    compile_block(compiler, body)?;
    // PUC-Rio: luaK_jump uses lastline from the last token in the body.
    let jmp = compiler.emit_jump(compiler.current_line);
    compiler.patch_list(jmp as i32, whileinit);
    compiler.leave_block();
    compiler.patch_to_here(condexit);

    // PUC-Rio: check_match consumes `end`, updating lastline.
    compiler.current_line = end_line;
    Ok(())
}

fn compile_repeat(
    compiler: &mut Compiler,
    body: &Block,
    condition: &super::ast::Expr,
    line: u32,
) -> LuaResult<()> {
    let repeat_init = compiler.get_label();
    compiler.enter_block(true); // loop block (breakable)
    compiler.enter_block(false); // scope block

    compile_block(compiler, body)?;

    let mut cond_e = compile_expr(compiler, condition)?;
    // PUC-Rio: cond() uses lastline from parsing the condition expression.
    let cond_line = condition.span().line;
    let condexit = compiler.compile_condition(&mut cond_e, cond_line)?;

    // Check if the scope block has captured upvalues.
    // PUC-Rio's repeatstat (lparser.c:1020) checks bl2.upval.
    let scope_has_upval = compiler.fs().blocks.last().is_some_and(|b| b.has_upval);

    if scope_has_upval {
        // Upvalue case: condition TRUE means exit loop, FALSE means
        // close scope and repeat.
        //
        // PUC-Rio's approach (lparser.c:1024-1029):
        //   breakstat(ls);           -- if TRUE, CLOSE + JMP to break
        //   patchtohere(condexit);   -- FALSE falls through
        //   leaveblock(scope);       -- CLOSE scope vars
        //   JMP repeat_init;         -- loop back
        //   leaveblock(loop);        -- patches break list

        // Emit break: CLOSE + JMP to exit loop.
        // Walk blocks to find the breakable loop and its active var level.
        let fs = compiler.fs();
        let mut close_level = 0u32;
        for block in fs.blocks.iter().rev() {
            if block.is_breakable {
                close_level = u32::from(block.num_active_vars);
                break;
            }
        }
        compiler.emit_abc(OpCode::Close, close_level, 0, 0, line);
        let break_jmp = compiler.emit_jump(line) as i32;
        compiler.add_break_jump(break_jmp)?;

        // FALSE branch: falls through here.
        compiler.patch_to_here(condexit);

        // Leave scope block (emits CLOSE for scope variables).
        compiler.leave_block();

        // Jump back to repeat_init.
        let loop_back = compiler.emit_jump(line);
        compiler.patch_list(loop_back as i32, repeat_init);
    } else {
        // Simple case: no upvalues, just leave scope and loop back.
        compiler.leave_block(); // scope
        compiler.patch_list(condexit, repeat_init);
    }

    compiler.leave_block(); // loop (patches break list)
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn compile_if(
    compiler: &mut Compiler,
    conditions: &[super::ast::Expr],
    bodies: &[Block],
    else_body: Option<&Block>,
    end_line: u32,
) -> LuaResult<()> {
    let mut escape_list = NO_JUMP;

    // First condition + body (the 'if' part)
    // PUC-Rio: cond() parses the expression, so TEST/JMP use the condition's
    // lastline, not the `if` keyword's line.
    let mut cond_e = compile_expr(compiler, &conditions[0])?;
    let cond_line = conditions[0].span().line;
    let mut flist = compiler.compile_condition(&mut cond_e, cond_line)?;

    compile_block_scoped(compiler, &bodies[0])?;

    // Process elseif clauses
    for i in 1..conditions.len() {
        // PUC-Rio: luaK_jump uses lastline, which is the last consumed token
        // from the previous block. compiler.current_line approximates this.
        let jmp = compiler.emit_jump(compiler.current_line) as i32;
        compiler.concat_jumps(&mut escape_list, jmp);
        compiler.patch_to_here(flist);

        let mut cond_e = compile_expr(compiler, &conditions[i])?;
        let cond_line = conditions[i].span().line;
        flist = compiler.compile_condition(&mut cond_e, cond_line)?;

        compile_block_scoped(compiler, &bodies[i])?;
    }

    // Else clause
    if let Some(else_block) = else_body {
        let jmp = compiler.emit_jump(compiler.current_line) as i32;
        compiler.concat_jumps(&mut escape_list, jmp);
        compiler.patch_to_here(flist);
        compile_block_scoped(compiler, else_block)?;
    } else {
        compiler.concat_jumps(&mut escape_list, flist);
    }

    compiler.patch_to_here(escape_list);

    // PUC-Rio: check_match consumes `end`, updating lastline to the `end`
    // keyword's line. Update current_line so subsequent instructions (e.g.,
    // CLOSE from leave_block) use this line.
    compiler.current_line = end_line;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_numeric_for(
    compiler: &mut Compiler,
    name: &str,
    start: &super::ast::Expr,
    stop: &super::ast::Expr,
    step: Option<&super::ast::Expr>,
    body: &Block,
    line: u32,
    end_line: u32,
) -> LuaResult<()> {
    compiler.enter_block(true); // loop scope (breakable)
    let base = u32::from(compiler.fs().free_reg);

    // Create 3 hidden control variables + 1 user variable
    compiler.new_local("(for index)")?;
    compiler.new_local("(for limit)")?;
    compiler.new_local("(for step)")?;
    compiler.new_local(name)?;

    // Compile start, stop, step expressions into consecutive registers
    let mut e = compile_expr(compiler, start)?;
    compiler.exp2nextreg(&mut e, line)?;
    let mut e = compile_expr(compiler, stop)?;
    compiler.exp2nextreg(&mut e, line)?;
    if let Some(step_expr) = step {
        let mut e = compile_expr(compiler, step_expr)?;
        compiler.exp2nextreg(&mut e, line)?;
    } else {
        // Default step = 1
        let k = compiler.number_constant(1.0)?;
        let reg = u32::from(compiler.fs().free_reg);
        compiler.emit_abx(OpCode::LoadK, reg, k, line);
        compiler.reserve_regs(1)?;
    }

    // Activate the 3 control variables
    compiler.activate_locals(3);

    // FORPREP: init and jump to condition check
    let prep = compiler.emit_asbx(OpCode::ForPrep, base, NO_JUMP, line);

    // Inner block for user variable
    compiler.enter_block(false);
    compiler.activate_locals(1); // user variable
    compiler.reserve_regs(1)?;

    compile_block(compiler, body)?;

    compiler.leave_block();

    // Patch FORPREP to jump here (the FORLOOP instruction)
    compiler.patch_to_here(prep as i32);

    // FORLOOP: increment and loop
    // PUC-Rio: FORLOOP uses lastline, which is the line of the `end` keyword
    // (check_match just consumed it). Use end_line.
    let endfor = compiler.emit_asbx(OpCode::ForLoop, base, NO_JUMP, line);
    compiler.patch_jump(endfor, prep + 1);

    compiler.leave_block(); // loop scope
    compiler.current_line = end_line;
    Ok(())
}

fn compile_generic_for(
    compiler: &mut Compiler,
    names: &[String],
    iterators: &[super::ast::Expr],
    body: &Block,
    line: u32,
    iter_line: u32,
    end_line: u32,
) -> LuaResult<()> {
    compiler.enter_block(true); // loop scope (breakable)
    let base = u32::from(compiler.fs().free_reg);

    // 3 hidden control variables
    compiler.new_local("(for generator)")?;
    compiler.new_local("(for state)")?;
    compiler.new_local("(for control)")?;

    // User-declared variables
    for name in names {
        compiler.new_local(name)?;
    }

    // Compile iterator expressions, adjust to exactly 3
    let (nexps, mut last_e) = compile_exprlist(compiler, iterators, line)?;
    adjust_assign(compiler, 3, nexps, &mut last_e, line)?;

    compiler.check_stack(3)?;
    compiler.activate_locals(3); // control vars

    // JMP over the loop body to TFORLOOP
    let prep = compiler.emit_jump(line);

    // Inner block for declared variables
    compiler.enter_block(false);
    let nvars = names.len();
    #[allow(clippy::cast_possible_truncation)]
    compiler.activate_locals(nvars as u32);
    #[allow(clippy::cast_possible_truncation)]
    compiler.reserve_regs(nvars as u32)?;

    compile_block(compiler, body)?;

    compiler.leave_block();

    // Patch prep JMP to point to TFORLOOP
    compiler.patch_to_here(prep as i32);

    // PUC-Rio: luaK_fixline(fs, line) -- tag TFORLOOP with the iterator
    // expression line, not the `for` keyword line.
    #[allow(clippy::cast_possible_truncation)]
    let endfor = compiler.emit_abc(OpCode::TForLoop, base, 0, nvars as u32, iter_line);
    // JMP back to the beginning of the loop body
    let loop_jmp = compiler.emit_jump(iter_line);
    compiler.patch_jump(loop_jmp, prep + 1);

    let _ = endfor;
    compiler.leave_block();
    compiler.current_line = end_line;
    Ok(())
}

fn compile_func_decl(
    compiler: &mut Compiler,
    name: &super::ast::FuncName,
    body: &super::ast::FuncBody,
    line: u32,
) -> LuaResult<()> {
    let need_self = name.method.is_some();

    // Resolve the function name to a variable target
    let mut var = compiler.resolve_var(&name.parts[0])?;

    // Handle dotted path: a.b.c -> index chain
    for part in &name.parts[1..] {
        compiler.exp2anyreg(&mut var, line)?;
        let k = compiler.string_constant(part.as_bytes())?;
        let mut key = ExprContext::new(ExprKind::K, k as i32);
        compiler.set_indexed(&mut var, &mut key, line)?;
    }

    // Handle method: a.b:c -> index + self
    if let Some(method) = &name.method {
        compiler.exp2anyreg(&mut var, line)?;
        let k = compiler.string_constant(method.as_bytes())?;
        let mut key = ExprContext::new(ExprKind::K, k as i32);
        compiler.set_indexed(&mut var, &mut key, line)?;
    }

    // Compile function body
    let mut func_e = compile_funcbody(compiler, body, need_self, line)?;

    // Store function to the variable
    compiler.storevar(&var, &mut func_e, line)?;
    Ok(())
}

fn compile_local_func(
    compiler: &mut Compiler,
    name: &str,
    body: &super::ast::FuncBody,
    line: u32,
) -> LuaResult<()> {
    // Create local first (function can reference itself)
    compiler.new_local(name)?;
    compiler.activate_locals(1);

    let mut func_e = compile_funcbody(compiler, body, false, line)?;
    // Local is already activated; store into its register
    let reg = u32::from(compiler.fs().num_active_vars) - 1;
    compiler.exp2reg(&mut func_e, reg, line);
    Ok(())
}

fn compile_return(
    compiler: &mut Compiler,
    values: &[super::ast::Expr],
    line: u32,
) -> LuaResult<()> {
    if values.is_empty() {
        compiler.emit_abc(OpCode::Return, 0, 1, 0, line);
    } else if values.len() == 1 {
        let mut e = compile_expr(compiler, &values[0])?;
        // Check for multi-return (call or vararg) BEFORE discharging.
        if e.kind == ExprKind::Call || e.kind == ExprKind::VarArg {
            // Tail call optimization: only for single CALL (not VARARG)
            if e.kind == ExprKind::Call {
                let instr = compiler.get_instruction(e.info as usize);
                if instr.opcode() == OpCode::Call {
                    let tail = Instruction::abc(OpCode::TailCall, instr.a(), instr.b(), 0);
                    compiler.set_instruction(e.info as usize, tail);
                }
            }
            compiler.set_multret(&mut e);
            let first = u32::from(compiler.fs().num_active_vars);
            compiler.emit_abc(OpCode::Return, first, 0, 0, line);
        } else {
            let first = compiler.exp2anyreg(&mut e, line)?;
            compiler.emit_abc(OpCode::Return, first, 2, 0, line);
        }
    } else {
        // Multiple return values
        let base = u32::from(compiler.fs().free_reg);
        for (i, expr) in values.iter().enumerate() {
            let mut e = compile_expr(compiler, expr)?;
            if i == values.len() - 1 {
                // Last expression: check for multi-return
                if e.kind == ExprKind::Call || e.kind == ExprKind::VarArg {
                    compiler.set_multret(&mut e);
                    let first = u32::from(compiler.fs().num_active_vars);
                    // 0 means multi-ret
                    compiler.emit_abc(OpCode::Return, first, 0, 0, line);
                    return Ok(());
                }
            }
            compiler.exp2nextreg(&mut e, line)?;
        }
        let nret = values.len() as u32;
        compiler.emit_abc(OpCode::Return, base, nret + 1, 0, line);
    }
    Ok(())
}

fn compile_expr_stat(
    compiler: &mut Compiler,
    expr: &super::ast::Expr,
    _line: u32,
) -> LuaResult<()> {
    let e = compile_expr(compiler, expr)?;
    if e.kind == ExprKind::Call {
        // Function call as statement: set result count to 0
        let mut instr = compiler.get_instruction(e.info as usize);
        instr.set_c(1); // C=1 means 0 results
        compiler.set_instruction(e.info as usize, instr);
    }
    Ok(())
}

// -- Expression compilation --

/// Compiles an expression list. Returns (count, last_expression).
fn compile_exprlist(
    compiler: &mut Compiler,
    exprs: &[super::ast::Expr],
    line: u32,
) -> LuaResult<(usize, ExprContext)> {
    if exprs.is_empty() {
        return Ok((0, ExprContext::void()));
    }

    // Compile all but the last expression into registers
    for expr in &exprs[..exprs.len() - 1] {
        let mut e = compile_expr(compiler, expr)?;
        compiler.exp2nextreg(&mut e, line)?;
    }

    // Compile the last expression (may be multi-return)
    let last = compile_expr(compiler, &exprs[exprs.len() - 1])?;
    Ok((exprs.len(), last))
}

/// Adjusts assignment: ensures exactly `nvars` values on the stack.
fn adjust_assign(
    compiler: &mut Compiler,
    nvars: usize,
    nexps: usize,
    last: &mut ExprContext,
    line: u32,
) -> LuaResult<()> {
    let extra = nvars as i32 - nexps as i32;
    if last.kind == ExprKind::Call || last.kind == ExprKind::VarArg {
        // Multi-return: adjust to produce needed values.
        // Matches PUC-Rio's `adjust_assign` calling `luaK_setreturns`.
        let is_call = last.kind == ExprKind::Call;
        let needed = extra + 1;
        if needed < 0 {
            // More expressions than variables — set to 1 result
            compiler.set_one_ret(last);
        } else if is_call {
            // CALL: set C = needed + 1
            let mut instr = compiler.get_instruction(last.info as usize);
            instr.set_c((needed + 1) as u32);
            compiler.set_instruction(last.info as usize, instr);
        } else {
            // VARARG: set B (count) and A (target register).
            // Matches PUC-Rio's luaK_setreturns for VVARARG.
            let mut instr = compiler.get_instruction(last.info as usize);
            instr.set_b((needed + 1) as u32);
            instr.set_a(u32::from(compiler.fs().free_reg));
            compiler.set_instruction(last.info as usize, instr);
            compiler.reserve_regs(1)?;
            last.kind = ExprKind::Relocable;
        }
        // For CALL with extra results, reserve additional registers.
        // VARARG already reserved its one register above.
        // Matches PUC-Rio: `if (e->k == VCALL && extra > 1) luaK_reserveregs`
        if is_call && needed > 1 {
            #[allow(clippy::cast_possible_truncation)]
            compiler.reserve_regs((needed - 1) as u32)?;
        }
    } else {
        if last.kind != ExprKind::Void {
            compiler.exp2nextreg(last, line)?;
        }
        if extra > 0 {
            // Pad with nils
            let reg = u32::from(compiler.fs().free_reg);
            #[allow(clippy::cast_possible_truncation)]
            compiler.reserve_regs(extra as u32)?;
            // LOADNIL with coalescing (PUC-Rio luaK_nil)
            #[allow(clippy::cast_possible_truncation)]
            compiler.emit_nil(reg, extra as u32, line);
        }
    }
    Ok(())
}

/// Compiles a function body into a Proto and returns a Closure expression.
fn compile_funcbody(
    compiler: &mut Compiler,
    body: &super::ast::FuncBody,
    need_self: bool,
    line: u32,
) -> LuaResult<ExprContext> {
    compiler.enter_function(&compiler.source_name.clone());
    compiler.fs_mut().proto.line_defined = line;

    // Add 'self' parameter if needed
    if need_self {
        compiler.new_local("self")?;
        compiler.activate_locals(1);
    }

    // Add parameters
    for param in &body.params {
        compiler.new_local(param)?;
    }
    #[allow(clippy::cast_possible_truncation)]
    {
        compiler.activate_locals(body.params.len() as u32);
    }

    // Set function metadata
    #[allow(clippy::cast_possible_truncation)]
    {
        let num_params = body.params.len() as u8 + u8::from(need_self);
        compiler.fs_mut().proto.num_params = num_params;
    }
    if body.has_varargs {
        // LUA_COMPAT_VARARG: add implicit 'arg' local and set all flags.
        // NEEDSARG is cleared later if the body actually uses '...'.
        compiler.fs_mut().proto.is_vararg = VARARG_HASARG | VARARG_ISVARARG | VARARG_NEEDSARG;
        compiler.new_local("arg")?;
        compiler.activate_locals(1);
    }

    // Reserve registers for parameters (+ 'arg' if vararg).
    // PUC-Rio: numparams excludes the 'arg' parameter.
    let nactvar = u32::from(compiler.fs().num_active_vars);
    compiler.reserve_regs(nactvar)?;

    // Compile body
    compile_block(compiler, &body.body)?;

    // PUC-Rio: f->lastlinedefined = ls->linenumber (line of `end` keyword)
    compiler.fs_mut().proto.last_line_defined = body.end_line;
    // Set current_line to `end` so leave_function's implicit RETURN maps to it.
    // PUC-Rio: close_func uses fs->ls->lastline for luaK_ret.
    compiler.current_line = body.end_line;

    // Save child's upvalue descriptors before leave_function discards them.
    // PUC-Rio accesses func->upvalues in pushclosure while the child
    // FuncState is still alive; we save a copy because leave_function
    // pops the FuncState entirely.
    let child_upvalues = compiler.fs().upvalues.clone();

    // Leave function — emits final RETURN and pops FuncState
    let proto = compiler.leave_function();

    // Add proto as a child of the current function
    let parent_fs = compiler.fs_mut();
    let proto_idx = parent_fs.proto.protos.len();
    parent_fs.proto.protos.push(ProtoRef::new(proto));

    // Emit CLOSURE instruction
    #[allow(clippy::cast_possible_truncation)]
    let pc = compiler.emit_abx(OpCode::Closure, 0, proto_idx as u32, line);

    // Emit pseudo-instructions for upvalue capture.
    // Each upvalue has a MOVE (local from current frame) or
    // GETUPVAL (inherited from parent's upvalues). Maps to PUC-Rio's
    // pushclosure loop.
    for uv in &child_upvalues {
        let op = if uv.in_stack {
            OpCode::Move
        } else {
            OpCode::GetUpval
        };
        compiler.emit_abc(op, 0, u32::from(uv.index), 0, line);
    }

    let e = ExprContext::new(ExprKind::Relocable, pc as i32);
    Ok(e)
}

/// Compiles an expression, returning its ExprContext.
fn compile_expr(compiler: &mut Compiler, expr: &super::ast::Expr) -> LuaResult<ExprContext> {
    use super::ast::Expr;
    let line = expr.span().line;
    compiler.current_line = line;

    match expr {
        Expr::Nil(_) => Ok(ExprContext::new(ExprKind::Nil, 0)),
        Expr::True(_) => Ok(ExprContext::new(ExprKind::True, 0)),
        Expr::False(_) => Ok(ExprContext::new(ExprKind::False, 0)),
        Expr::Number(n, _) => Ok(ExprContext::number(*n)),

        Expr::Str(s, _) => {
            let k = compiler.string_constant(s)?;
            Ok(ExprContext::new(ExprKind::K, k as i32))
        }

        Expr::VarArg(_) => {
            // LUA_COMPAT_VARARG: using '...' means 'arg' table is not needed.
            compiler.fs_mut().proto.is_vararg &= !VARARG_NEEDSARG;
            let pc = compiler.emit_abc(OpCode::VarArg, 0, 1, 0, line);
            Ok(ExprContext::new(ExprKind::VarArg, pc as i32))
        }

        Expr::Name(name, _) => compiler.resolve_var(name),

        Expr::BinOp {
            op, left, right, ..
        } => {
            let mut e1 = compile_expr(compiler, left)?;
            compiler.infix(*op, &mut e1, line)?;
            let mut e2 = compile_expr(compiler, right)?;
            compiler.postfix(*op, &mut e1, &mut e2, line)?;
            Ok(e1)
        }

        Expr::UnOp { op, operand, .. } => {
            let mut e = compile_expr(compiler, operand)?;
            compiler.prefix(*op, &mut e, line)?;
            Ok(e)
        }

        Expr::Index { table, key, .. } => {
            let mut t = compile_expr(compiler, table)?;
            compiler.exp2anyreg(&mut t, line)?;
            let mut k = compile_expr(compiler, key)?;
            compiler.set_indexed(&mut t, &mut k, line)?;
            Ok(t)
        }

        Expr::Field { table, field, .. } => {
            let mut t = compile_expr(compiler, table)?;
            compiler.exp2anyreg(&mut t, line)?;
            let k_idx = compiler.string_constant(field.as_bytes())?;
            let mut k = ExprContext::new(ExprKind::K, k_idx as i32);
            compiler.set_indexed(&mut t, &mut k, line)?;
            Ok(t)
        }

        Expr::MethodCall {
            table,
            method,
            args,
            ..
        } => {
            let mut obj = compile_expr(compiler, table)?;
            compiler.exp2anyreg(&mut obj, line)?;
            let k = compiler.string_constant(method.as_bytes())?;
            let mut key = ExprContext::new(ExprKind::K, k as i32);
            compiler.code_self(&mut obj, &mut key, line)?;
            compile_funcargs(compiler, &mut obj, args, line)?;
            Ok(obj)
        }

        Expr::Call { func, args, .. } => {
            let mut f = compile_expr(compiler, func)?;
            compiler.exp2nextreg(&mut f, line)?;
            compile_funcargs(compiler, &mut f, args, line)?;
            Ok(f)
        }

        Expr::FuncDef { body, .. } => compile_funcbody(compiler, body, false, line),

        Expr::TableCtor { fields, .. } => compile_table_ctor(compiler, fields, line),

        Expr::Paren(inner, _) => {
            let mut e = compile_expr(compiler, inner)?;
            // Parenthesized expressions force calls and varargs to return
            // exactly one value. Maps to PUC-Rio's luaK_dischargevars in
            // prefixexp which calls luaK_setoneret for VCALL/VVARARG.
            compiler.discharge_vars(&mut e, line);
            Ok(e)
        }
    }
}

/// Compiles function arguments and emits CALL.
fn compile_funcargs(
    compiler: &mut Compiler,
    func: &mut ExprContext,
    args: &[super::ast::Expr],
    line: u32,
) -> LuaResult<()> {
    let base = func.info as u32;

    if args.is_empty() {
        // No arguments
    } else {
        for (i, arg) in args.iter().enumerate() {
            let mut e = compile_expr(compiler, arg)?;
            if i == args.len() - 1 {
                // Last argument: check for multi-return
                if e.kind == ExprKind::Call || e.kind == ExprKind::VarArg {
                    compiler.set_multret(&mut e);
                    // nparams = LUA_MULTRET
                    let pc = compiler.emit_abc(OpCode::Call, base, 0, 2, line);
                    func.info = pc as i32;
                    func.kind = ExprKind::Call;
                    compiler.fs_mut().free_reg = (base + 1) as u8;
                    return Ok(());
                }
            }
            compiler.exp2nextreg(&mut e, line)?;
        }
    }

    let nparams = u32::from(compiler.fs().free_reg) - (base + 1);
    let pc = compiler.emit_abc(OpCode::Call, base, nparams + 1, 2, line);
    func.info = pc as i32;
    func.kind = ExprKind::Call;
    compiler.fs_mut().free_reg = (base + 1) as u8;
    Ok(())
}

/// Compiles a table constructor.
fn compile_table_ctor(
    compiler: &mut Compiler,
    fields: &[super::ast::TableField],
    line: u32,
) -> LuaResult<ExprContext> {
    use super::ast::TableField;

    let pc = compiler.emit_abc(OpCode::NewTable, 0, 0, 0, line);
    let mut t = ExprContext::new(ExprKind::Relocable, pc as i32);
    compiler.exp2nextreg(&mut t, line)?;
    let table_reg = t.info as u32;

    let mut na: u32 = 0; // array fields count
    let mut nh: u32 = 0; // hash fields count
    let mut tostore: u32 = 0; // pending array fields

    // Find the index of the last ValueField for lastlistfield handling.
    // PUC-Rio's lastlistfield: if the last list item is a multi-return
    // expression (call or vararg), expand it into all return values.
    let last_value_idx = fields
        .iter()
        .rposition(|f| matches!(f, TableField::ValueField { .. }));

    for (i, field) in fields.iter().enumerate() {
        match field {
            TableField::ValueField { value, .. } => {
                // Array/list field
                na += 1;
                tostore += 1;

                let is_last = last_value_idx == Some(i);
                let mut val_e = compile_expr(compiler, value)?;
                if is_last && is_multret_expr(value) {
                    // Last value field with multi-return: expand all results.
                    // Matches PUC-Rio's lastlistfield() in lparser.c.
                    compiler.set_multret(&mut val_e);
                    compiler.code_setlist(table_reg, na, 0, line); // B=0 = MULTRET
                    na -= 1; // don't count last (unknown count)
                    tostore = 0;
                } else {
                    compiler.exp2nextreg(&mut val_e, line)?;
                    if tostore >= LFIELDS_PER_FLUSH {
                        compiler.code_setlist(table_reg, na, tostore, line);
                        tostore = 0;
                    }
                }
            }
            TableField::NameField { name, value, .. } => {
                // Hash field: name = value
                // PUC-Rio recfield(): uses exp2RK for the key to handle
                // constant pool indices > MAXINDEXRK (falls back to register).
                nh += 1;
                let k = compiler.string_constant(name.as_bytes())?;
                let mut key_e = ExprContext::new(ExprKind::K, k as i32);
                let key_rk = compiler.exp2rk(&mut key_e, line)?;
                let mut val_e = compile_expr(compiler, value)?;
                let val_rk = compiler.exp2rk(&mut val_e, line)?;
                compiler.emit_abc(OpCode::SetTable, table_reg, key_rk, val_rk, line);
                compiler.free_expr(&val_e);
                compiler.free_expr(&key_e);
            }
            TableField::IndexField { key, value, .. } => {
                // Hash field: [key] = value
                nh += 1;
                let mut key_e = compile_expr(compiler, key)?;
                let key_rk = compiler.exp2rk(&mut key_e, line)?;
                let mut val_e = compile_expr(compiler, value)?;
                let val_rk = compiler.exp2rk(&mut val_e, line)?;
                compiler.emit_abc(OpCode::SetTable, table_reg, key_rk, val_rk, line);
                compiler.free_expr(&val_e);
                compiler.free_expr(&key_e);
            }
        }
    }

    // Flush remaining array fields
    if tostore > 0 {
        compiler.code_setlist(table_reg, na, tostore, line);
    }

    // Backpatch NEWTABLE with actual sizes
    // Use luaO_int2fb encoding (float byte: eeeeexxx)
    let mut instr = compiler.get_instruction(pc);
    instr.set_b(int2fb(na));
    instr.set_c(int2fb(nh));
    compiler.set_instruction(pc, instr);

    Ok(t)
}

/// Returns true if an expression can produce multiple return values
/// (function call or vararg). Matches PUC-Rio's `hasmultret` macro.
fn is_multret_expr(expr: &super::ast::Expr) -> bool {
    matches!(
        expr,
        super::ast::Expr::Call { .. }
            | super::ast::Expr::MethodCall { .. }
            | super::ast::Expr::VarArg(..)
    )
}

/// Converts an integer to PUC-Rio's "float byte" format (eeeeexxx).
/// If x < 8: result = x. Otherwise: result = ((e+1) << 3) | (x >> (e-1) - 8).
pub(crate) fn int2fb(mut x: u32) -> u32 {
    if x < 8 {
        return x;
    }
    let mut e = 0u32;
    while x >= 16 {
        x = (x + 1) >> 1;
        e += 1;
    }
    ((e + 1) << 3) | (x - 8)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::approx_constant,
    clippy::items_after_statements,
    clippy::needless_collect,
    clippy::bool_comparison,
    clippy::useless_vec,
    clippy::needless_bool_assign,
    clippy::unnecessary_operation
)]
mod tests {
    use super::*;

    // -- Compiler construction --

    #[test]
    fn empty_program_compiles() {
        let proto = compile(b"", "test").unwrap();
        assert_eq!(proto.num_params, 0);
        assert_eq!(proto.is_vararg, 2);
        // Should have at least the final RETURN
        assert!(!proto.code.is_empty());
    }

    #[test]
    fn return_no_values() {
        let proto = compile(b"return", "test").unwrap();
        // Should have RETURN 0,1 (from the return) + RETURN 0,1 (from finish_main)
        assert!(!proto.code.is_empty());
        let instr = Instruction::from_raw(proto.code[0]);
        assert_eq!(instr.opcode(), OpCode::Return);
    }

    // -- Constant pool --

    #[test]
    fn number_constant_dedup() {
        let mut compiler = Compiler::new("test");
        let k1 = compiler.number_constant(42.0).unwrap();
        let k2 = compiler.number_constant(42.0).unwrap();
        let k3 = compiler.number_constant(99.0).unwrap();
        assert_eq!(k1, k2); // Same constant, deduped
        assert_ne!(k1, k3); // Different constant
    }

    #[test]
    fn constant_bool_dedup() {
        let mut compiler = Compiler::new("test");
        let k1 = compiler.add_constant(Val::Bool(true)).unwrap();
        let k2 = compiler.add_constant(Val::Bool(true)).unwrap();
        let k3 = compiler.add_constant(Val::Bool(false)).unwrap();
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn constant_nan_not_deduped() {
        let mut compiler = Compiler::new("test");
        let k1 = compiler.add_constant(Val::Num(f64::NAN)).unwrap();
        let k2 = compiler.add_constant(Val::Num(f64::NAN)).unwrap();
        // NaN != NaN, but we use to_bits for dedup, so same bit pattern dedupes
        assert_eq!(k1, k2);
    }

    // -- Register allocation --

    #[test]
    fn alloc_and_free_reg() {
        let mut compiler = Compiler::new("test");
        let r1 = compiler.alloc_reg().unwrap();
        assert_eq!(r1, 0);
        let r2 = compiler.alloc_reg().unwrap();
        assert_eq!(r2, 1);
        compiler.free_reg(r2);
        assert_eq!(compiler.fs().free_reg, 1);
        let r3 = compiler.alloc_reg().unwrap();
        assert_eq!(r3, 1); // Reused
    }

    #[test]
    fn reserve_regs() {
        let mut compiler = Compiler::new("test");
        compiler.reserve_regs(3).unwrap();
        assert_eq!(compiler.fs().free_reg, 3);
    }

    #[test]
    fn stack_overflow_error() {
        let mut compiler = Compiler::new("test");
        compiler.fs_mut().free_reg = 249;
        assert!(compiler.check_stack(2).is_err());
    }

    // -- Variable resolution --

    #[test]
    fn resolve_global() {
        let mut compiler = Compiler::new("test");
        let e = compiler.resolve_var("x").unwrap();
        assert_eq!(e.kind, ExprKind::Global);
    }

    #[test]
    fn resolve_local() {
        let mut compiler = Compiler::new("test");
        compiler.new_local("x").unwrap();
        compiler.activate_locals(1);
        compiler.fs_mut().free_reg = 1; // Local occupies register 0

        let e = compiler.resolve_var("x").unwrap();
        assert_eq!(e.kind, ExprKind::Local);
        assert_eq!(e.info, 0); // Register 0
    }

    // -- Block management --

    #[test]
    fn enter_leave_block() {
        let mut compiler = Compiler::new("test");
        compiler.enter_block(false);
        assert_eq!(compiler.fs().blocks.len(), 1);
        compiler.leave_block();
        assert_eq!(compiler.fs().blocks.len(), 0);
    }

    #[test]
    fn locals_removed_on_block_exit() {
        let mut compiler = Compiler::new("test");
        compiler.enter_block(false);

        compiler.new_local("x").unwrap();
        compiler.activate_locals(1);
        compiler.fs_mut().free_reg = 1;
        assert_eq!(compiler.fs().num_active_vars, 1);

        compiler.leave_block();
        assert_eq!(compiler.fs().num_active_vars, 0);
    }

    // -- Instruction emission --

    #[test]
    fn emit_instruction() {
        let mut compiler = Compiler::new("test");
        let pc = compiler.emit_abc(OpCode::Move, 0, 1, 0, 1);
        assert_eq!(pc, 0);
        let instr = compiler.get_instruction(0);
        assert_eq!(instr.opcode(), OpCode::Move);
        assert_eq!(instr.a(), 0);
        assert_eq!(instr.b(), 1);
    }

    #[test]
    fn emit_records_line_info() {
        let mut compiler = Compiler::new("test");
        compiler.emit_abc(OpCode::Move, 0, 1, 0, 5);
        compiler.emit_abc(OpCode::LoadK, 1, 0, 0, 10);
        assert_eq!(compiler.fs().proto.line_info[0], 5);
        assert_eq!(compiler.fs().proto.line_info[1], 10);
    }

    // -- Expression discharge --

    #[test]
    fn discharge_nil() {
        let mut compiler = Compiler::new("test");
        // Emit a dummy instruction so we're not at pc==0
        // (at pc==0, LOADNIL is elided because registers start as nil).
        compiler.emit_abc(OpCode::Return, 0, 1, 0, 1);
        let mut e = ExprContext::new(ExprKind::Nil, 0);
        let reg = compiler.alloc_reg().unwrap();
        compiler.discharge2reg(&mut e, reg, 1);
        let instr = compiler.get_instruction(1);
        assert_eq!(instr.opcode(), OpCode::LoadNil);
    }

    #[test]
    fn discharge_true() {
        let mut compiler = Compiler::new("test");
        let mut e = ExprContext::new(ExprKind::True, 0);
        let reg = compiler.alloc_reg().unwrap();
        compiler.discharge2reg(&mut e, reg, 1);
        let instr = compiler.get_instruction(0);
        assert_eq!(instr.opcode(), OpCode::LoadBool);
        assert_eq!(instr.b(), 1);
    }

    #[test]
    fn discharge_number() {
        let mut compiler = Compiler::new("test");
        let mut e = ExprContext::number(42.0);
        let reg = compiler.alloc_reg().unwrap();
        compiler.discharge2reg(&mut e, reg, 1);
        let instr = compiler.get_instruction(0);
        assert_eq!(instr.opcode(), OpCode::LoadK);
    }

    #[test]
    fn discharge_local_to_different_reg() {
        let mut compiler = Compiler::new("test");
        // Local in register 0
        compiler.new_local("x").unwrap();
        compiler.activate_locals(1);
        compiler.fs_mut().free_reg = 1;

        let mut e = ExprContext::new(ExprKind::Local, 0);
        compiler.discharge_vars(&mut e, 1);
        assert_eq!(e.kind, ExprKind::NonReloc);

        // Discharge to register 1 (different from local's register 0)
        let reg = compiler.alloc_reg().unwrap();
        compiler.discharge2reg(&mut e, reg, 1);
        let instr = compiler.get_instruction(0);
        assert_eq!(instr.opcode(), OpCode::Move);
        assert_eq!(instr.a(), 1);
        assert_eq!(instr.b(), 0);
    }

    // -- Compile integration --

    #[test]
    fn compile_return_number() {
        let proto = compile(b"return 42", "test").unwrap();
        // Should have: LOADK, RETURN (from return stmt), RETURN (from finish)
        assert!(proto.code.len() >= 2);
        // Check constant pool has 42.0
        assert!(
            proto
                .constants
                .iter()
                .any(|v| matches!(v, Val::Num(n) if *n == 42.0))
        );
    }

    #[test]
    fn compile_return_nil() {
        let proto = compile(b"return nil", "test").unwrap();
        // At pc==0, LOADNIL is elided (registers start as nil).
        // First instruction should be RETURN.
        let instr = Instruction::from_raw(proto.code[0]);
        assert_eq!(instr.opcode(), OpCode::Return);
    }

    #[test]
    fn compile_return_bool() {
        let proto = compile(b"return true", "test").unwrap();
        let instr = Instruction::from_raw(proto.code[0]);
        assert_eq!(instr.opcode(), OpCode::LoadBool);
        assert_eq!(instr.b(), 1); // true
    }

    #[test]
    fn compile_return_multiple() {
        let proto = compile(b"return 1, 2, 3", "test").unwrap();
        // Should have 3 LOADKs + RETURN + final RETURN
        let mut loadk_count = 0;
        for &code in &proto.code {
            if Instruction::from_raw(code).opcode() == OpCode::LoadK {
                loadk_count += 1;
            }
        }
        assert_eq!(loadk_count, 3);
    }

    // -- Helper --

    /// Returns the sequence of opcodes in a compiled proto.
    fn opcodes(proto: &Proto) -> Vec<OpCode> {
        proto
            .code
            .iter()
            .map(|&raw| Instruction::from_raw(raw).opcode())
            .collect()
    }

    // -- Statement compilation --

    #[test]
    fn compile_local_decl() {
        let proto = compile(b"local x = 42", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::LoadK));
    }

    #[test]
    fn compile_local_nil_init() {
        let proto = compile(b"local x, y", "test").unwrap();
        let ops = opcodes(&proto);
        // At function start, locals are already nil so LOADNIL is elided.
        // Only RETURN should remain.
        assert!(!ops.contains(&OpCode::LoadNil));
        assert!(ops.contains(&OpCode::Return));
    }

    #[test]
    fn compile_global_assign() {
        let proto = compile(b"x = 1", "test").unwrap();
        let ops = opcodes(&proto);
        // Assignment to a global: SETGLOBAL
        assert!(ops.contains(&OpCode::SetGlobal));
    }

    #[test]
    fn compile_local_assign() {
        let proto = compile(b"local x; x = 1", "test").unwrap();
        let ops = opcodes(&proto);
        // Assignment to a local: LOADK into the local's register (MOVE or direct LOADK)
        assert!(ops.contains(&OpCode::LoadK));
    }

    #[test]
    fn compile_do_block() {
        // 'do' block creates a scope; locals inside are freed
        let proto = compile(b"do local x = 1 end; return", "test").unwrap();
        assert!(proto.code.len() >= 2);
    }

    // -- Control flow --

    #[test]
    fn compile_while_loop() {
        let proto = compile(b"local x = true; while x do x = false end", "test").unwrap();
        let ops = opcodes(&proto);
        // Should contain a JMP (back edge of while loop)
        assert!(ops.contains(&OpCode::Jmp));
    }

    #[test]
    fn compile_repeat_until() {
        let proto = compile(b"local x = 0; repeat x = x + 1 until x", "test").unwrap();
        let ops = opcodes(&proto);
        // Repeat-until has a conditional jump and arithmetic
        assert!(ops.contains(&OpCode::Add));
    }

    #[test]
    fn compile_if_then() {
        let proto = compile(b"local x = true; if x then return 1 end", "test").unwrap();
        let ops = opcodes(&proto);
        // If generates TEST + JMP
        assert!(
            ops.contains(&OpCode::Test) || ops.contains(&OpCode::Jmp),
            "if-then should generate control flow"
        );
    }

    #[test]
    fn compile_if_else() {
        let proto = compile(
            b"local x = true; if x then return 1 else return 2 end",
            "test",
        )
        .unwrap();
        let ops = opcodes(&proto);
        // Should have at least 2 JMPs (condition + else skip)
        let jmp_count = ops.iter().filter(|&&op| op == OpCode::Jmp).count();
        assert!(jmp_count >= 1, "if-else needs at least one JMP");
    }

    #[test]
    fn compile_numeric_for() {
        let proto = compile(b"for i = 1, 10 do end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::ForPrep));
        assert!(ops.contains(&OpCode::ForLoop));
    }

    #[test]
    fn compile_numeric_for_with_step() {
        let proto = compile(b"for i = 1, 10, 2 do end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::ForPrep));
        assert!(ops.contains(&OpCode::ForLoop));
        // Step value 2 should be in constants
        assert!(
            proto
                .constants
                .iter()
                .any(|v| matches!(v, Val::Num(n) if n == &2.0))
        );
    }

    #[test]
    fn compile_generic_for() {
        let proto = compile(b"for k, v in next, t do end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::TForLoop));
        assert!(ops.contains(&OpCode::Jmp));
    }

    #[test]
    fn compile_break() {
        let proto = compile(b"while true do break end", "test").unwrap();
        let ops = opcodes(&proto);
        // Break emits a JMP that gets patched to loop exit
        assert!(ops.contains(&OpCode::Jmp));
    }

    // -- Expression compilation --

    #[test]
    fn compile_arithmetic() {
        // 1 + 2 is constant-folded to 3, so use a local to prevent folding.
        let proto = compile(b"local a; return a + 2", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Add));
    }

    #[test]
    fn compile_comparison() {
        let proto = compile(b"return 1 < 2", "test").unwrap();
        let ops = opcodes(&proto);
        // Comparisons emit EQ/LT/LE + JMP + LoadBool pair
        assert!(ops.contains(&OpCode::Lt));
    }

    #[test]
    fn compile_concat() {
        let proto = compile(b"return 'a' .. 'b'", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Concat));
    }

    #[test]
    fn compile_unary_neg() {
        let proto = compile(b"local x = 1; return -x", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Unm));
    }

    #[test]
    fn compile_unary_not() {
        let proto = compile(b"return not true", "test").unwrap();
        let ops = opcodes(&proto);
        // 'not true' is constant-folded to false at compile time
        assert!(ops.contains(&OpCode::LoadBool));
    }

    #[test]
    fn compile_unary_len() {
        let proto = compile(b"local x = {}; return #x", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Len));
    }

    #[test]
    fn compile_string_constant() {
        // String constants are stored as Val::Nil placeholders until GC integration.
        // Verify the code compiles and emits a LOADK to reference the constant.
        let proto = compile(b"return 'hello'", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::LoadK));
        // Constant pool should have at least one entry (the string placeholder)
        assert!(!proto.constants.is_empty());
    }

    #[test]
    fn compile_and_short_circuit() {
        let proto = compile(b"local a, b; return a and b", "test").unwrap();
        let ops = opcodes(&proto);
        // 'and' uses TEST + JMP (short-circuit)
        assert!(
            ops.contains(&OpCode::Test) || ops.contains(&OpCode::TestSet),
            "and should use TEST/TESTSET"
        );
    }

    #[test]
    fn compile_or_short_circuit() {
        let proto = compile(b"local a, b; return a or b", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(
            ops.contains(&OpCode::Test) || ops.contains(&OpCode::TestSet),
            "or should use TEST/TESTSET"
        );
    }

    // -- Function compilation --

    #[test]
    fn compile_function_call() {
        let proto = compile(b"print(42)", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::GetGlobal));
        assert!(ops.contains(&OpCode::Call));
    }

    #[test]
    fn compile_method_call() {
        let proto = compile(b"local t = {}; t:foo(1)", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::OpSelf));
        assert!(ops.contains(&OpCode::Call));
    }

    #[test]
    fn compile_function_def() {
        let proto = compile(b"local f = function(x) return x end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Closure));
        // Should have a child proto
        assert_eq!(proto.protos.len(), 1);
        let child = &proto.protos[0];
        assert_eq!(child.num_params, 1);
    }

    #[test]
    fn compile_local_function() {
        let proto = compile(b"local function f(a, b) return a + b end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Closure));
        assert_eq!(proto.protos.len(), 1);
        let child = &proto.protos[0];
        assert_eq!(child.num_params, 2);
    }

    #[test]
    fn compile_named_function() {
        let proto = compile(b"function f(x) return x end", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Closure));
        assert!(ops.contains(&OpCode::SetGlobal));
    }

    #[test]
    fn compile_vararg_function() {
        let proto = compile(b"local f = function(...) return ... end", "test").unwrap();
        let child = &proto.protos[0];
        assert!(child.is_vararg & 2 != 0); // VARARG_ISVARARG
    }

    // -- Table construction --

    #[test]
    fn compile_empty_table() {
        let proto = compile(b"return {}", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::NewTable));
    }

    #[test]
    fn compile_array_table() {
        let proto = compile(b"return {1, 2, 3}", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::NewTable));
        assert!(ops.contains(&OpCode::SetList));
    }

    #[test]
    fn compile_hash_table() {
        let proto = compile(b"return {x = 1, y = 2}", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::NewTable));
        assert!(ops.contains(&OpCode::SetTable));
    }

    #[test]
    fn compile_index_table() {
        let proto = compile(b"return {[1] = 'a'}", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::NewTable));
        assert!(ops.contains(&OpCode::SetTable));
    }

    // -- Table access --

    #[test]
    fn compile_table_field_access() {
        let proto = compile(b"local t = {}; return t.x", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::GetTable));
    }

    #[test]
    fn compile_table_index_access() {
        let proto = compile(b"local t = {}; return t[1]", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::GetTable));
    }

    // -- Tail calls --

    #[test]
    fn compile_tail_call() {
        let proto = compile(b"local function f(x) return f(x) end", "test").unwrap();
        let child = &proto.protos[0];
        let child_ops: Vec<OpCode> = child
            .code
            .iter()
            .map(|&raw| Instruction::from_raw(raw).opcode())
            .collect();
        assert!(
            child_ops.contains(&OpCode::TailCall),
            "recursive return should use TAILCALL"
        );
    }

    // -- Line info --

    #[test]
    fn compile_line_info() {
        let proto = compile(b"return 42", "test").unwrap();
        assert_eq!(proto.code.len(), proto.line_info.len());
    }

    // -- Edge cases --

    #[test]
    fn compile_return_string() {
        let proto = compile(b"return 'hello'", "test").unwrap();
        let instr = Instruction::from_raw(proto.code[0]);
        assert_eq!(instr.opcode(), OpCode::LoadK);
    }

    #[test]
    fn compile_expr_stat_call() {
        // Expression statement with function call should discard results
        let proto = compile(b"print(1)", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::Call));
        // The CALL should have C=1 (0 results)
        for &code in &proto.code {
            let instr = Instruction::from_raw(code);
            if instr.opcode() == OpCode::Call {
                assert_eq!(
                    instr.c(),
                    1,
                    "expression statement call should have C=1 (0 results)"
                );
                break;
            }
        }
    }

    #[test]
    fn compile_single_global_assign() {
        // Single global assignment
        let proto = compile(b"x = 42", "test").unwrap();
        let ops = opcodes(&proto);
        assert!(ops.contains(&OpCode::SetGlobal));
    }

    #[test]
    fn compile_nested_function() {
        let proto = compile(
            b"local function f() local function g() return 1 end return g end",
            "test",
        )
        .unwrap();
        assert_eq!(proto.protos.len(), 1);
        let f = &proto.protos[0];
        assert_eq!(f.protos.len(), 1); // g is nested inside f
    }

    // -- int2fb --

    #[test]
    fn int2fb_small_values() {
        assert_eq!(int2fb(0), 0);
        assert_eq!(int2fb(1), 1);
        assert_eq!(int2fb(7), 7);
    }

    #[test]
    fn int2fb_exact_powers() {
        // Values >= 8 get encoded in float-byte format
        let encoded = int2fb(8);
        assert!(encoded >= 8);
    }

    #[test]
    fn compile_syntax_error() {
        let result = compile(b"if", "test");
        assert!(result.is_err());
    }
}
