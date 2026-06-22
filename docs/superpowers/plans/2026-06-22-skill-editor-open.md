# Skill Editor Open Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Skills Management action that opens an installed Skill directory in a detected external editor.

**Architecture:** Backend owns path resolution, editor detection, and command execution. Frontend only requests available open targets and invokes a selected target for a Skill id.

**Tech Stack:** Tauri v2 commands, Rust service helpers, React, TypeScript, React Query.

---

### Task 1: Backend Test Surface

**Files:**
- Modify: `src-tauri/src/services/skill.rs`

- [ ] Add tests for editor choice order, explicit editor lookup, system fallback, and Skill directory validation.
- [ ] Run `codex-quiet-run -- cargo test skill_editor --lib` and confirm the tests fail because helpers are missing.

### Task 2: Backend Implementation

**Files:**
- Modify: `src-tauri/src/services/skill.rs`
- Modify: `src-tauri/src/commands/skill.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] Add serializable editor/open target types.
- [ ] Add service helpers for available targets and validated Skill directory resolution.
- [ ] Add Tauri commands `get_skill_open_targets` and `open_skill_directory`.
- [ ] Register the commands.
- [ ] Run `codex-quiet-run -- cargo test skill_editor --lib` and confirm the tests pass.

### Task 3: Frontend Wiring

**Files:**
- Modify: `src/lib/api/skills.ts`
- Modify: `src/components/skills/UnifiedSkillsPanel.tsx`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/zh-CN.json`
- Modify: `src/i18n/locales/zh-TW.json`
- Modify: `src/i18n/locales/ja.json`

- [ ] Add API wrappers and types for open targets.
- [ ] Add an icon action and editor menu per installed Skill row.
- [ ] Add localized labels and toast messages.
- [ ] Run `codex-quiet-run -- pnpm typecheck`.

### Task 4: Final Verification

**Files:**
- Review all modified files.

- [ ] Run targeted Rust tests.
- [ ] Run TypeScript typecheck.
- [ ] Check `git diff --stat` and inspect the changed hunks.
