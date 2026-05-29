# Documentation Consolidation Plan

**Current Status:** 26 markdown files (some redundant)  
**Goal:** Consolidate while preserving all critical information

---

## Files Status Review

### 🟢 Core Documentation (Keep As-Is)

**Language Specification:**
- SPEC.md (523 lines) — Primary language spec
- GRAMMAR.md (336 lines) — EBNF grammar
- LANGUAGE_SPEC.md (149 lines) — Can be merged with SPEC.md

**Type System & Architecture:**
- TYPE_SYSTEM.md (309 lines)
- RUST_BACKEND.md (774 lines) — Critical compilation mapping
- IR_INVARIANTS.md (375 lines)
- RUNTIME_ABI.md (315 lines)

**Design & Philosophy:**
- DESIGN_DECISIONS.md (437 lines)
- ERRORS.md (418 lines)
- PYTHON_COMPATIBILITY.md (298 lines)

**Project Planning:**
- DEVELOPMENT_PLAN.md (387 lines) — ✅ JUST UPDATED
- PROJECT_STATUS.md (new) — Replaces SUMMARY_5_28_26.md

---

## 🟡 Redundant Phase Docs (Consolidate)

### Phase 13 Consolidation
**Current State:**
- PHASE_13_PLAN.md (184 lines) — Strategy document
- PHASE_13_PROGRESS.md (182 lines) — Phase 13.1 progress
- PHASE_13_COMPLETION.md (316 lines) — Phase 13.1 completion
- PHASE_13_2_COMPLETION.md (345 lines) — Phase 13.2 completion
- **Total:** 1,027 lines (highly redundant)

**Recommendation:**
→ **Create: PHASES_13_COMPLETION.md** (consolidate all into single document with sections for 13.1 and 13.2)
→ **Delete:** PHASE_13_PLAN.md (no longer needed, work is complete)
→ **Delete:** PHASE_13_PROGRESS.md (superseded by completion doc)

**Estimated Result:** 700 lines → 500 lines (27% reduction)

### Phases 9-12 Consolidation (Already Done)
- PHASES_7_8_COMPLETION.md (329 lines) — Good (Phase 7 & 8 together)
- PHASE_9_COMPLETION.md (231 lines) — Standalone
- PHASE_10_PROGRESS.md (192 lines) — Standalone
- PHASE_11_12_COMPLETION.md (340 lines) — Good (Phase 11 & 12 together)

**Recommendation:**
→ Leave as-is (already reasonably consolidated)
→ Could optionally create: PHASES_9_12_SUMMARY.md with brief overviews of all four

---

## 🔴 Outdated Documents (Replace)

### SUMMARY_5_28_26.md (255 lines)
- **Status:** Obsolete (Phase 7 only, now at Phase 13.2)
- **Action:** Replace with PROJECT_STATUS.md (already created)
- **Keep:** Historical value only (archive if desired)

---

## ✅ Well-Maintained (No Changes)

- IMPLEMENTATION_SUMMARY.md (344 lines)
- TEST_RESULTS.md (317 lines)
- AGENTS.md (367 lines)
- README.md (165 lines)

---

## Consolidation Timeline

### Option 1: Quick Consolidation (Recommended)
**Time:** 30 minutes

1. Delete PHASE_13_PLAN.md (no longer needed)
2. Delete PHASE_13_PROGRESS.md (superseded)
3. Rename PHASE_13_COMPLETION.md → PHASES_13_COMPLETION.md
4. Add Phase 13.2 content to PHASES_13_COMPLETION.md
5. Delete PHASE_13_2_COMPLETION.md
6. Update README.md to link to new consolidated files

**Result:** 26 → 23 files (3 files eliminated)

### Option 2: Comprehensive Consolidation (Future)
**Time:** 2 hours

Do Option 1, plus:
1. Merge LANGUAGE_SPEC.md into SPEC.md
2. Create PHASES_9_12_SUMMARY.md with overview sections
3. Create "Phase Completion Logs" index document
4. Archive old summary docs in `docs/archived/`

**Result:** 23 → 20 files (more organized, full index)

---

## File Organization After Consolidation

### Recommended Structure

```
Root .md files (active):
  README.md
  PROJECT_STATUS.md (current status)
  DEVELOPMENT_PLAN.md (roadmap)
  
Specifications:
  SPEC.md (or LANGUAGE_SPEC.md + SPEC.md merged)
  GRAMMAR.md
  TYPE_SYSTEM.md
  
Implementation:
  RUST_BACKEND.md
  DESIGN_DECISIONS.md
  ERRORS.md
  IR_INVARIANTS.md
  RUNTIME_ABI.md
  
Phase Documentation:
  PHASES_7_8_COMPLETION.md
  PHASES_9_12_COMPLETION.md (or separate 9, 10, 11_12)
  PHASES_13_COMPLETION.md (consolidated)
  
Testing & Tools:
  TEST_RESULTS.md
  IMPLEMENTATION_SUMMARY.md
  AGENTS.md
  
Performance:
  PHASE_10_BENCHMARKS.md
  PYTHON_COMPATIBILITY.md
```

---

## Recommendation

**Go with Option 1 (Quick Consolidation):**
- Eliminates redundancy immediately
- Takes 30 minutes
- Keeps all information
- Can do Option 2 later if needed
- Ready for Phase 14 development

**Actions:**
1. ✅ Create PROJECT_STATUS.md (done)
2. ✅ Update DEVELOPMENT_PLAN.md (done)
3. ⏳ Delete PHASE_13_PLAN.md
4. ⏳ Delete PHASE_13_PROGRESS.md
5. ⏳ Rename PHASE_13_COMPLETION.md → PHASES_13_COMPLETION.md
6. ⏳ Move Phase 13.2 content to consolidated doc
7. ⏳ Delete PHASE_13_2_COMPLETION.md
8. ⏳ Update README.md to reference new structure

---

## Summary

**Current:** 26 files with some redundancy  
**After Option 1:** 23 files, cleanly organized  
**After Option 2:** 20 files, fully organized with index  

**Recommended:** Do Option 1 now, revisit Option 2 after Phase 14 completion.
