# Skill Editor Open Design

## Goal

Support opening an installed Skill directory from Skills Management in an external editor such as Zed or VS Code, with automatic local editor discovery.

## Evidence

- Installed Skills are rendered from `src/components/skills/UnifiedSkillsPanel.tsx`.
- Installed Skill records expose `id` and `directory`, but not an absolute path.
- The SSOT directory is resolved in `SkillService::get_ssot_dir`.
- Existing folder-opening behavior uses Tauri opener APIs in `open_config_folder`.

## Design

The default path is: resolve the installed Skill by `id`, join its `directory` under the active SSOT directory, verify the directory exists and contains `SKILL.md`, then open that directory with the first available editor in the preferred order `zed`, `code`, `cursor`.

The fallback path is: if no supported editor is detected or the caller asks for the system opener, open the directory with the OS default folder opener. This fallback is only for editor unavailability, not for missing or malformed Skill directories.

The frontend adds a compact action to each installed Skill row. Clicking it opens the Skill with the auto-selected editor. A small menu lets the user pick another detected editor or the system folder opener.

## Verification

- Rust unit tests cover editor detection order and Skill path validation.
- TypeScript typecheck verifies the new API and UI wiring.
