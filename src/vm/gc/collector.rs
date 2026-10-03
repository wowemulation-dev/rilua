//! Incremental mark-sweep garbage collector.
//!
//! Implements PUC-Rio's 5-state incremental GC: a state machine that
//! performs bounded work per step, allowing collection to be interleaved
//! with VM execution.
//!
//! ## State Machine
//!
//! `Pause -> Propagate -> SweepString -> Sweep -> Finalize -> Pause`
//!
//! Each `gc_singlestep()` call advances one unit of work in the current phase.
//! `gc_step()` runs multiple singlesteps up to a budget. `full_gc()` drives
//! the state machine to completion.
//!
//! ## Algorithm
//!
//! 1. **Mark roots** (Pause -> Propagate): global table, registry, type
//!    metatables, tm_names, main thread stack, call stack.
//! 2. **Propagate** (incremental): pop one gray object, traverse its children,
//!    mark it black. Tables, closures, and threads are traversable; strings
//!    and userdata are marked directly.
//! 3. **Atomic** (indivisible): re-traverse grayagain, separate finalizable
//!    userdata, flip whites, clear weak tables.
//! 4. **SweepString** (incremental): sweep string arena in batches.
//! 5. **Sweep** (incremental): sweep non-string arenas in batches.
//! 6. **Finalize** (incremental): run `__gc` metamethods on dead userdata.
//!
//! ## Memory Accounting
//!
//! Allocation methods track `total_bytes` and `gc_debt`. When debt
//! exceeds 0, the VM triggers an incremental step. After each cycle,
//! the threshold is set to `(estimate / 100) * gc_pause`.
//!
//! ## Write Barriers
//!
//! During the Propagate phase, mutations must maintain the tri-color
//! invariant. Tables use backward barriers (parent demoted to gray);
//! upvalues use forward barriers (child marked).
//!
//! Reference: `lgc.c`, `lgc.h` in PUC-Rio Lua 5.1.1.

use super::Color;
use super::arena::GcRef;

use crate::vm::closure::{Closure, Upvalue, UpvalueState};
use crate::vm::proto::Proto;
use crate::vm::state::{Gc, LuaState, LuaThread};
use crate::vm::string::LuaString;
use crate::vm::table::Table;
use crate::vm::value::{Userdata, Val};
use crate::{LuaError, LuaResult};

// ---------------------------------------------------------------------------
// GrayItem: typed gray-list entry
// ---------------------------------------------------------------------------

/// A GC-managed object that needs traversal (has internal references).
///
/// Only tables, closures, and threads have complex internal structure
/// that requires gray-list traversal. Strings are marked directly to
/// black. Userdata marks its metatable/env immediately.
///
/// `MainThread` is a special variant representing the main `LuaState`
/// thread which is not stored in the thread arena. It must be traversed
/// by `LuaState::gc_singlestep` (which owns the stack), not by
/// `Gc::propagate_one`.
#[derive(Clone, Copy)]
pub enum GrayItem {
    Table(GcRef<Table>),
    Closure(GcRef<Closure>),
    Thread(GcRef<LuaThread>),
    MainThread,
}

/// Result of `Gc::propagate_one()`.
///
/// `Done(cost)` means one gray object was traversed with the given cost.
/// `NeedMainThread` means the next item is the main thread, which must be
/// traversed by `LuaState` (since `Gc` doesn't own the main thread's stack).
/// `Empty` means the gray list is empty (propagation phase is complete).
pub enum PropagateResult {
    Done(usize),
    NeedMainThread,
    Empty,
}

// ---------------------------------------------------------------------------
// GcPhase: incremental GC state machine
// ---------------------------------------------------------------------------

/// Phase of the incremental GC cycle.
///
/// The collector advances through these phases in order:
/// `Pause -> Propagate -> SweepString -> Sweep -> Finalize -> Pause`.
///
/// Each call to `gc_step()` does bounded work in the current phase,
/// then returns. This matches PUC-Rio's `singlestep()` state machine
/// (`GCSpause`, `GCSpropagate`, `GCSsweepstring`, `GCSsweep`, `GCSfinalize`).
///
/// Reference: `lgc.h:17-21` in PUC-Rio Lua 5.1.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcPhase {
    /// Idle, waiting for threshold to be exceeded.
    Pause,
    /// Marking gray objects (traversing references).
    Propagate,
    /// Sweeping the string arena incrementally.
    SweepString,
    /// Sweeping non-string arenas (tables, closures, upvalues, userdata, threads).
    Sweep,
    /// Running `__gc` metamethods on dead userdata.
    Finalize,
}

// ---------------------------------------------------------------------------
// SweepCursor: tracks incremental sweep position
// ---------------------------------------------------------------------------

/// Tracks position within the incremental sweep phase.
///
/// During `SweepString`, only `slot_position` is used (for the string arena).
/// During `Sweep`, `arena_index` selects which arena and `slot_position`
/// tracks the position within it.
///
/// Arena indices: 0=tables, 1=closures, 2=upvalues, 3=userdata, 4=threads.
#[derive(Clone, Copy, Debug)]
pub struct SweepCursor {
    /// Which non-string arena is being swept (0..5).
    pub arena_index: u8,
    /// Current slot position within the arena.
    pub slot_position: u32,
}

// ---------------------------------------------------------------------------
// GC pacing constants
// ---------------------------------------------------------------------------

/// Default GC pause percentage (200 = collect when memory doubles).
pub const DEFAULT_GC_PAUSE: u32 = 200;

/// Default GC step multiplier (200 = collector runs at 2x alloc speed).
pub const DEFAULT_GC_STEPMUL: u32 = 200;

/// Initial GC threshold before the first collection (bytes).
/// PUC-Rio defers the first collection; we use a reasonable default.
const INITIAL_GC_THRESHOLD: usize = 64 * 1024;

/// Step size for incremental GC scheduling (bytes).
/// Reference: `GCSTEPSIZE` in `lgc.c`.
pub const GCSTEPSIZE: usize = 1024;

/// Maximum number of arena slots to sweep per incremental step.
/// PUC-Rio uses 40, but our per-slot cost is higher (Rust bounds checking +
/// enum matching vs C pointer chasing). Larger batches amortize the fixed
/// overhead of `gc_singlestep` and `sweep_other_step` dispatch.
const GCSWEEPMAX: u32 = 80;

/// Cost (work units) of sweeping one batch of slots.
/// Reference: `GCSWEEPCOST` in `lgc.c`.
const GCSWEEPCOST: usize = 10;

/// Cost (work units) of running one `__gc` finalizer.
/// Reference: `GCFINALIZECOST` in `lgc.c`.
pub const GCFINALIZECOST: usize = 100;

/// Estimated size of a string object (bytes), for memory tracking.
pub const EST_STRING_SIZE: usize = 48;

/// Estimated size of a table object (bytes).
pub const EST_TABLE_SIZE: usize = 64;

/// Estimated size of a closure object (bytes).
pub const EST_CLOSURE_SIZE: usize = 48;

/// Estimated size of an upvalue object (bytes).
pub const EST_UPVALUE_SIZE: usize = 24;

/// Estimated size of a userdata object (bytes).
pub const EST_USERDATA_SIZE: usize = 48;

/// Estimated size of a thread object (bytes).
pub const EST_THREAD_SIZE: usize = 256;

// ---------------------------------------------------------------------------
// GcState: collection pacing fields (stored in Gc)
// ---------------------------------------------------------------------------

/// GC pacing and collection state.
pub struct GcState {
    /// Objects marked but not yet traversed.
    pub gray: Vec<GrayItem>,
    /// Objects that need re-traversal in atomic phase (threads).
    pub grayagain: Vec<GrayItem>,
    /// Weak tables found during mark phase (cleared in atomic).
    pub weak_tables: Vec<GcRef<Table>>,
    /// Estimated total allocated bytes.
    pub total_bytes: usize,
    /// Trigger threshold: collect when `total_bytes >= gc_threshold`.
    pub gc_threshold: usize,
    /// Pause percentage (default 200).
    pub gc_pause: u32,
    /// Step multiplier (default 200).
    pub gc_stepmul: u32,
    /// Whether the GC is currently running (prevents re-entrant collection).
    pub gc_running: bool,
    /// Current phase of the incremental GC cycle.
    pub phase: GcPhase,
    /// Cursor tracking incremental sweep position.
    pub sweep_cursor: SweepCursor,
    /// Estimated reachable memory from last mark phase (bytes).
    /// Starts as `total_bytes` after atomic, reduced as sweep frees objects.
    /// Used to compute the next GC threshold after a cycle completes.
    pub estimate: usize,
    /// Accumulated GC debt (bytes). Positive = work needed.
    /// Each allocation adds to debt; each GC step reduces it.
    /// When debt exceeds 0, the VM should run a GC step.
    ///
    /// Matches PUC-Rio's `gcdept` field.
    pub gc_debt: i64,
    /// Dead userdata with `__gc` metamethods, pending finalization.
    pub tmudata: Vec<GcRef<Userdata>>,
    /// Monotonic counter for userdata allocation ordering.
    /// Incremented each time a userdata is allocated. Used to ensure
    /// `__gc` finalization runs newest-first (matching PUC-Rio's LIFO order).
    pub ud_alloc_seq: u64,
    /// Memory allocation limit for OOM testing. `usize::MAX` = no limit.
    /// When `total_bytes` exceeds this, allocations return errors.
    pub alloc_limit: usize,
    /// Peak memory usage (bytes). Tracks the maximum value of `total_bytes`
    /// over the lifetime of the state. Used by `T.totalmem()`.
    pub max_bytes: usize,
}

impl GcState {
    pub fn new() -> Self {
        Self {
            gray: Vec::new(),
            grayagain: Vec::new(),
            weak_tables: Vec::new(),
            total_bytes: 0,
            // Start with a reasonable threshold so the first collection doesn't
            // trigger immediately. PUC-Rio defers the first collection until
            // enough allocations have occurred.
            gc_threshold: INITIAL_GC_THRESHOLD,
            gc_pause: DEFAULT_GC_PAUSE,
            gc_stepmul: DEFAULT_GC_STEPMUL,
            gc_running: false,
            phase: GcPhase::Pause,
            sweep_cursor: SweepCursor {
                arena_index: 0,
                slot_position: 0,
            },
            estimate: 0,
            gc_debt: 0,
            tmudata: Vec::new(),
            ud_alloc_seq: 0,
            alloc_limit: usize::MAX,
            max_bytes: 0,
        }
    }

    /// Tracks an allocation: adds `size` to `total_bytes` and updates
    /// `max_bytes` if a new peak is reached.
    #[inline]
    pub fn track_alloc(&mut self, size: usize) {
        self.total_bytes += size;
        if self.total_bytes > self.max_bytes {
            self.max_bytes = self.total_bytes;
        }
    }
}

impl Default for GcState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Gc: marking methods
// ---------------------------------------------------------------------------

impl Gc {
    /// Marks a Val if it is a collectable (GC-managed) type.
    #[inline]
    pub fn mark_value(&mut self, val: Val) {
        match val {
            Val::Str(r) => self.mark_string(r),
            Val::Table(r) => self.mark_table(r),
            Val::Function(r) => self.mark_closure(r),
            Val::Userdata(r) => self.mark_userdata(r),
            Val::Thread(r) => self.mark_thread(r),
            Val::Nil | Val::Bool(_) | Val::Num(_) | Val::LightUserdata(_) => {}
        }
    }

    /// Marks a string (directly to black -- strings have no children).
    #[inline]
    fn mark_string(&mut self, r: GcRef<LuaString>) {
        if let Some(color) = self.string_arena.color(r)
            && color.is_white()
        {
            self.string_arena.set_color(r, Color::Black);
        }
    }

    /// Marks a table: white -> gray, added to gray list for traversal.
    #[inline]
    fn mark_table(&mut self, r: GcRef<Table>) {
        if let Some(color) = self.tables.color(r)
            && color.is_white()
        {
            self.tables.set_color(r, Color::Gray);
            self.gc_state.gray.push(GrayItem::Table(r));
        }
    }

    /// Marks a closure: white -> gray, added to gray list.
    #[inline]
    fn mark_closure(&mut self, r: GcRef<Closure>) {
        if let Some(color) = self.closures.color(r)
            && color.is_white()
        {
            self.closures.set_color(r, Color::Gray);
            self.gc_state.gray.push(GrayItem::Closure(r));
        }
    }

    /// Marks a thread: white -> gray, added to gray list.
    #[inline]
    fn mark_thread(&mut self, r: GcRef<LuaThread>) {
        if let Some(color) = self.threads.color(r)
            && color.is_white()
        {
            self.threads.set_color(r, Color::Gray);
            self.gc_state.gray.push(GrayItem::Thread(r));
        }
    }

    /// Marks a userdata: marks metatable and env, then goes black.
    fn mark_userdata(&mut self, r: GcRef<Userdata>) {
        if let Some(color) = self.userdata.color(r)
            && color.is_white()
        {
            let (mt, env) = {
                if let Some(ud) = self.userdata.get(r) {
                    (ud.metatable(), ud.env())
                } else {
                    return;
                }
            };
            self.userdata.set_color(r, Color::Black);
            if let Some(mt) = mt {
                self.mark_table(mt);
            }
            if let Some(env) = env {
                self.mark_table(env);
            }
        }
    }

    /// Marks an upvalue: if closed, marks the stored value.
    #[inline]
    fn mark_upvalue(&mut self, r: GcRef<Upvalue>) {
        if let Some(color) = self.upvalues.color(r)
            && color.is_white()
        {
            let closed_val = {
                if let Some(uv) = self.upvalues.get(r) {
                    match &uv.state {
                        UpvalueState::Closed { value } => Some(*value),
                        UpvalueState::Open { .. } => None,
                    }
                } else {
                    return;
                }
            };
            self.upvalues.set_color(r, Color::Black);
            if let Some(val) = closed_val {
                self.mark_value(val);
            }
        }
    }

    /// Marks GC-internal roots: type metatables, tm_names.
    fn mark_gc_roots(&mut self) {
        for r in self.type_metatables.into_iter().flatten() {
            self.mark_table(r);
        }
        for r in self.tm_names.into_iter().flatten() {
            self.mark_string(r);
        }
    }

    // -----------------------------------------------------------------------
    // Propagation
    // -----------------------------------------------------------------------

    /// Traverses a gray table: marks metatable, array values, hash entries.
    ///
    /// Uses indexed access to avoid allocating Vecs for array/hash contents.
    /// Each Val is Copy, so we extract one at a time and release the table
    /// borrow before calling mark_value.
    fn traverse_table(&mut self, r: GcRef<Table>) {
        // Extract metadata in a single borrow.
        let (metatable, array_len, hash_count, weak_keys, weak_values) = {
            let Some(table) = self.tables.get(r) else {
                return;
            };
            let mt = table.metatable();
            let (wk, wv) = self.check_weak_mode(mt);
            (mt, table.array_len(), table.hash_node_count(), wk, wv)
        };

        self.tables.set_color(r, Color::Black);

        if let Some(mt) = metatable {
            self.mark_table(mt);
        }

        let is_weak = weak_keys || weak_values;
        if is_weak {
            self.gc_state.weak_tables.push(r);
        }

        if weak_keys && weak_values {
            return;
        }

        // Mark array values by index (no Vec allocation).
        if !weak_values {
            for i in 0..array_len {
                // Brief borrow per element: extract the Copy Val, then mark.
                let val = self.tables.get(r).and_then(|t| t.array_get(i));
                if let Some(val) = val {
                    self.mark_value(val);
                }
            }
        }

        // Mark hash entries by index (no Vec allocation).
        for i in 0..hash_count {
            let kv = self.tables.get(r).and_then(|t| t.hash_node_kv(i));
            let Some((key, val)) = kv else { continue };
            if val.is_nil() {
                continue;
            }
            if !weak_keys {
                self.mark_value(key);
            }
            if !weak_values {
                self.mark_value(val);
            }
        }
    }

    /// Returns `(weak_keys, weak_values)` for a table's `__mode` metafield.
    fn check_weak_mode(&self, mt: Option<GcRef<Table>>) -> (bool, bool) {
        let Some(mt_ref) = mt else {
            return (false, false);
        };
        let Some(mt) = self.tables.get(mt_ref) else {
            return (false, false);
        };
        // Look up "__mode" in the metatable.
        if let Some(mode_str_ref) = self.find_interned_string(b"__mode") {
            let mode_val = mt.get(Val::Str(mode_str_ref), &self.string_arena);
            if let Val::Str(s) = mode_val
                && let Some(ls) = self.string_arena.get(s)
            {
                let data = ls.data();
                return (data.contains(&b'k'), data.contains(&b'v'));
            }
        }
        (false, false)
    }

    /// Looks up a string in the TM names array, falling back to
    /// scanning interned strings.
    fn find_interned_string(&self, name: &[u8]) -> Option<GcRef<LuaString>> {
        // Check tm_names first (common metamethod names).
        for tm_name in &self.tm_names {
            if let Some(r) = tm_name
                && let Some(ls) = self.string_arena.get(*r)
                && ls.data() == name
            {
                return Some(*r);
            }
        }
        // Scan the string arena for a match.
        for (r, ls, _) in &self.string_arena {
            if ls.data() == name {
                return Some(r);
            }
        }
        None
    }

    /// Traverses a gray closure: marks env, upvalues, and proto constants.
    ///
    /// Avoids Vec allocations by:
    /// - Cloning the ProtoRef (cheap refcount bump) and walking it directly
    /// - Accessing upvalue refs and inline upvals by index
    fn traverse_closure(&mut self, r: GcRef<Closure>) {
        // Extract what we need with minimal cloning.
        enum ClosureData {
            Lua {
                env: GcRef<Table>,
                upvalue_count: usize,
                proto: crate::vm::proto::ProtoRef,
            },
            Rust {
                env: Option<GcRef<Table>>,
                upvalue_count: usize,
            },
        }

        let data = {
            let Some(cl) = self.closures.get(r) else {
                return;
            };
            match cl {
                Closure::Lua(lc) => ClosureData::Lua {
                    env: lc.env,
                    upvalue_count: lc.upvalues.len(),
                    proto: crate::vm::proto::ProtoRef::clone(&lc.proto),
                },
                Closure::Rust(rc) => ClosureData::Rust {
                    env: rc.env,
                    upvalue_count: rc.upvalues.len(),
                },
            }
        };

        self.closures.set_color(r, Color::Black);

        match data {
            ClosureData::Lua {
                env,
                upvalue_count,
                proto,
            } => {
                self.mark_table(env);
                // Mark upvalue refs by index.
                for i in 0..upvalue_count {
                    let uv_ref = self.closures.get(r).and_then(|cl| match cl {
                        Closure::Lua(lc) => lc.upvalues.get(i).copied(),
                        Closure::Rust(_) => None,
                    });
                    if let Some(uv_ref) = uv_ref {
                        self.mark_upvalue(uv_ref);
                    }
                }
                // Walk the Proto tree directly -- no Vec allocation.
                self.mark_proto_constants(&proto);
            }
            ClosureData::Rust { env, upvalue_count } => {
                if let Some(env) = env {
                    self.mark_table(env);
                }
                // Mark inline upvals by index.
                for i in 0..upvalue_count {
                    let val = self.closures.get(r).and_then(|cl| match cl {
                        Closure::Rust(rc) => rc.upvalues.get(i).copied(),
                        Closure::Lua(_) => None,
                    });
                    if let Some(val) = val {
                        self.mark_value(val);
                    }
                }
            }
        }
    }

    /// Recursively marks all Val constants in a Proto tree without allocation.
    fn mark_proto_constants(&mut self, proto: &Proto) {
        for val in &proto.constants {
            self.mark_value(*val);
        }
        for nested in &proto.protos {
            self.mark_proto_constants(nested);
        }
    }

    /// Traverses a gray thread: marks stack values and open upvalues.
    /// Threads are moved to `grayagain` for atomic re-traversal.
    ///
    /// Uses indexed access to avoid allocating temporary Vecs for the
    /// thread's stack and upvalue lists. Each Val/GcRef is Copy, so we
    /// extract one at a time via re-borrow per iteration.
    fn traverse_thread(&mut self, r: GcRef<LuaThread>) {
        // Extract scalar metadata in one borrow.
        let (stack_top, open_uv_len, suspended_uv_len, thread_global, hook_func) = {
            if let Some(thread) = self.threads.get(r) {
                (
                    thread.top.min(thread.stack.len()),
                    thread.open_upvalues.len(),
                    thread.suspended_upvals.len(),
                    thread.global,
                    thread.hook.hook_func,
                )
            } else {
                return;
            }
        };

        self.threads.set_color(r, Color::Black);

        self.mark_table(thread_global);
        self.mark_value(hook_func);

        // Mark stack values by index (no Vec allocation).
        for i in 0..stack_top {
            let val = self.threads.get(r).map(|t| t.stack[i]);
            if let Some(val) = val {
                self.mark_value(val);
            }
        }

        // Mark open upvalues by index.
        for i in 0..open_uv_len {
            let uv_ref = self.threads.get(r).map(|t| t.open_upvalues[i]);
            if let Some(uv_ref) = uv_ref {
                self.mark_upvalue(uv_ref);
            }
        }

        // Mark suspended upvalues by index.
        for i in 0..suspended_uv_len {
            let uv_ref = self.threads.get(r).map(|t| t.suspended_upvals[i].0);
            if let Some(uv_ref) = uv_ref {
                self.mark_upvalue(uv_ref);
            }
        }

        self.gc_state.grayagain.push(GrayItem::Thread(r));
    }

    /// Propagates one gray object, returning the estimated work (bytes traversed).
    ///
    /// Returns `None` if the gray list is empty (propagation is complete).
    /// The returned cost approximates the memory traversed, matching PUC-Rio's
    /// `propagatemark()` which returns `sizeof(T) + child_sizes`.
    pub fn propagate_one(&mut self) -> PropagateResult {
        let Some(item) = self.gc_state.gray.pop() else {
            return PropagateResult::Empty;
        };
        match item {
            GrayItem::Table(r) => {
                let size = if let Some(t) = self.tables.get(r) {
                    EST_TABLE_SIZE + t.array_slice().len() * 16 + t.hash_size() as usize * 32
                } else {
                    EST_TABLE_SIZE
                };
                self.traverse_table(r);
                PropagateResult::Done(size)
            }
            GrayItem::Closure(r) => {
                let size = if let Some(cl) = self.closures.get(r) {
                    match cl {
                        Closure::Lua(lc) => EST_CLOSURE_SIZE + lc.upvalues.len() * 8,
                        Closure::Rust(rc) => EST_CLOSURE_SIZE + rc.upvalues.len() * 16,
                    }
                } else {
                    EST_CLOSURE_SIZE
                };
                self.traverse_closure(r);
                PropagateResult::Done(size)
            }
            GrayItem::Thread(r) => {
                let size = if let Some(th) = self.threads.get(r) {
                    EST_THREAD_SIZE + th.stack.len() * 16
                } else {
                    EST_THREAD_SIZE
                };
                self.traverse_thread(r);
                PropagateResult::Done(size)
            }
            GrayItem::MainThread => PropagateResult::NeedMainThread,
        }
    }

    /// Propagates all gray objects until the gray list is empty.
    /// Returns total bytes traversed.
    ///
    /// Note: `MainThread` items are treated as zero-cost here since the
    /// main thread's stack was already traversed inline (used in atomic
    /// and `full_gc` contexts where cost accounting doesn't matter).
    fn propagate_all(&mut self) -> usize {
        let mut total = 0;
        loop {
            match self.propagate_one() {
                PropagateResult::Done(cost) => total += cost,
                PropagateResult::NeedMainThread => {
                    // In propagate_all context (atomic/full_gc), we skip
                    // the main thread because it was already traversed
                    // by mark_roots or will be traversed by the caller.
                }
                PropagateResult::Empty => break,
            }
        }
        total
    }

    // -----------------------------------------------------------------------
    // Atomic phase
    // -----------------------------------------------------------------------

    /// Atomic phase: re-traverse threads, process weak tables, separate
    /// finalizable userdata, flip whites, prepare for sweep.
    ///
    /// This is the indivisible phase between mark and sweep. It:
    /// 1. Re-traverses grayagain objects (threads mostly)
    /// 2. Separates dead userdata with `__gc` into `tmudata`
    /// 3. Marks tmudata entries as reachable (so they survive sweep)
    /// 4. Flips the current white
    /// 5. Clears dead entries from weak tables
    /// 6. Sets up sweep cursor and estimate
    fn atomic(&mut self) {
        let grayagain = std::mem::take(&mut self.gc_state.grayagain);
        for item in grayagain {
            match item {
                GrayItem::Thread(r) => {
                    // Extract scalar metadata, then use indexed access
                    // (same pattern as traverse_thread).
                    let (stack_top, open_uv_len, suspended_uv_len, thread_global) = {
                        if let Some(thread) = self.threads.get(r) {
                            (
                                thread.top.min(thread.stack.len()),
                                thread.open_upvalues.len(),
                                thread.suspended_upvals.len(),
                                thread.global,
                            )
                        } else {
                            continue;
                        }
                    };
                    self.threads.set_color(r, Color::Black);
                    self.mark_table(thread_global);
                    for i in 0..stack_top {
                        let val = self.threads.get(r).map(|t| t.stack[i]);
                        if let Some(val) = val {
                            self.mark_value(val);
                        }
                    }
                    for i in 0..open_uv_len {
                        let uv_ref = self.threads.get(r).map(|t| t.open_upvalues[i]);
                        if let Some(uv_ref) = uv_ref {
                            self.mark_upvalue(uv_ref);
                        }
                    }
                    for i in 0..suspended_uv_len {
                        let uv_ref = self.threads.get(r).map(|t| t.suspended_upvals[i].0);
                        if let Some(uv_ref) = uv_ref {
                            self.mark_upvalue(uv_ref);
                        }
                    }
                }
                GrayItem::Table(r) => {
                    self.tables.set_color(r, Color::Gray);
                    self.traverse_table(r);
                }
                GrayItem::Closure(r) => {
                    self.closures.set_color(r, Color::Gray);
                    self.traverse_closure(r);
                }
                GrayItem::MainThread => {
                    // Main thread grayagain is handled by the caller
                    // (LuaState::gc_singlestep) which has access to the
                    // main thread's stack.
                }
            }
        }

        // Propagate anything new.
        self.propagate_all();

        // Separate dead userdata with __gc metamethods.
        let ud_size = self.separate_userdata();

        // Mark tmudata entries as reachable (they must survive sweep).
        self.mark_tmudata();
        self.propagate_all();

        // Flip whites: current becomes the new allocation color, old white
        // (now `other_white()`) marks objects that were not reached.
        self.current_white = self.current_white.other_white();

        // Clear weak tables (uses the new other_white to identify dead objects).
        self.clear_weak_tables();

        // Set estimate: reachable memory minus dead userdata size.
        self.gc_state.estimate = self.gc_state.total_bytes.saturating_sub(ud_size);

        // Prepare sweep cursor: start with string arena.
        self.gc_state.sweep_cursor = SweepCursor {
            arena_index: 0,
            slot_position: 0,
        };

        // Transition to SweepString phase.
        self.gc_state.phase = GcPhase::SweepString;
    }

    /// Separates dead userdata with `__gc` metamethods into `tmudata`.
    ///
    /// Scans the userdata arena for objects that are dead (other-white),
    /// have a `__gc` metamethod, and are not yet finalized. Moves them
    /// to the `tmudata` list and marks them (so they survive sweep).
    ///
    /// Returns the estimated size of separated userdata.
    ///
    /// Reference: `luaC_separateudata` in `lgc.c`.
    fn separate_userdata(&mut self) -> usize {
        // Dead objects have the CURRENT white (pre-flip). They were allocated
        // with current_white and never marked during this cycle.
        // Note: this runs BEFORE the white flip in atomic().
        let dead_white = self.current_white;
        let mut dead_size = 0usize;

        // First pass: collect refs of dead userdata with __gc.
        let mut to_finalize = Vec::new();
        for (r, ud, color) in &self.userdata {
            if color != dead_white {
                continue; // alive (marked Black/Gray) or already finalized
            }
            if ud.finalized() {
                continue; // already finalized
            }
            // Check for __gc metamethod.
            if let Some(mt_ref) = ud.metatable()
                && self.has_gc_metamethod(mt_ref)
            {
                to_finalize.push(r);
                dead_size += EST_USERDATA_SIZE;
            }
        }

        // Sort by alloc_seq ascending (oldest first) so that after pushing
        // to tmudata Vec, the newest is at the end. Vec::pop() returns the
        // last element, giving us newest-first (LIFO) finalization order
        // matching PUC-Rio.
        to_finalize.sort_by(|a, b| {
            let seq_a = self.userdata.get(*a).map_or(0, Userdata::alloc_seq);
            let seq_b = self.userdata.get(*b).map_or(0, Userdata::alloc_seq);
            seq_a.cmp(&seq_b)
        });

        // Second pass: mark them finalized and add to tmudata.
        for r in &to_finalize {
            if let Some(ud) = self.userdata.get_mut(*r) {
                ud.set_finalized(true);
            }
            // Mark the userdata so it survives sweep.
            self.userdata.set_color(*r, Color::Gray);
            self.gc_state.tmudata.push(*r);
        }

        dead_size
    }

    /// Checks if a table has a `__gc` field (for userdata finalization).
    fn has_gc_metamethod(&self, mt_ref: GcRef<Table>) -> bool {
        if let Some(gc_name) = self.find_interned_string(b"__gc")
            && let Some(mt) = self.tables.get(mt_ref)
        {
            let val = mt.get(Val::Str(gc_name), &self.string_arena);
            return !val.is_nil();
        }
        false
    }

    /// Marks all userdata in `tmudata` as reachable.
    ///
    /// The userdata itself was already set to Gray by `separate_userdata`
    /// (so it survives sweep). We must mark its metatable and environment
    /// directly, because `mark_userdata` bails out on non-white objects.
    ///
    /// Reference: `marktmu()` in `lgc.c` which calls `makewhite` then
    /// `reallymarkobject` to force re-marking regardless of current color.
    fn mark_tmudata(&mut self) {
        let tmudata: Vec<GcRef<Userdata>> = self.gc_state.tmudata.clone();
        for r in &tmudata {
            // Mark metatable and environment so they survive sweep.
            let (mt, env) = {
                if let Some(ud) = self.userdata.get(*r) {
                    (ud.metatable(), ud.env())
                } else {
                    continue;
                }
            };
            self.userdata.set_color(*r, Color::Black);
            if let Some(mt) = mt {
                self.mark_table(mt);
            }
            if let Some(env) = env {
                self.mark_table(env);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Weak table clearing
    // -----------------------------------------------------------------------

    /// Clears dead entries from weak tables.
    ///
    /// Strings are never cleared from weak tables (Lua 5.1.1 quirk).
    ///
    /// PUC-Rio's `iscleared` function marks any strings it encounters
    /// during weak table clearing. This prevents them from being swept.
    /// We replicate this by first marking all strings in weak tables,
    /// then clearing dead non-string entries.
    fn clear_weak_tables(&mut self) {
        let weak_refs = std::mem::take(&mut self.gc_state.weak_tables);
        let other_white = self.current_white.other_white();
        let current_white = self.current_white;

        // First pass: mark all strings referenced in weak tables.
        // PUC-Rio's iscleared() calls stringmark() on any string it finds,
        // preventing them from being swept regardless of weak mode.
        for &table_ref in &weak_refs {
            let Some(table) = self.tables.get(table_ref) else {
                continue;
            };
            let array_len = table.array_len();
            let hash_count = table.hash_node_count();
            // Mark strings in array part.
            for i in 0..array_len {
                if let Some(Val::Str(s)) = table.array_get(i) {
                    self.string_arena.set_color(s, current_white);
                }
            }
            // Mark strings in hash part (both keys and values).
            for i in 0..hash_count {
                if let Some((key, val)) = table.hash_node_kv(i) {
                    if let Val::Str(s) = key {
                        self.string_arena.set_color(s, current_white);
                    }
                    if let Val::Str(s) = val {
                        self.string_arena.set_color(s, current_white);
                    }
                }
            }
        }

        // Second pass: clear dead non-string entries.
        for table_ref in weak_refs {
            let (weak_keys, weak_values) = {
                if let Some(table) = self.tables.get(table_ref) {
                    self.get_weak_mode(table.metatable())
                } else {
                    continue;
                }
            };

            if !weak_keys && !weak_values {
                continue;
            }

            let dead_array_indices: Vec<usize> = if weak_values {
                if let Some(table) = self.tables.get(table_ref) {
                    table
                        .array_slice()
                        .iter()
                        .enumerate()
                        .filter(|(_, val)| self.is_dead_collectable(**val, other_white, false))
                        .map(|(i, _)| i)
                        .collect()
                } else {
                    continue;
                }
            } else {
                Vec::new()
            };

            let dead_hash_indices: Vec<usize> = {
                if let Some(table) = self.tables.get(table_ref) {
                    table.find_dead_hash_entries(weak_keys, weak_values, |val, is_key| {
                        self.is_dead_collectable(val, other_white, is_key)
                    })
                } else {
                    continue;
                }
            };

            if let Some(table) = self.tables.get_mut(table_ref) {
                for idx in &dead_array_indices {
                    table.nil_array_entry(*idx);
                }
                table.nil_hash_entries(&dead_hash_indices);
            }
        }
    }

    /// Checks if a Val references a dead collectable for weak table clearing.
    ///
    /// Strings are excluded (never dead in weak tables -- handled separately).
    ///
    /// When `is_key` is false (checking a VALUE), finalized userdata are also
    /// considered dead. This matches PUC-Rio's `iscleared()` which returns
    /// true for `isfinalized(u)` when checking values but not keys.
    fn is_dead_collectable(&self, val: Val, other_white: Color, is_key: bool) -> bool {
        #[allow(clippy::match_same_arms)]
        match val {
            Val::Str(_) => false,
            Val::Table(r) => self.tables.color(r) == Some(other_white),
            Val::Function(r) => self.closures.color(r) == Some(other_white),
            Val::Userdata(r) => {
                self.userdata.color(r) == Some(other_white)
                    || (!is_key
                        && self
                            .userdata
                            .get(r)
                            .is_some_and(super::super::value::Userdata::finalized))
            }
            Val::Thread(r) => self.threads.color(r) == Some(other_white),
            _ => false,
        }
    }

    /// Returns (weak_keys, weak_values) for a table's __mode.
    fn get_weak_mode(&self, mt: Option<GcRef<Table>>) -> (bool, bool) {
        let Some(mt_ref) = mt else {
            return (false, false);
        };
        let Some(mt) = self.tables.get(mt_ref) else {
            return (false, false);
        };
        if let Some(mode_str_ref) = self.find_interned_string(b"__mode") {
            let mode_val = mt.get(Val::Str(mode_str_ref), &self.string_arena);
            if let Val::Str(s) = mode_val
                && let Some(ls) = self.string_arena.get(s)
            {
                let data = ls.data();
                return (data.contains(&b'k'), data.contains(&b'v'));
            }
        }
        (false, false)
    }

    // -----------------------------------------------------------------------
    // Sweep phase
    // -----------------------------------------------------------------------

    /// Incrementally sweeps the string arena.
    ///
    /// Sweeps up to `GCSWEEPMAX` slots from the current cursor position.
    /// Returns the estimated work cost and advances the cursor.
    /// When done, transitions to the `Sweep` phase.
    fn sweep_strings_step(&mut self) -> usize {
        let dead = self.current_white.other_white();
        let new_white = self.current_white;
        let start = self.gc_state.sweep_cursor.slot_position;

        let (freed, next_pos, is_done) = self
            .string_arena
            .sweep_partial(dead, new_white, start, GCSWEEPMAX);

        let freed_bytes = freed as usize * EST_STRING_SIZE;
        // PUC-Rio decrements totalbytes inside freeobj(); we must do the same
        // so that threshold/debt calculations in the step loop are correct.
        self.gc_state.total_bytes = self.gc_state.total_bytes.saturating_sub(freed_bytes);
        self.gc_state.estimate = self.gc_state.estimate.saturating_sub(freed_bytes);

        if is_done {
            // Done sweeping strings. Clean up intern table and move to Sweep phase.
            self.strings.sweep_dead(&self.string_arena);
            self.gc_state.phase = GcPhase::Sweep;
            self.gc_state.sweep_cursor = SweepCursor {
                arena_index: 0,
                slot_position: 0,
            };
        } else {
            self.gc_state.sweep_cursor.slot_position = next_pos;
        }

        GCSWEEPCOST
    }

    /// Incrementally sweeps non-string arenas (tables, closures, upvalues,
    /// userdata, threads).
    ///
    /// Sweeps up to `GCSWEEPMAX` slots from the current arena/cursor position.
    /// When all arenas are done, transitions to the `Finalize` phase.
    fn sweep_other_step(&mut self) -> usize {
        let dead = self.current_white.other_white();
        let new_white = self.current_white;
        let arena_idx = self.gc_state.sweep_cursor.arena_index;
        let start = self.gc_state.sweep_cursor.slot_position;

        let (freed, est_size, next_pos, is_arena_done) = match arena_idx {
            0 => {
                let (f, np, done) = self
                    .tables
                    .sweep_partial(dead, new_white, start, GCSWEEPMAX);
                (f, EST_TABLE_SIZE, np, done)
            }
            1 => {
                let (f, np, done) = self
                    .closures
                    .sweep_partial(dead, new_white, start, GCSWEEPMAX);
                (f, EST_CLOSURE_SIZE, np, done)
            }
            2 => {
                let (f, np, done) = self
                    .upvalues
                    .sweep_partial(dead, new_white, start, GCSWEEPMAX);
                (f, EST_UPVALUE_SIZE, np, done)
            }
            3 => {
                let (f, np, done) = self
                    .userdata
                    .sweep_partial(dead, new_white, start, GCSWEEPMAX);
                (f, EST_USERDATA_SIZE, np, done)
            }
            _ => {
                // Arena 4 = threads (last one)
                let (f, np, done) = self
                    .threads
                    .sweep_partial(dead, new_white, start, GCSWEEPMAX);
                (f, EST_THREAD_SIZE, np, done)
            }
        };

        let freed_bytes = freed as usize * est_size;
        // PUC-Rio decrements totalbytes inside freeobj(); we must do the same
        // so that threshold/debt calculations in the step loop are correct.
        self.gc_state.total_bytes = self.gc_state.total_bytes.saturating_sub(freed_bytes);
        self.gc_state.estimate = self.gc_state.estimate.saturating_sub(freed_bytes);

        if is_arena_done {
            if arena_idx >= 4 {
                // All arenas swept. Transition to Finalize.
                self.gc_state.phase = GcPhase::Finalize;
            } else {
                // Move to next arena.
                self.gc_state.sweep_cursor.arena_index = arena_idx + 1;
                self.gc_state.sweep_cursor.slot_position = 0;
            }
        } else {
            self.gc_state.sweep_cursor.slot_position = next_pos;
        }

        GCSWEEPMAX as usize * GCSWEEPCOST
    }

    // -----------------------------------------------------------------------
    // Write barriers
    // -----------------------------------------------------------------------

    /// Backward barrier: demotes a black table parent to gray when a
    /// white child is written into it during the Propagate phase.
    ///
    /// Tables use backward barriers (parent goes to `grayagain`) because
    /// tables are frequently mutated and re-traversal is cheaper than
    /// forward-marking each new value.
    ///
    /// Reference: `luaC_barrierback` in `lgc.c`.
    pub fn barrier_back(&mut self, parent: GcRef<Table>) {
        if self.gc_state.phase != GcPhase::Propagate {
            return;
        }
        if let Some(color) = self.tables.color(parent)
            && color.is_black()
        {
            self.tables.set_color(parent, Color::Gray);
            self.gc_state.grayagain.push(GrayItem::Table(parent));
        }
    }

    /// Forward barrier: marks a white child when written into a black
    /// non-table parent during the Propagate phase.
    ///
    /// Used for upvalue writes and similar non-table mutations. If not
    /// in the Propagate phase, makes the parent white instead (to avoid
    /// triggering further barriers).
    ///
    /// Reference: `luaC_barrierf` in `lgc.c`.
    pub fn barrier_forward_val(&mut self, parent_color: Color, child: Val) {
        // Only active during Propagate phase. During other phases,
        // the tri-color invariant doesn't need maintaining.
        if self.gc_state.phase != GcPhase::Propagate {
            return;
        }
        if !parent_color.is_black() {
            return;
        }
        // Check if child is white (current white = alive but unmarked).
        let child_is_white = match child {
            Val::Str(r) => self.string_arena.color(r) == Some(self.current_white),
            Val::Table(r) => self.tables.color(r) == Some(self.current_white),
            Val::Function(r) => self.closures.color(r) == Some(self.current_white),
            Val::Userdata(r) => self.userdata.color(r) == Some(self.current_white),
            Val::Thread(r) => self.threads.color(r) == Some(self.current_white),
            _ => false,
        };
        if child_is_white {
            self.mark_value(child);
        }
    }

    // -----------------------------------------------------------------------
    // Memory tracking
    // -----------------------------------------------------------------------

    /// Estimates total memory from arena occupancy.
    pub fn estimate_memory(&self) -> usize {
        let strings = self.string_arena.len() as usize * EST_STRING_SIZE;
        let tables = self.tables.len() as usize * EST_TABLE_SIZE;
        let closures = self.closures.len() as usize * EST_CLOSURE_SIZE;
        let upvalues = self.upvalues.len() as usize * EST_UPVALUE_SIZE;
        let userdata = self.userdata.len() as usize * EST_USERDATA_SIZE;
        let threads = self.threads.len() as usize * EST_THREAD_SIZE;
        strings + tables + closures + upvalues + userdata + threads
    }

    /// Returns whether the GC threshold has been exceeded.
    pub fn should_collect(&self) -> bool {
        !self.gc_state.gc_running && self.gc_state.total_bytes >= self.gc_state.gc_threshold
    }

    /// Updates the threshold after a collection cycle completes.
    ///
    /// Sets threshold based on the estimate of reachable memory and the
    /// pause parameter. Matches PUC-Rio's `setthreshold` macro:
    /// `threshold = (estimate / 100) * gcpause`.
    pub fn update_threshold(&mut self) {
        let estimate = self.gc_state.estimate.max(self.estimate_memory());
        self.gc_state.total_bytes = self.estimate_memory();
        self.gc_state.gc_threshold =
            (estimate / 100).saturating_mul(self.gc_state.gc_pause as usize);
        if self.gc_state.gc_threshold < 4096 {
            self.gc_state.gc_threshold = 4096;
        }
    }
}

// ---------------------------------------------------------------------------
// LuaState: incremental GC control
// ---------------------------------------------------------------------------

impl LuaState {
    /// Performs one incremental GC step, returning the work cost.
    ///
    /// Advances the state machine by one unit of work in the current phase:
    /// - `Pause`: marks roots, transitions to `Propagate`
    /// - `Propagate`: traverses one gray object; when gray list empties,
    ///   runs atomic phase and transitions to `SweepString`
    /// - `SweepString`: sweeps a batch of string arena slots
    /// - `Sweep`: sweeps a batch of non-string arena slots
    /// - `Finalize`: runs one `__gc` finalizer; when done, transitions to `Pause`
    ///
    /// Returns the estimated work cost (bytes-equivalent).
    ///
    /// Reference: `singlestep()` in `lgc.c`.
    pub fn gc_singlestep(&mut self) -> LuaResult<usize> {
        let result = match self.gc.gc_state.phase {
            GcPhase::Pause => {
                // Start a new collection cycle.
                self.gc.gc_state.gray.clear();
                self.gc.gc_state.grayagain.clear();
                self.gc.gc_state.weak_tables.clear();
                self.mark_roots();
                self.gc.gc_state.phase = GcPhase::Propagate;
                0
            }
            GcPhase::Propagate => {
                match self.gc.propagate_one() {
                    PropagateResult::Done(cost) => cost,
                    PropagateResult::NeedMainThread => {
                        // The main thread's traversal must be done here
                        // because Gc doesn't own the main thread's stack.
                        self.traverse_main_thread()
                    }
                    PropagateResult::Empty => {
                        // Gray list empty: run atomic phase.
                        // Re-traverse main thread during atomic (PUC-Rio
                        // re-marks the running thread in atomic()).
                        self.traverse_main_thread_for_atomic();
                        self.gc.atomic();
                        // atomic() sets phase to SweepString.
                        0
                    }
                }
            }
            GcPhase::SweepString => self.gc.sweep_strings_step(),
            GcPhase::Sweep => self.gc.sweep_other_step(),
            GcPhase::Finalize => {
                if self.call_gc_finalizer()? {
                    if self.gc.gc_state.estimate > GCFINALIZECOST {
                        self.gc.gc_state.estimate -= GCFINALIZECOST;
                    }
                    GCFINALIZECOST
                } else {
                    // No more finalizers: cycle complete.
                    self.gc.gc_state.phase = GcPhase::Pause;
                    self.gc.gc_state.gc_debt = 0;
                    0
                }
            }
        };
        Ok(result)
    }

    /// Performs incremental GC work with the given budget.
    ///
    /// Runs `singlestep()` repeatedly until the budget is exhausted or
    /// a full cycle completes (phase returns to `Pause`).
    ///
    /// Returns `true` if a full cycle completed.
    ///
    /// Reference: `luaC_step()` in `lgc.c`.
    pub fn gc_step(&mut self, mut budget: i64) -> LuaResult<bool> {
        if self.gc.gc_state.gc_running {
            return Ok(false);
        }
        self.gc.gc_state.gc_running = true;

        let result = (|| {
            loop {
                let cost = self.gc_singlestep()?;
                budget -= cost as i64;
                if self.gc.gc_state.phase == GcPhase::Pause {
                    self.gc.update_threshold();
                    return Ok(true);
                }
                if budget <= 0 {
                    break;
                }
            }

            // Cycle not yet complete. Set threshold for next automatic trigger.
            if self.gc.gc_state.gc_debt < GCSTEPSIZE as i64 {
                self.gc.gc_state.gc_threshold =
                    self.gc.gc_state.total_bytes.saturating_add(GCSTEPSIZE);
            } else {
                self.gc.gc_state.gc_debt -= GCSTEPSIZE as i64;
                self.gc.gc_state.gc_threshold = self.gc.gc_state.total_bytes;
            }
            Ok(false)
        })();

        self.gc.gc_state.gc_running = false;
        result
    }

    /// Runs a full mark-sweep garbage collection cycle.
    ///
    /// Follows PUC-Rio's `luaC_fullgc()`: if a cycle is in progress,
    /// finish pending sweep/finalize, then run a complete fresh cycle.
    ///
    /// Errors from `__gc` finalizers propagate to the caller (PUC-Rio
    /// 5.1.1 uses `luaD_call`, not pcall, for GCTM).
    pub fn full_gc(&mut self) -> LuaResult<()> {
        if self.gc.gc_state.gc_running {
            return Ok(());
        }
        self.gc.gc_state.gc_running = true;

        let result = self.full_gc_inner();

        // Always reset gc_running, even on error.
        self.gc.gc_state.gc_running = false;
        result
    }

    /// Inner implementation of `full_gc`, separated so gc_running cleanup
    /// always happens in the outer function.
    fn full_gc_inner(&mut self) -> LuaResult<()> {
        let phase = self.gc.gc_state.phase;

        // If currently in mark phase, abort it and reset to sweep.
        if phase == GcPhase::Pause || phase == GcPhase::Propagate {
            self.gc.gc_state.gray.clear();
            self.gc.gc_state.grayagain.clear();
            self.gc.gc_state.weak_tables.clear();
            self.gc.gc_state.phase = GcPhase::SweepString;
            self.gc.gc_state.sweep_cursor = SweepCursor {
                arena_index: 0,
                slot_position: 0,
            };
        }

        // Finish any pending sweep/finalize phase.
        while self.gc.gc_state.phase != GcPhase::Pause
            && self.gc.gc_state.phase != GcPhase::Propagate
        {
            self.gc.gc_state.gc_running = false;
            self.gc_singlestep()?;
            self.gc.gc_state.gc_running = true;
        }

        // Now run a complete fresh cycle: mark -> propagate -> atomic -> sweep -> finalize.
        self.gc.gc_state.gray.clear();
        self.gc.gc_state.grayagain.clear();
        self.gc.gc_state.weak_tables.clear();
        self.mark_roots();
        self.gc.gc_state.phase = GcPhase::Propagate;

        // Drive the state machine to completion.
        while self.gc.gc_state.phase != GcPhase::Pause {
            self.gc.gc_state.gc_running = false;
            self.gc_singlestep()?;
            self.gc.gc_state.gc_running = true;
        }

        self.gc.update_threshold();
        Ok(())
    }

    /// Runs one `__gc` finalizer from the `tmudata` list.
    ///
    /// Returns `Ok(true)` if a finalizer was called, `Ok(false)` if the
    /// list is empty. Errors from `__gc` propagate to the caller.
    ///
    /// PUC-Rio 5.1.1's `GCTM()` uses `luaD_call` (not pcall), so errors
    /// propagate. This was changed in Lua 5.2+ to protect __gc calls.
    ///
    /// Reference: `GCTM()` in `lgc.c`.
    fn call_gc_finalizer(&mut self) -> LuaResult<bool> {
        let Some(ud_ref) = self.gc.gc_state.tmudata.pop() else {
            return Ok(false);
        };

        // Find the __gc metamethod.
        let gc_fn = {
            let mt_ref = self
                .gc
                .userdata
                .get(ud_ref)
                .and_then(super::super::value::Userdata::metatable);
            if let Some(mt_ref) = mt_ref {
                if let Some(gc_name) = self.gc.find_interned_string(b"__gc") {
                    if let Some(mt) = self.gc.tables.get(mt_ref) {
                        let v = mt.get(Val::Str(gc_name), &self.gc.string_arena);
                        if v.is_nil() { None } else { Some(v) }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        let Some(gc_fn) = gc_fn else {
            return Ok(true); // Had a tmudata entry but no __gc method
        };

        // Temporarily prevent recursive GC (PUC-Rio: raise threshold).
        self.gc.gc_state.gc_threshold = self.gc.gc_state.total_bytes.saturating_add(GCSTEPSIZE);

        // Push gc_fn and userdata, then call.
        // Errors propagate to caller (PUC-Rio 5.1.1 behavior).
        // Save state so we can restore on error (unlike PUC-Rio's longjmp,
        // our Result-based errors don't automatically unwind the call stack).
        let saved_ci = self.ci;
        let saved_top = self.top;
        let base = self.top;
        self.push(gc_fn);
        self.push(Val::Userdata(ud_ref));
        // PUC-Rio GCTM: stop debug hooks during GC tag methods.
        let old_allow_hook = self.hook.allow_hook;
        self.hook.allow_hook = false;
        let result = self.call_function(base, 0);
        self.hook.allow_hook = old_allow_hook;
        if result.is_err() {
            self.ci = saved_ci;
            self.base = self.call_stack[saved_ci].base;
            self.top = saved_top;
        }
        result?;

        // Restore threshold (only reached on success; on error, PUC-Rio
        // skips this too via longjmp).
        self.gc.gc_state.gc_threshold =
            self.gc.gc_state.total_bytes * self.gc.gc_state.gc_pause as usize / 100;

        Ok(true)
    }

    /// Checks if automatic GC should run and runs a step if needed.
    ///
    /// Uses debt-based triggering: when `gc_debt > 0` and GC is enabled,
    /// runs an incremental step with a budget based on the step multiplier.
    #[inline]
    pub fn gc_check(&mut self) -> LuaResult<()> {
        // PUC-Rio's luaC_checkGC: only checks totalbytes >= GCthreshold.
        // "stop" sets threshold = MAX to disable auto-GC; a subsequent
        // full GC resets the threshold via update_threshold().
        if !self.gc.gc_state.gc_running
            && self.gc.gc_state.total_bytes >= self.gc.gc_state.gc_threshold
        {
            self.gc_step_auto()?;
        }
        // Check allocation limit (used by T.totalmem for OOM testing).
        if self.gc.gc_state.total_bytes > self.gc.gc_state.alloc_limit {
            return Err(LuaError::Memory);
        }
        Ok(())
    }

    /// Runs an automatic incremental GC step based on current parameters.
    ///
    /// Budget = `(GCSTEPSIZE / 100) * gc_stepmul`.
    /// If stepmul is 0, runs with no limit (effectively a full cycle).
    pub fn gc_step_auto(&mut self) -> LuaResult<()> {
        let stepmul = i64::from(self.gc.gc_state.gc_stepmul);
        let budget = if stepmul == 0 {
            i64::MAX / 2 // no limit
        } else {
            (GCSTEPSIZE as i64 / 100) * stepmul
        };

        // Accumulate debt.
        self.gc.gc_state.gc_debt +=
            self.gc.gc_state.total_bytes as i64 - self.gc.gc_state.gc_threshold as i64;

        self.gc_step(budget)?;
        Ok(())
    }

    /// Marks all GC roots.
    ///
    /// Follows PUC-Rio's `markroot()`: marks the main thread as a gray
    /// object (deferred traversal during propagation), then marks the
    /// global table and registry. The main thread's stack traversal
    /// happens when `MainThread` is popped from the gray list during
    /// the propagation phase, ensuring its cost is properly accounted.
    fn mark_roots(&mut self) {
        // Push main thread onto gray list for deferred traversal.
        // This matches PUC-Rio's `markobject(g, g->mainthread)`.
        self.gc.gc_state.gray.push(GrayItem::MainThread);

        // Mark global table and registry (pushed onto gray list).
        // PUC-Rio: `markvalue(g, gt(g->mainthread))`, `markvalue(g, registry(L))`
        let global = self.global;
        let registry = self.registry;
        self.gc.mark_table(global);
        self.gc.mark_table(registry);

        self.gc.mark_gc_roots();
    }

    /// Traverses the main thread's stack, open upvalues, call stack,
    /// and error object. Called during propagation when `MainThread` is
    /// popped from the gray list.
    ///
    /// Returns the estimated traversal cost, matching PUC-Rio's
    /// `sizeof(lua_State) + sizeof(TValue)*stacksize + sizeof(CallInfo)*size_ci`.
    fn traverse_main_thread(&mut self) -> usize {
        // Mark stack values up to top.
        let top = self.top;
        for i in 0..top {
            let val = self.stack[i];
            self.gc.mark_value(val);
        }

        // Mark open upvalues.
        let open_upvals: Vec<GcRef<Upvalue>> = self.open_upvalues.clone();
        for uv_ref in &open_upvals {
            self.gc.mark_upvalue(*uv_ref);
        }

        // Mark error object if present.
        if let Some(err_val) = self.error_object {
            self.gc.mark_value(err_val);
        }

        // Mark the debug hook function (PUC-Rio marks this in traversestack).
        // Without this, a collectgarbage() call from hooked code would sweep
        // the hook closure.
        let hook_func = self.hook.hook_func;
        self.gc.mark_value(hook_func);

        // Mark call stack function values.
        for ci_idx in 0..=self.ci {
            if ci_idx < self.call_stack.len() {
                let func_idx = self.call_stack[ci_idx].func;
                if func_idx < self.stack.len() {
                    let func_val = self.stack[func_idx];
                    self.gc.mark_value(func_val);
                }
            }
        }

        // Mark values in saved resumer threads. When coroutine.resume swaps
        // a coroutine's state into LuaState, the resumer's state is pushed
        // onto saved_threads. Without traversing these, the resumer's stack
        // values (closures, strings, etc.) are invisible to the GC and get
        // incorrectly swept during a full GC cycle.
        for saved in &self.saved_threads {
            for i in 0..saved.top {
                if i < saved.stack.len() {
                    self.gc.mark_value(saved.stack[i]);
                }
            }
            for uv_ref in &saved.open_upvalues {
                self.gc.mark_upvalue(*uv_ref);
            }
            for &(uv_ref, _) in &saved.suspended_upvals {
                self.gc.mark_upvalue(uv_ref);
            }
            if let Some(err_val) = saved.error_object {
                self.gc.mark_value(err_val);
            }
            self.gc.mark_value(saved.hook.hook_func);
            for ci_idx in 0..=saved.ci {
                if ci_idx < saved.call_stack.len() {
                    let func_idx = saved.call_stack[ci_idx].func;
                    if func_idx < saved.stack.len() {
                        self.gc.mark_value(saved.stack[func_idx]);
                    }
                }
            }
        }

        // Add to grayagain so the main thread is re-traversed in atomic.
        // This matches PUC-Rio's behavior where threads are always moved
        // to grayagain after traversal (they need re-scanning because
        // their stacks can be mutated between mark phases).
        self.gc.gc_state.grayagain.push(GrayItem::MainThread);

        // Cost: PUC-Rio returns sizeof(lua_State) + sizeof(TValue)*stacksize
        //       + sizeof(CallInfo)*size_ci.
        // We use EST_THREAD_SIZE for the base, 16 per stack slot (Val is
        // approximately TValue-sized), and 40 per CallInfo entry.
        EST_THREAD_SIZE + self.stack.len() * 16 + self.call_stack.len() * 40
    }

    /// Re-traverses the main thread for the atomic phase.
    ///
    /// PUC-Rio's `atomic()` calls `markobject(g, L)` to re-mark the
    /// running thread (line 536 in lgc.c). We do this before calling
    /// `Gc::atomic()` so the main thread's current stack values are
    /// marked before the final propagation.
    fn traverse_main_thread_for_atomic(&mut self) {
        let top = self.top;
        for i in 0..top {
            let val = self.stack[i];
            self.gc.mark_value(val);
        }

        let open_upvals: Vec<GcRef<Upvalue>> = self.open_upvalues.clone();
        for uv_ref in &open_upvals {
            self.gc.mark_upvalue(*uv_ref);
        }

        if let Some(err_val) = self.error_object {
            self.gc.mark_value(err_val);
        }

        // Mark the debug hook function.
        let hook_func = self.hook.hook_func;
        self.gc.mark_value(hook_func);

        for ci_idx in 0..=self.ci {
            if ci_idx < self.call_stack.len() {
                let func_idx = self.call_stack[ci_idx].func;
                if func_idx < self.stack.len() {
                    let func_val = self.stack[func_idx];
                    self.gc.mark_value(func_val);
                }
            }
        }

        // Mark saved resumer threads (same as traverse_main_thread).
        for saved in &self.saved_threads {
            for i in 0..saved.top {
                if i < saved.stack.len() {
                    self.gc.mark_value(saved.stack[i]);
                }
            }
            for uv_ref in &saved.open_upvalues {
                self.gc.mark_upvalue(*uv_ref);
            }
            for &(uv_ref, _) in &saved.suspended_upvals {
                self.gc.mark_upvalue(uv_ref);
            }
            if let Some(err_val) = saved.error_object {
                self.gc.mark_value(err_val);
            }
            self.gc.mark_value(saved.hook.hook_func);
            for ci_idx in 0..=saved.ci {
                if ci_idx < saved.call_stack.len() {
                    let func_idx = saved.call_stack[ci_idx].func;
                    if func_idx < saved.stack.len() {
                        self.gc.mark_value(saved.stack[func_idx]);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::state::LuaState;

    #[test]
    fn full_gc_does_not_crash() {
        let mut state = LuaState::new();
        let _ = state.full_gc();
    }

    #[test]
    fn full_gc_collects_unreachable_table() {
        let mut state = LuaState::new();

        let _orphan = state.gc.alloc_table(Table::new());
        let initial_count = state.gc.tables.len();

        let _ = state.full_gc();
        let after_count = state.gc.tables.len();

        assert!(
            after_count < initial_count,
            "expected orphan table to be collected: before={initial_count}, after={after_count}"
        );
    }

    #[test]
    fn full_gc_preserves_reachable_table() {
        let mut state = LuaState::new();

        let t = state.gc.alloc_table(Table::new());
        let key = state.gc.intern_string(b"mytable");
        if let Some(global) = state.gc.tables.get_mut(state.global) {
            let _ = global.raw_set(Val::Str(key), Val::Table(t), &state.gc.string_arena);
        }

        let _ = state.full_gc();

        assert!(
            state.gc.tables.is_valid(t),
            "reachable table should survive GC"
        );
    }

    #[test]
    fn full_gc_preserves_stack_values() {
        let mut state = LuaState::new();

        let s = state.gc.intern_string(b"hello gc");
        state.stack[1] = Val::Str(s);
        state.top = 2;

        let _ = state.full_gc();

        assert!(
            state.gc.string_arena.is_valid(s),
            "stack string should survive GC"
        );
    }

    #[test]
    fn gc_threshold_updates_after_collection() {
        let mut state = LuaState::new();
        state.gc.gc_state.gc_threshold = 0;
        state.gc.gc_state.total_bytes = 0;

        let _ = state.full_gc();

        assert!(
            state.gc.gc_state.gc_threshold >= 4096,
            "threshold should be at least 4096 after collection"
        );
    }

    #[test]
    fn gc_collects_unreachable_string() {
        let mut state = LuaState::new();

        let _s = state.gc.intern_string(b"unique_orphan_string_12345");
        let initial = state.gc.string_arena.len();

        let _ = state.full_gc();
        let after = state.gc.string_arena.len();

        assert!(
            after < initial,
            "orphan string should be collected: before={initial}, after={after}"
        );
    }

    #[test]
    fn gc_collects_unreachable_closure() {
        use crate::vm::closure::{Closure, RustClosure};

        let mut state = LuaState::new();

        let rc = RustClosure::new(|_| Ok(0), "orphan");
        let _orphan = state.gc.alloc_closure(Closure::Rust(rc));
        let initial = state.gc.closures.len();

        let _ = state.full_gc();
        let after = state.gc.closures.len();

        assert!(
            after < initial,
            "orphan closure should be collected: before={initial}, after={after}"
        );
    }

    #[test]
    fn gc_preserves_closure_on_stack() {
        use crate::vm::closure::{Closure, RustClosure};

        let mut state = LuaState::new();

        let rc = RustClosure::new(|_| Ok(0), "on_stack");
        let cl = state.gc.alloc_closure(Closure::Rust(rc));
        state.stack[1] = Val::Function(cl);
        state.top = 2;

        let _ = state.full_gc();

        assert!(
            state.gc.closures.is_valid(cl),
            "stack closure should survive GC"
        );
    }
}
