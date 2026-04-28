# Smart Agent Coordination — Design Spec

**Date:** 2026-04-28
**Status:** Approved
**Author:** opencode (brainstorming session)

## Problem

The current Mentor→Executor→Review cycle uses a fixed iteration loop with static handoff prompts, causing:
1. **Review quality varies** — Mentor may approve superficially without structured evidence
2. **Wasteful cycles** — Full iterations run even on simple tasks that are already complete
3. **Context loss** — Plan details get diluted over iterations; Executor loses track of original intent
4. **No smart stopping** — Agents don't know when the job is truly done

## Solution: Four Coordination Pillars

### 1. Adaptive Stopping (`AdaptiveStop`)

Computes a dynamic iteration budget based on task complexity after each Executor turn.

**Inputs:**
- Number of files changed
- Lines of diff (small <20, medium 20-100, large >100)
- File types touched (config, business logic, tests)

**Budget tiers:**
- No changes: 1 cycle (verify and finish)
- Simple (≤5 files, <20 lines): 3 cycles
- Medium (≤15 files, <100 lines): 6 cycles
- Complex (>15 files, >100 lines): 10 cycles
- Falls back to `max_iterations` from pair config if computation fails

**Location:** New `AdaptiveStop` module in `src-tauri/src/`; integrated into `message_broker.rs`

### 2. Review Quality Gate (`QualityGate`)

Mentor must produce structured evidence before a verdict is accepted.

**Required evidence:**
- List of files reviewed (must match git-tracked changes)
- Specific checks performed (naming, error handling, edge cases, type safety)
- Quote or reference of changed code being validated

**Validation:**
- If evidence is missing or weak → auto-reject with specific feedback
- Parse failures fall back to "insufficient review detail" rejection
- Verdict parsing enhanced, not replaced

**Location:** Enhanced `parse_mentor_verdict()` in `message_broker.rs`

### 3. Context Preservation (`ContextBridge`)

Each handoff includes a structured summary anchor so context is never lost.

**Handoff payload:**
- Original task spec (always present)
- Current sub-goal (what this turn should accomplish)
- Progress summary (what's been done, what remains)
- Key decisions made so far
- Mentor's plan extracted into a structured checklist

**Prompt integration:**
- Mentor receives: task spec + progress summary + checklist template
- Executor receives: task spec + Mentor's checklist + current sub-goal
- Review receives: task spec + checklist + Executor's changes + diff summary

**Location:** New `build_handoff_context()` in `message_broker.rs`; prompt templates updated in `process_spawner.rs` or provider adapter

### 4. Smart Pause Triggers (`SmartPause`)

Pauses for human intervention when specific conditions are met, beyond just iteration count.

**Pause conditions:**
- Budget exhausted AND task not complete
- Executor changed files not in Mentor's plan (scope drift detection)
- Risk level changed mid-execution (new backend files added)
- Stalled activity >60s (existing, unchanged)

**Pause message:** Tells human exactly why and what to decide

**Location:** Integrated into existing `max_iterations` check in `pair_manager.rs` and `message_broker.rs`

## Data Flow

```
Task Spec → AdaptiveStop.compute_budget() → ContextBridge.build_handoff()
    → Mentor Turn (plan as structured checklist)
    → Executor Turn → diff analysis → AdaptiveStop.update()
    → Review (QualityGate validates evidence) → Verdict
    → SmartPause check → (loop | finish | pause)
```

## Error Handling & Degradation

All smart features degrade gracefully to current behavior:
- `compute_adaptive_budget()` failure → fall back to `max_iterations`
- Evidence extraction failure → reject with "insufficient review detail"
- Diff computation failure → conservative (stay in loop, don't prematurely finish)
- Context build failure → fall back to existing handoff prompt

## Testing

- Unit tests for `compute_adaptive_budget()` with varied diff scenarios
- Unit tests for `parse_mentor_verdict()` with evidence validation
- Unit tests for `build_handoff_context()` structure
- Integration test: end-to-end handoff cycle with adaptive stopping

## Files Changed

| File | Change |
|------|--------|
| `src-tauri/src/adaptive_stop.rs` | New: adaptive budget computation |
| `src-tauri/src/context_bridge.rs` | New: handoff context builder |
| `src-tauri/src/quality_gate.rs` | New: review evidence validation |
| `src-tauri/src/smart_pause.rs` | New: pause trigger logic |
| `src-tauri/src/message_broker.rs` | Modify: integrate all four pillars |
| `src-tauri/src/pair_manager.rs` | Modify: smart pause integration |
| `src-tauri/src/lib.rs` | Modify: register new modules |
| `src-tauri/Cargo.toml` | Modify: if new deps needed |
| Tests for all new modules | New unit tests |
