# Window Hug: Content-Hugging, Ratio-Locked Window (Windows First) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking. Where a skill says to dispatch `general-purpose`, dispatch the role agent this plan names, without `model` and without `name`.

**Goal:** Build the approved window-hug spec in its section 8 order, so the window's client area always has the content's shape (980 : H), keeps that ratio during any edge or corner drag on Windows, follows content-height changes in one resize without a zoom flash, and scales the fixed 980-local-px layout by `min(w/980, h/C)` when maximized or capped.

**Acceptance criteria.** Each item passes or fails.
1. Every task's commit passes the five gates (below) with `exit=0`, with logs in `.claude-work/window-hug/logs/`.
2. Every test named in spec §7.1, §7.2 and §7.3 exists under that name and passes, or under the name recorded as a deviation in that task's REPORT.
3. Each task's "Done when" grep returns exactly what the task states.
4. M10 (Task 9): S1-S6 pass, every H1, H2 and H4-H12 answer is recorded yes or no in `.claude-work/window-hug/manual/M10.md`, and the window-state file is restored from its backup.
5. Spec §1 goal items 1-7 each map to a passing automated test or a passing M10 check (table "Goal coverage" at the end of this plan).
6. `git diff master..followups -- package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json` shows no `version` change, and no tag or release is created.
7. The `hostile-reviewer` whole-plan review after Task 10 returns 0 Critical and 0 Important.

**Architecture.** One number crosses the boundary in each direction (spec §3).
- Frontend: `.app` is a fixed 980-local-px, content-sized box. `subscribeContentHeight` (`src/lib/contentHeight.ts`) reports its ResizeObserver border-box height as `Math.ceil(h - 1/64)`. `createFitController` and `fitReducer` (`src/lib/fit.ts`, no tauri import) send it through `backend().setContentHeight` and own the zoom's content height `zoomContentH`. `windowZoom(w, h, contentH)` is `clamp(min(w/980, h/contentH))`.
- Rust shared (`src-tauri/src/window_aspect.rs`): constants, `AspectState` (atomics), and pure, table-tested decisions (`validate_content_h`, `decide_fit`, `client_bounds`, `fit_client`, `plan_fit`, `step`, `fit_rect`, `run_guarded`, `handle_report`). The `set_content_height` command and the `window-aspect` plugin are thin glue over them.
- Rust Windows (`src-tauri/src/platform/windows/aspect.rs`): a `SetWindowSubclass` proc rewrites the `WM_SIZING` rect through `fit_rect`, and applies the pending fit on `WM_EXITSIZEMOVE`, `WM_SIZE`/`SIZE_RESTORED` and `WM_DPICHANGED`. `hwnd_facts` and `apply_hwnd` are shared by the proc and the command. `platform/other.rs` compiles the same interface with Tauri's portable API.

**Rejected alternatives** (spec §3.0, not re-argued here): the null design (no lock, fit on release only) snaps back on a bottom-edge drag and needs `WM_SIZING` anyway; measuring `.app-inner` and dividing by the zoom can never shrink the window and races a stale zoom; a width-following layout oscillates near wrap thresholds; a Rust "fitted" event duplicates the command's return value; moving the existing `cfg(windows)` domain code under `platform/` splits concepts across owners (R8).

**Tech stack.** Rust 2021, Tauri 2.12.1, tao 0.37.1, windows-sys 0.61 (no new crate), tracing; React 19 + TypeScript, Vite, vitest (node environment); PowerShell for the M10 harness.

**Spec:** `docs/superpowers/specs/2026-10-06-window-hug-design.md` (sections 1-8; sections 9-11 are review history). **Rulings (binding):** `.claude-work/window-hug/review/RULINGS-window-hug.md` (R1-R29). Section numbers below (§) are spec sections.

## Global Constraints

### Branch and release

- All work is on `followups`; continue on it. Do not create another branch, do not open or merge a PR, do not bump any version, do not tag or release. ClaudeUsageTracker is public: merging waits for Josh.
- Each task is one commit on `followups` and leaves every gate green.

### The five gates

Run them at the end of every task, in this order, one at a time (never in parallel), each to its own log. `<n>` is the task number. The cargo gates run from Git Bash; npm and npx run from PowerShell only (never from Git Bash), through `cmd /c` so the redirection writes plain bytes and `$LASTEXITCODE` carries the exit code.

```bash
mkdir -p .claude-work/window-hug/logs
cargo test --manifest-path src-tauri/Cargo.toml > .claude-work/window-hug/logs/T<n>-test.log 2>&1; echo "exit=$?"; tail -40 .claude-work/window-hug/logs/T<n>-test.log
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings > .claude-work/window-hug/logs/T<n>-clippy.log 2>&1; echo "exit=$?"; tail -40 .claude-work/window-hug/logs/T<n>-clippy.log
```

```powershell
cmd /c "npm test > .claude-work\window-hug\logs\T<n>-vitest.log 2>&1"; "exit=$LASTEXITCODE"; Get-Content -Tail 40 .claude-work\window-hug\logs\T<n>-vitest.log
cmd /c "npm run build > .claude-work\window-hug\logs\T<n>-build.log 2>&1"; "exit=$LASTEXITCODE"; Get-Content -Tail 40 .claude-work\window-hug\logs\T<n>-build.log
```

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --check > .claude-work/window-hug/logs/T<n>-fmt.log 2>&1; echo "exit=$?"; tail -40 .claude-work/window-hug/logs/T<n>-fmt.log
```

- `npm test` is `vitest run`; `npm run build` is `tsc && vite build`, so it is the typecheck gate.
- A gate passes only with `exit=0`. A skipped or ignored test counts as failing.
- There is no `.github` and no `.husky`: these five commands are the whole gate. Never pass `--no-verify`.

### Test-driven development

- Write the task's named tests first. Run only those tests and record the expected failure in `.claude-work/window-hug/logs/T<n>-red.log`. Then implement, run them green, then run the five gates, then commit.
- Targeted red runs: `cargo test --manifest-path src-tauri/Cargo.toml <filter> > .claude-work/window-hug/logs/T<n>-red.log 2>&1` (bash); `cmd /c "npx vitest run <file> > .claude-work\window-hug\logs\T<n>-red.log 2>&1"` (PowerShell).
- The red reason must be the one the task names. A red run that fails for another reason (a typo, a missing fixture) is fixed before implementing.

### Code rules

- TS: no `any`, no `!` non-null assertions, no silent catches; every async call handles its error; every timer, observer and listener is cleaned up.
- Rust: no `clamp`, no `unwrap()`, no `expect(` outside `mod tests` in `window_aspect.rs` and `platform/` (bounds use ordered `min`/`max` steps; `f64::clamp` and `i32::clamp` panic when min > max). Nothing panics out of an `extern "system"` frame.
- `-D warnings` applies. Fix an unused item by using or removing it, never by adding an `allow`. Loosening a lint, type or format setting is a finding.
- Never `sed -i` a tracked file. Use the Edit tool, or a binary-safe Python script that preserves line endings.

### Load-bearing invariants

- **One owner per value.** 980 is `BASE_WIDTH` (TS) and `BASE_WIDTH_PX` (Rust); 735 is `BASE_WIDTH * ZOOM_MIN` (TS) and `MIN_WIDTH_PX` (Rust); 2.5 is `ZOOM_MAX` in each. Each has a drift test against `src-tauri/tauri.conf.json` (R11). The `FitOutcome` strings have a drift test between `fit.ts` and Rust (Task 8).
- **One decision path.** `step()` is the only caller of `decide_fit` and `plan_fit` outside tests; the command, `fit_now` and the proc all go through it. Every non-`Fit` action marks pending (R4); every apply failure re-marks pending (R19).
- **One apply.** `apply_hwnd` is the only `SetWindowPos` for the fit; Tauri `set_size` is never called from inside the proc.
- **One owner of `FitState`.** `createFitController` applies `fitReducer` and publishes; there is no React `useReducer` copy.
- **Main thread.** The command (sync), the ready hook and the proc run on the main thread; no `SWP_ASYNCWINDOWPOS` (R6).

### Dependencies

- No new crate and no new npm package. Task 5 adds windows-sys features only.
- `ignore-scripts=true` stays on. Prefer `npm ci`; no task changes `package-lock.json`.
- Never print or log a token.

### Machine resources

- `NODE_OPTIONS=--max-old-space-size=8192` is set in the session environment. Never raise or unset it. A test that needs more memory is the bug.
- One writer at a time: one implementer edits and runs gates; a reviewer reads only and runs no tests while gates run.
- The HWND tests (Tasks 5, 6) create hidden windows only; they never show a window or touch the app's real window.

### Agents and reports

- Dispatch role agents with no `model` and no `name`. No sub-agents.
- Every dispatch names REPORT (`.claude-work/window-hug/reports/T<n>.md`), TASK (this file, Task n) and FILES (the task's Files list), and carries the interfaces earlier tasks actually landed.
- Implementers may deviate from this plan with a note in REPORT (renamed test, moved line anchor). A design that fails its own listed expectations returns to the orchestrator; it is never tuned until green.

### Review tiers

- Tasks 1-8 (logic, including the Windows proc and the pending/apply paths of Tasks 5, 6 and 7): per-task review by `reviewer`, given the task diff as a file (`git diff HEAD~1..HEAD > .claude-work/window-hug/review/T<n>.diff`) and the rulings file.
- Task 9 (tests-only harness, untracked) and Task 10 (docs): no per-task review; the whole-plan review is their net.
- After Task 10: whole-plan review by `hostile-reviewer` over `git diff c683bec..HEAD`, then a scoped re-review of every fix diff.

### Commit procedure C

Every task's last step uses this procedure. `<files>` is the task's Files list (deleted files are staged with `git add` on the path too).

```bash
git add <files>
git diff --cached --stat > .claude-work/window-hug/logs/T<n>-stat.txt
git diff --cached --stat --ignore-cr-at-eol > .claude-work/window-hug/logs/T<n>-stat-nocr.txt
diff .claude-work/window-hug/logs/T<n>-stat.txt .claude-work/window-hug/logs/T<n>-stat-nocr.txt && echo "no CR flip"
git ls-files --eol <each added file>   # every line must show i/lf
git commit -F .claude-work/window-hug/logs/T<n>-msg.txt
```

- Any difference between the two stat files is a line-ending flip. So is `i/crlf` or `i/mixed` on an added file. Fix it with a byte-level pass before committing.
- The message file holds the subject line given in the task, a blank line, and these two lines:

  ```
  Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01H2gG36Wwd1qacRcewC3y2y
  ```
- Never pass `--no-verify`. The commit contains only the task's Files.

## Tasks

### Task 1: Shell box, modal floor, fixed chart height

**Spec:** §3.1 "Shell CSS", "`--viewport-h`", "History chart"; §7.1 css-contract and layout.test rows. **Rulings:** R1, R13 (tests M4, arch M6). **Agent:** `implementer`. **Tier:** logic (`reviewer`). **Risk:** low. **Depends on:** nothing.

**Why:** the measured box (`.app`) must be content-sized and exactly 980 local px wide before anything reports its height, and the drawer chart must stop depending on the window height, or the content height would depend on the window and the fit would oscillate.

**Files:** `src/styles.css` (`.app` line 40, `.chart` line 226, `.modal-body` line 317), `src/lib/layout.ts` (`shellVars`, new `CHART_HEIGHT`, `ROW_BORDER` doc comment at line 17), `src/lib/layout.test.ts`, `src/lib/css-contract.test.ts`, `src/components/HistoryDrawer.tsx`, `src/components/AccountRow.tsx` (lines 24, 43, 47, 88, 108), `src/components/AccountsTable.tsx` (the `<AccountRow zoom=...>` prop near line 241); deleted: `src/hooks/useChartHeight.ts`, `src/lib/chart.ts`, `src/lib/chart.test.ts`.

**Interfaces:** Consumes: nothing. Produces: `shellVars()` emits `--base-w: "980px"` (from `BASE_WIDTH`) and `--chart-h: "220px"` (from `CHART_HEIGHT = 220`); `.app` is `width: var(--base-w); margin: 0 auto`; `HistoryDrawer` has no `zoom` or `rowRef` prop; `AccountRow` has no `zoom` prop.

- [ ] **Step 1: Tests first.**
  - `src/lib/css-contract.test.ts`:
    - Replace `.app min-height is the viewport variable` (line 37) with `.app is content-sized and fixed-width`: `decl(rule(".app"), "height")` and `decl(rule(".app"), "min-height")` are undefined, `width` is `var(--base-w)`, `margin` is `0 auto`.
    - Replace `.modal-body max-height is 0.8 of the viewport variable` (line 40) with `.modal-body max-height is the window less the gutters`: `calc(var(--viewport-h) - 2 * var(--gutter))`.
    - Invert the chart assertion at lines 81-82 into `.chart height reads --chart-h`: `decl(rule(".chart"), "height") === "var(--chart-h)"`.
    - `ASSERTED_VARS` (line 21) gains `"--base-w"` and `"--chart-h"`.
  - `src/lib/layout.test.ts`: the `shellVars` `toEqual` gains `"--base-w": "980px"` and `"--chart-h": "220px"`.
  - Run `cmd /c "npx vitest run src/lib/css-contract.test.ts src/lib/layout.test.ts > .claude-work\window-hug\logs\T1-red.log 2>&1"`.
  - Expected red: the three css-contract rows fail on the current rules (`min-height: var(--viewport-h)` present, `.chart` height not `var(--chart-h)`, `.modal-body` `calc(0.8 * ...)`); the `ASSERTED_VARS` row and `shellVars` `toEqual` fail because `--base-w` and `--chart-h` are not emitted.
- [ ] **Step 2: Implement.**
  - `src/lib/layout.ts`: add `export const CHART_HEIGHT = 220;` (local px) with a doc comment; `shellVars()` emits `--base-w: \`${BASE_WIDTH}px\`` and `--chart-h: \`${CHART_HEIGHT}px\``. Rewrite the `ROW_BORDER` comment as "the reorder drag stride adds it to ROW_HEIGHT" (AccountsTable.tsx:177).
  - `src/styles.css:40`: `.app { width: var(--base-w); margin: 0 auto; padding: var(--gutter); display: flex; font-family: var(--ui); }` (no `min-height`, no `height`). `.app-inner` unchanged.
  - `.chart { height: var(--chart-h); ... }` keeping its other declarations.
  - `.modal-body { max-height: calc(var(--viewport-h) - 2 * var(--gutter)); ... }` keeping its other declarations. `shellStyle` keeps emitting `--viewport-h`.
  - Delete `src/hooks/useChartHeight.ts`, `src/lib/chart.ts`, `src/lib/chart.test.ts`.
  - `HistoryDrawer.tsx`: remove the `useChartHeight` import (line 3) and call (line 61), the `zoom` and `rowRef` props (lines 21-24, 51) and any import only they used.
  - `AccountRow.tsx`: remove the `zoom` prop (24, 43) and `rowRef` (47, the `ref={rowRef}` at 88, the props at 108).
  - `AccountsTable.tsx`: keep `zoom` and `zoomRef` (the reorder drag stride) and stop passing `zoom` to `<AccountRow>`.
- [ ] **Step 3:** Re-run the Step 1 command to `T1-green.log`. Expected: all pass.
- [ ] **Step 4:** Run the five gates (`<n>` = 1). Expected: all `exit=0`.
- [ ] **Step 5:** `grep -rn "useChartHeight\|chartHeightPx" src` matches nothing.
- [ ] **Step 6:** Commit (procedure C): `Window hug 1: content-sized .app at 980 local px, modal floor, fixed chart height`.

**Interim behaviour:** the window keeps its current free shape; short content leaves `body` background below it; the drawer chart is 220 local px tall at every window size. This is expected until Task 8.

**Done when:** Steps 3-5 hold and the commit contains only the Files list.

---

### Task 2: The two-term zoom and the window config

**Spec:** §3.1 "`windowZoom(widthPx, heightPx, contentH)`", "Window config"; §7.1 layout.test rows. **Rulings:** R11, goal items 4-5. **Agent:** `implementer`. **Tier:** logic (`reviewer`). **Risk:** low. **Depends on:** Task 1.

**Why:** the zoom needs the content-height term to scale a maximized or capped window, and `tauri.conf.json` must hold 735 and no `minHeight` before Task 4's Rust drift test reads it.

**Files:** `src/lib/layout.ts` (`windowZoom` line 92, delete `BASE_HEIGHT` line 81), `src/lib/layout.test.ts`, `src/App.tsx` (line 23), `src-tauri/tauri.conf.json` (lines 19-20).

**Interfaces:** Consumes: `BASE_WIDTH`, `ZOOM_MIN = 0.75`, `ZOOM_MAX = 2.5` (layout.ts:83-84). Produces: `export function windowZoom(widthPx: number, heightPx: number, contentH: number | null): number`; config `minWidth: 735`, no `minHeight`.

- [ ] **Step 1: Tests first** in `src/lib/layout.test.ts`.
  - Replace the rows of `describe("windowZoom")` (lines 21-36) with:
    - `fitted window: both terms agree`: `windowZoom(1470, 960, 640)` is 1.5;
    - `taller than needed: width term wins`: `(1470, 980, 640)` is 1.5;
    - `maximized: height term wins`: `(1920, 1032, 900)` is `1032 / 900` (`toBeCloseTo(.., 10)`);
    - `null contentH: width term only`: `(1470, 300, null)` is 1.5;
    - `bad contentH falls back to the width term`: each of `0`, `-1`, `NaN`, `Infinity` at `(1470, 300, c)` is 1.5;
    - `bad width or height gives 1`: the existing non-finite and non-positive cases, each with a third argument `640`;
    - `clamps`: `(500, 2000, 640)` is `ZOOM_MIN`, `(4000, 4000, 640)` is `ZOOM_MAX`.
  - Replace `the base canvas is the window configured in tauri.conf.json` (line 49, which asserts `BASE_HEIGHT`) with `the window config matches the base canvas`: `conf.app.windows[0]`, read the same way (`readFileSync` of `../../src-tauri/tauri.conf.json`), has `width === BASE_WIDTH`, `minWidth === BASE_WIDTH * ZOOM_MIN` and `minHeight === undefined`.
  - Remove `BASE_HEIGHT` from the import list (line 6).
  - Run `cmd /c "npx vitest run src/lib/layout.test.ts > .claude-work\window-hug\logs\T2-red.log 2>&1"`.
  - Expected red: `maximized: height term wins`, `null contentH: width term only` and `bad contentH falls back to the width term` fail (the current height term divides by 640, giving 0.75 at height 300); `the window config matches the base canvas` fails on `minWidth` 360 and `minHeight` 240.
- [ ] **Step 2: Implement.**
  - `src/lib/layout.ts`: delete `BASE_HEIGHT`. `windowZoom(widthPx, heightPx, contentH)`: a non-finite or `<= 0` width or height returns 1; a `contentH` that is null, non-finite or `<= 0` gives `widthPx / BASE_WIDTH`; otherwise `Math.min(widthPx / BASE_WIDTH, heightPx / contentH)`; every result is `Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, x))`. Update the doc comment (layout.ts:86-91) to say this.
  - `src/App.tsx:23`: `windowZoom(viewport.width, viewport.height, null)` (Task 8 replaces `null` with `contentH`).
  - `src-tauri/tauri.conf.json`: `"minWidth": 735`, delete the `"minHeight"` line; `width` 980 and `height` 640 stay.
- [ ] **Step 3:** Re-run the Step 1 command to `T2-green.log`. Expected: all pass.
- [ ] **Step 4:** Run the five gates (`<n>` = 2). Expected: all `exit=0`.
- [ ] **Step 5:** `grep -rn "BASE_HEIGHT" src` matches nothing.
- [ ] **Step 6:** Commit (procedure C): `Window hug 2: two-term zoom against the content height; minWidth 735, no minHeight`.

**Interim behaviour:** with `null`, the zoom is the width term only, so a window shorter than its content scrolls at the root until Task 8; the window can no longer be dragged narrower than 735 logical px.

**Done when:** Steps 3-5 hold and the commit contains only the Files list.

---

### Task 3: Remove the narrow and cards layouts

**Spec:** §3.1 "Layouts"; §7.1 css-contract, present.test and gauge.test rows; §8 task 3. **Rulings:** R7, R25, R26. **Agent:** `implementer`. **Tier:** logic (`reviewer`). **Risk:** medium: a wide deletion across components. **Depends on:** Task 1 (`shellVars` and `ASSERTED_VARS` were edited there).

**Why:** the local width is exactly 980 in every state after Task 1, so `layoutFor` never returns `narrow` or `cards` and their code is dead. Dead branches would mislead the next developer and keep CSS the contract tests must still guard. Flagged to Josh as reversible (R7).

**Files:** `src/lib/layout.ts` (`layoutFor`, `Layout`, `BREAKPOINTS`, `NARROW_HIDDEN`, `autoHiddenColumns`, `SHELL_PADDING`, the `--ring-min`/`--ring-max` entries in `shellVars`), `src/lib/layout.test.ts`, `src/App.tsx` (cards branch lines 80-85, the `layout` prop passed to Settings, the import at line 15), `src/components/AccountCard.tsx` (deleted), `src/components/Ring.tsx` (the `md` variant), `src/lib/gauge.ts`, `src/lib/gauge.test.ts` (`RING`, `RING_SIZES.md`), `src/components/Header.tsx` (`compact`), `src/lib/present.ts`, `src/lib/present.test.ts` (`countPlacement`'s `compact` parameter), `src/components/Settings.tsx` (line 6 import, the `layout` prop, the width hints at lines 152-182), `src/styles.css` (card and ring rules at lines 197-207), `src/lib/css-contract.test.ts`.

**Interfaces:** Consumes: Task 1's `shellVars` and `ASSERTED_VARS`. Produces: `countPlacement(...)` without `compact`; `RING_SIZES` with `sm` only; `Ring` with the `sm` variant only; `Header` and `Settings` without `compact`/`layout` props; `shellVars()` without ring variables.

- [ ] **Step 1: Tests first.**
  - `src/lib/css-contract.test.ts`:
    - Add `no card-layout rules remain`: the styles.css text does not match `/^\.(cards|card|card-rings|ring|ring-label|ring-value)(?![\w-])/m` (the lookahead lets `.ring-sm` through; `\b` would not).
    - Delete `.card has no border-radius (flat look)`, `.ring width clamps between the ring variables`, `.card-rings is an inline-size container`, `.ring-label may use the full ring width`.
    - Rewrite `.ring svg fills its wrapper` as `.ring-sm svg fills its wrapper`, selecting the rule by its full selector text `.ring-sm svg`, with the same declaration assertions.
    - Keep `.ring-sm is a fixed 20px glyph` and the "no viewport units" test unchanged.
    - `ASSERTED_VARS` loses `"--ring-min"` and `"--ring-max"`.
  - `src/lib/layout.test.ts`: the `shellVars` `toEqual` loses the ring variables; delete the `layoutFor`, `autoHiddenColumns`, `BREAKPOINTS` and `SHELL_PADDING` describes and their imports.
  - `src/lib/present.test.ts`: `countPlacement` rows call it without `compact`; delete the three `compact === true` rows.
  - `src/lib/gauge.test.ts`: re-point the `ringDash` describe at `c = 2 * Math.PI * RING_SIZES.sm.radius`, keeping its six rows (null, 0, 50, 100, 140 clamped to 100, -5 clamped to 0) with `RING_SIZES.sm.radius` instead of `RING.radius`; delete `ring geometry fits the 44px box with a 5px stroke`; rename `keeps the md geometry and adds a 20px variant` to `sm is the 20px variant`, keeping only its `sm` line; delete the `ringViewBox(RING_SIZES.md)` line. `dashes the small radius at 0, 50 and 100 percent` and the `sm` `ringViewBox` line stay.
  - Run `cmd /c "npx vitest run src/lib/css-contract.test.ts src/lib/layout.test.ts src/lib/present.test.ts src/lib/gauge.test.ts > .claude-work\window-hug\logs\T3-red.log 2>&1"`.
  - Expected red: only `no card-layout rules remain` and `.ring-sm svg fills its wrapper` (styles.css not edited yet) and layout.test's `shellVars` `toEqual` (ring variables still emitted). The trimmed `ASSERTED_VARS`, re-pointed `ringDash` rows and single-argument `countPlacement` rows may already pass at runtime; they guard the deletion, and `npm run build` (tsc) is what fails on a surviving import.
- [ ] **Step 2: Implement.**
  - `src/lib/layout.ts`: delete `layoutFor`, `Layout`, `BREAKPOINTS`, `NARROW_HIDDEN`, `autoHiddenColumns`, `SHELL_PADDING`, and the ring entries in `shellVars`.
  - `src/App.tsx`: delete the cards branch (80-85) and the `layoutFor`/`autoHiddenColumns` use; the table always renders; stop passing `layout` to Settings; hidden columns are the user's choice only.
  - Delete `src/components/AccountCard.tsx`.
  - `src/components/Ring.tsx`: remove the `md` variant; `src/lib/gauge.ts`: remove `RING` and `RING_SIZES.md`.
  - `src/components/Header.tsx`: remove `compact`. `src/lib/present.ts`: `countPlacement` loses its `compact` parameter and branch.
  - `src/components/Settings.tsx`: remove the `layout` prop, the line-6 import and the width hints (152-182).
  - `src/styles.css`: delete the `.cards`, `.card`, `.card-rings`, `.ring`, `.ring-label`, `.ring-value` rules (197-207); `.ring svg, .ring-sm svg { ... }` (line 205) becomes `.ring-sm svg { ... }` with the same declarations.
- [ ] **Step 3:** Re-run the Step 1 command to `T3-green.log`. Expected: all pass.
- [ ] **Step 4:** Run the five gates (`<n>` = 3). Expected: all `exit=0`; `npm run build` proves no removed export is still imported.
- [ ] **Step 5:** `grep -rn "layoutFor\|BREAKPOINTS\|autoHiddenColumns\|AccountCard\|SHELL_PADDING\|NARROW_HIDDEN" src` matches nothing.
- [ ] **Step 6:** Commit (procedure C): `Window hug 3: remove the unreachable narrow and cards layouts`.

**Interim behaviour:** none visible; the deleted paths were unreachable after Task 1.

**Done when:** Steps 3-5 hold and the commit contains only the Files list (including the deletion).

---

### Task 4: The Rust pure core

**Spec:** §3.2 (constants, `AspectState`, pure functions, `FitOutcome`), §4 (`fit_rect` contract and worked case), §7.2. **Rulings:** R3, R4, R5, R11, R13, R21, R22, R23. **Agent:** `implementer`. **Tier:** logic (`reviewer`). **Risk:** medium: every later task trusts this geometry. **Depends on:** Task 2 (the config values `config_matches_constants` reads).

**Why:** every decision lives in portable, table-tested functions so the proc and the command are thin glue that cannot hide a geometry bug, and so the module compiles and tests on every platform.

**Files:** `src-tauri/src/window_aspect.rs` (new), `src-tauri/src/lib.rs` (`pub mod window_aspect;` in the `pub mod` list, lines 1-15).

**Interfaces:** Consumes: `crate::error::{AppError, AppResult}` (`AppError::OutOfRange(String)`, `AppError::Internal(String)`), `crate::test_log::captured(f: impl FnOnce()) -> String` (tests). Produces (all `pub`; physical px in `i32` unless stated):
- `BASE_WIDTH_PX: f64 = 980.0`, `MIN_WIDTH_PX: f64 = 735.0`, `ZOOM_MAX: f64 = 2.5`, `MIN_CONTENT_H: f64 = 80.0`, `MAX_CONTENT_H: f64 = 10_000.0`.
- `Rect { left, top, right, bottom }`, `Size { w, h }` (`Copy`, `PartialEq`, `Debug`).
- `AspectState { ratio_bits: AtomicU64, in_size_move: AtomicBool, pending_fit: AtomicBool }` with `Default`; `ratio() -> Option<f64>` (bits 0 means `None`), `set_ratio(f64)`, `begin_size_move()`, `end_size_move() -> bool` (clears `in_size_move`, returns `is_pending()`), `in_size_move()`, `is_pending()`, `mark_pending()`, `take_pending() -> bool`.
- `validate_content_h(h: f64) -> AppResult<f64>`; `accept_report(state: &AspectState, content_h: f64) -> AppResult<f64>` (validate, then `set_ratio(BASE_WIDTH_PX / content_h)`; a rejection stores nothing).
- `FitAction { Fit, Defer, SkipMaximized, SkipMinimized }`; `decide_fit(maximized, minimized, in_size_move) -> FitAction` (minimized, then maximized, then size-move, else `Fit`).
- `min_client_px(min_logical: f64, dpi: u32) -> i32` = `ceil(min_logical * dpi / 96)`.
- `ClientBounds { min_w, max_w, min_h, max_h }`; `client_bounds(dpi: u32, work: Option<Rect>, nc: Size) -> ClientBounds`: `min_w = min_client_px(MIN_WIDTH_PX, dpi)`; `max_w = min(work.w - nc.w, floor(BASE_WIDTH_PX * ZOOM_MAX * dpi / 96))`; `max_h = work.h - nc.h`; no work area: `max_w` is the ZOOM_MAX term, `max_h = i32::MAX`; `min_h = min_client_px(MIN_CONTENT_H * MIN_WIDTH_PX / BASE_WIDTH_PX, dpi)`.
- `ClientFit { w, h, capped }`; `fit_client(target_w: f64, ratio: f64, b: &ClientBounds) -> ClientFit`, exactly the §3.2 pseudo-code: `upper = min(b.max_w, floor(b.max_h * ratio))`; `w = round(target_w)`, then `if w > upper { w = upper }`, then `if w < b.min_w { w = b.min_w }`; `want_h = ceil(w / ratio)`; `h = min(max(want_h, b.min_h), b.max_h)`; `capped = round(target_w) > upper || want_h > b.max_h`. A NaN `target_w` gives `min_w` (write the NaN case explicitly: `f64 as i32` of NaN is 0, which the min step lifts).
- `WindowFacts { client: Size, outer: Rect, work: Option<Rect>, dpi: u32, maximized: bool, minimized: bool }`.
- `FitPlan { Unchanged, Resize { outer: Rect, client_w: i32, client_h: i32, capped: bool } }`; `plan_fit(f: &WindowFacts, ratio: f64) -> FitPlan`: `nc = outer size - client`; `fit = fit_client(client.w as f64, ratio, &client_bounds(dpi, work, nc))`; `grow = max(0, fit.h - client.h)`; `overflow = max(0, outer.top + nc.h + fit.h - work.bottom)` (0 with no work area); `top = max(work.top, outer.top - min(grow, overflow))` (`outer.top - min(..)` with no work area); `Unchanged` when `fit.w == client.w` and `0 <= client.h - fit.h <= 1`; otherwise `Resize` with `outer.left` kept, `top`, `right = left + fit.w + nc.w`, `bottom = top + fit.h + nc.h`.
- `Step { NoRatio, Skip(FitAction), Unchanged, Apply(FitPlan) }`; `step(state: &AspectState, f: &WindowFacts) -> Step`: no ratio gives `NoRatio`; a non-`Fit` action calls `mark_pending()` and returns `Skip`; `Fit` calls `take_pending()` before `plan_fit`, then `Unchanged` or `Apply(Resize ..)`.
- `Edge { Left, Right, Top, TopLeft, TopRight, Bottom, BottomLeft, BottomRight }`; `Edge::from_wmsz(v: u32) -> Option<Edge>` with literals 1 Left, 2 Right, 3 Top, 4 TopLeft, 5 TopRight, 6 Bottom, 7 BottomLeft, 8 BottomRight; others `None`.
- `fit_rect(edge: Edge, rect: Rect, nc: Size, ratio: f64, b: &ClientBounds) -> Rect`, exactly §4: `cw = rect.w - nc.w`, `ch = rect.h - nc.h`; `target_w` = `cw` for Left/Right, `ch * ratio` for Top/Bottom, `max(cw, ch * ratio)` for corners; `fit = fit_client(target_w, ratio, b)`; `W' = fit.w + nc.w`, `H' = fit.h + nc.h`; Left, TopLeft, BottomLeft set `left = right - W'`, others `right = left + W'`; Top, TopLeft, TopRight set `top = bottom - H'`, others `bottom = top + H'`. It never moves the window up beyond what the dragged top edge implies.
- `run_guarded<T>(warned: &AtomicBool, body: impl FnOnce() -> T, fallback: impl FnOnce() -> T) -> T`: `catch_unwind(AssertUnwindSafe(body))`; on `Err` logs `tracing::warn!("window aspect subclass panicked; forwarding")` only when `warned.swap(true, SeqCst)` was false, then returns `fallback()`.
- `FitOutcome { Applied, AlreadyFitted, Capped, SkippedMaximized, SkippedMinimized, Deferred }` with `#[derive(Serialize)] #[serde(rename_all = "camelCase")]`; `outcome_of(&Step) -> Option<FitOutcome>`: `Apply(Resize{capped: true, ..})` is `Capped`, other `Apply` is `Applied`, `Unchanged` is `AlreadyFitted`, `Skip(SkipMaximized)` / `Skip(SkipMinimized)` / `Skip(Defer)` are `SkippedMaximized` / `SkippedMinimized` / `Deferred`, `NoRatio` is `None`.
- No `clamp`, no `unwrap()`, no `expect(` outside `mod tests`; float-to-int conversions go through explicit `floor`/`ceil`/`round` then `as i32`.

- [ ] **Step 1: Tests first** in `#[cfg(test)] mod tests` of the new `window_aspect.rs`, with every public item above declared and each body `unimplemented!()` (stubs exist only for the red run; Step 2 replaces every one). Add `pub mod window_aspect;` to lib.rs. Tests (fixture defaults: work 1920x1032 at (0, 0), nc (16, 39), dpi 96, ratio 980/640):
  - `validate_content_h_accepts_the_range`: 80, 640, 10000 are `Ok`.
  - `validate_content_h_rejects`: 79.9, 10000.1, 0, -1, NaN, +inf, -inf, 1e-300 are `Err(AppError::OutOfRange(_))`.
  - `default_ratio_is_unknown`: `AspectState::default().ratio()` is `None`.
  - `rejected_report_keeps_old_ratio`: `accept_report(&s, 640.0)` then `accept_report(&s, 0.0)` errs and `ratio()` is still `980/640`.
  - `end_size_move_reports_pending_and_step_takes_it_once`: begin, `mark_pending`, `end_size_move()` is true; `step` on an off-shape normal window gives `Apply` and `is_pending()` false; a second `step` on the same facts does not re-mark.
  - `decide_fit_table`: all 8 combinations of (maximized, minimized, in_size_move); precedence minimized > maximized > size-move.
  - `step_table`: no ratio gives `NoRatio`; minimized, maximized and in a size-move each give `Skip(..)` with `is_pending()` true; normal within 1 px gives `Unchanged` with pending cleared; normal off-shape gives `Apply` with pending cleared.
  - `min_client_px_table`: (735, 96) is 735; (735, 144) is 1103; (735, 120) is 919.
  - `client_bounds_table`: work 1920x1032, nc (16, 39), dpi 96 gives `min_w` 735, `max_w` 1904, `max_h` 993, `min_h` 60; work 3840x2112 at dpi 96 gives `max_w` 2450; no work area gives `max_h` `i32::MAX`.
  - `fit_client_table`: inside the bounds; above `upper` gives `upper` and `capped`; height-capped with the ratio kept; crossed (`min_w > floor(max_h * ratio)`) gives `w = min_w`, `h = max_h`, `capped`; a NaN target gives `min_w`; ratio 100 at `w = min_w` gives `h = min_h` (want_h 8 < min_h 60 at dpi 96).
  - `plan_fit_table`: within 1 px gives `Unchanged`; a client 1 px short gives `Resize`; a grow with room keeps the top; outer.top 400, client 1000x400, ratio 980/640 gives client 1000x654 and top 339 (grow 254, overflow 61); `a same-size window below the work area is Unchanged` (outer.top 900, fitted client); `a grow from an already-overflowing position moves up by the growth only` (outer.top 500, client 1000x654, ratio 980/660: fit.h 674, grow 20, overflow 181, top 480); an overflow beyond the room gives `top = work.top`, the cap and `capped`; a shrink; a width out of bounds after a DPI change is resized into bounds.
  - `fit_rect_worked_case`: ratio 980/640, nc (16, 39), Right, outer 1016x700 at (100, 100), dpi 96, work 1920x1032: the result is 1016x693 (client 1000x654), `left`, `top` and `right` unchanged.
  - `fit_rect_edges_anchor`: 8 rows; dragged edges move, opposite edges keep their coordinate; Left/Right keep the top, Top/Bottom keep the left.
  - `fit_rect_min_width`: 8 rows proposing a client narrower than 735: client width is `min_w`.
  - `fit_rect_height_cap`: 8 rows with the height cap binding and bounds not crossed: client `h = max_h`, `w = floor(max_h * ratio)` within 1 px, opposite edges fixed, and for Top, TopLeft, TopRight `rect.top = rect.bottom - H'`.
  - `fit_rect_crossed_bounds`: 8 rows, none panicking, each `w = min_w`, `h = max_h`.
  - `fit_rect_zoom_max_cap`: a proposed client wider than 2450 at dpi 96 on a 3840 work area gives width 2450.
  - `fit_rect_keeps_ratio_within_1px`: every edge x client widths `735..=2400` step 15 x three ratios (980/640, 980/300, 980/1200), whenever not capped: `0 <= fit.h - ceil(fit.w / ratio) <= 1`.
  - `edge_from_wmsz_table`: 1-8 map to the 8 edges in the order above; 0 and 9 give `None`.
  - `run_guarded_returns_fallback_on_panic_and_warns_once`: a test-local `AtomicBool`; inside `captured(..)`, two panicking bodies both return the fallback; the captured text has exactly one "window aspect subclass panicked; forwarding".
  - `run_guarded_returns_the_body_value_without_a_panic`: the fallback closure panics if called; the captured text is empty.
  - `fit_outcome_serializes_camel_case`: `serde_json::to_string` gives `"applied"`, `"alreadyFitted"`, `"capped"`, `"skippedMaximized"`, `"skippedMinimized"`, `"deferred"`.
  - `config_matches_constants`: `include_str!("../tauri.conf.json")` parsed with `serde_json`; `app.windows[0].width == BASE_WIDTH_PX`, `minWidth == MIN_WIDTH_PX`, `minHeight` absent.
  - `zoom_max_matches_layout_ts`: `include_str!("../../src/lib/layout.ts")` contains `format!("export const ZOOM_MAX = {ZOOM_MAX};")`.
  - Run `cargo test --manifest-path src-tauri/Cargo.toml window_aspect > .claude-work/window-hug/logs/T4-red.log 2>&1`.
  - Expected red: every test except `config_matches_constants`, `zoom_max_matches_layout_ts` and `fit_outcome_serializes_camel_case` (a derive, no stub) fails with a "not implemented" panic. The two drift tests pass already (Task 2 landed the config and `ZOOM_MAX` is unchanged); that is correct, because they guard drift, not this task's code.
- [ ] **Step 2: Implement** every stub per the Interfaces list. Use ordered `min`/`max` steps; no `clamp`, `unwrap()` or `expect(` outside tests; atomics with `Ordering::SeqCst`.
- [ ] **Step 3:** Re-run the Step 1 command to `T4-green.log`. Expected: all pass.
- [ ] **Step 4:** Run the five gates (`<n>` = 4). Expected: all `exit=0`; the `pub` items raise no dead-code lint.
- [ ] **Step 5:** `grep -n "clamp(\|unwrap()\|expect(\|unimplemented!\|todo!" src-tauri/src/window_aspect.rs` matches only inside `mod tests` (and no `unimplemented!`/`todo!` at all).
- [ ] **Step 6:** Commit (procedure C): `Window hug 4: pure aspect core (bounds, fit, plan, step, fit_rect, run_guarded)`.

**Interim behaviour:** nothing calls the module yet.

**Done when:** Steps 3-5 hold and the commit contains only the Files list.

---

### Task 5: Platform facts and apply

**Spec:** §3.3 "Facts and apply", §3.4, §7.3 fixture and the four task-5 tests. **Rulings:** R5, R8, R13 (arch M3), R23 (T2-M3). **Agent:** `implementer`. **Tier:** logic (`reviewer`; Windows proc and apply path). **Risk:** medium: first Win32 calls and the test fixture every Task 6 test stands on. **Depends on:** Task 4.

**Why:** the command and the proc must share one apply (`apply_hwnd`, one `SetWindowPos` carrying position and size), and the proc must read facts without tauri getters, which may re-enter tao's state lock from inside the proc.

**Files:** `src-tauri/src/platform/mod.rs` (new), `src-tauri/src/platform/windows/mod.rs` (new), `src-tauri/src/platform/windows/aspect.rs` (new: `hwnd_facts`, `apply_hwnd`, `apply_fit`, `#[cfg(all(windows, test))] mod hwnd_tests` with the fixture), `src-tauri/src/platform/other.rs` (new: `apply_fit`), `src-tauri/src/lib.rs` (`pub mod platform;`), `src-tauri/Cargo.toml`.

**Interfaces:** Consumes: Task 4's `Rect`, `Size`, `WindowFacts`, `FitPlan`, `AppError`, `AppResult`. Produces:
- `platform/mod.rs`: `#[cfg(windows)] pub mod windows; #[cfg(windows)] pub use windows::*; #[cfg(not(windows))] mod other; #[cfg(not(windows))] pub use other::*;`. `platform/windows/mod.rs`: `pub mod aspect; pub use aspect::{apply_fit, install};` (`install` is added to the re-export in Task 6).
- `aspect::hwnd_facts(hwnd: HWND) -> Option<WindowFacts>`: `GetWindowRect` (outer), `GetClientRect` (client size), `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW` `rcWork` (work), `GetDpiForWindow` (dpi; 0 is a failure), `IsZoomed`, `IsIconic`. Any failed call gives `None`.
- `aspect::apply_hwnd(hwnd: HWND, plan: &FitPlan, label: &str) -> bool`: `Unchanged` returns true with no call. `Resize { outer, .. }` makes one `SetWindowPos(hwnd, null, outer.left, outer.top, outer.right - outer.left, outer.bottom - outer.top, SWP_NOZORDER | SWP_NOACTIVATE)`, adding `SWP_NOMOVE` when `outer.top` equals the current `GetWindowRect` top. No `SWP_ASYNCWINDOWPOS`. Failure logs `tracing::warn!(label, error = GetLastError(), "SetWindowPos failed")` and returns false.
- `aspect::apply_fit<R: tauri::Runtime>(window: &tauri::Window<R>, plan: &FitPlan) -> AppResult<()>`: resolves the HWND from `window.hwnd()`, calls `apply_hwnd(hwnd, plan, window.label())`, maps false to `AppError::Internal("SetWindowPos failed".into())`. Generic over `R` so the plugin's `Window<R>` and the command's `Window` both fit (a refinement of §3.4's non-generic signature; note it in REPORT).
- `other.rs`: `apply_fit<R: Runtime>(window: &Window<R>, plan: &FitPlan) -> AppResult<()>`: `Unchanged` does nothing; `Resize` calls `window.set_position(PhysicalPosition::new(outer.left, outer.top))` only when the top differs from `outer_position()`, then `window.set_size(PhysicalSize::new(client_w, client_h))` (`set_size` sets the inner size); errors map to `AppError::Internal`.
- Hypothesis - validate before building: `tauri::Window<R>::hwnd()` exists in tauri 2.12.1 on Windows and returns the `windows` crate's `HWND`, whose `.0` converts to the windows-sys `HWND` (`*mut c_void`). Check with `grep -rn "pub fn hwnd" ~/.cargo/registry/src/*/tauri-2.12.*/src/window/` before writing `apply_fit`; if it is absent or differs, stop and report the exact signature.

- [ ] **Step 1: Cargo features.** In `src-tauri/Cargo.toml` `[target.'cfg(windows)'.dependencies] windows-sys` (line 50) add `"Win32_UI_Shell"`, `"Win32_UI_WindowsAndMessaging"`, `"Win32_UI_HiDpi"`, `"Win32_Graphics_Gdi"`; in `[target.'cfg(windows)'.dev-dependencies] windows-sys` (line 57) add `"Win32_System_LibraryLoader"`. No new crate; `Cargo.lock` must not change (`git diff --stat src-tauri/Cargo.lock` empty).
- [ ] **Step 2: Tests first.** Create the three platform files and `pub mod platform;` in lib.rs, with `hwnd_facts`, `apply_hwnd` and `apply_fit` declared and their bodies `unimplemented!()`. In `#[cfg(all(windows, test))] mod hwnd_tests` write the fixture:
  - A `std::sync::Once` registers class `CUT_ASPECT_TEST` (`RegisterClassW`, `hInstance` from `GetModuleHandleW(null)`) whose proc `counting_wndproc` increments a `thread_local!` `RefCell<HashMap<u32, u32>>` counter for the message, then returns `DefWindowProcW(..)`. A helper `forwards(msg) -> u32` reads it. Each test runs on its own thread, so counters never race.
  - Geometry from the host (never constants): `work` from `SystemParametersInfoW(SPI_GETWORKAREA)`; `dpi` from `GetDpiForWindow` on a hidden probe window; `nc_oracle` = the frame `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, FALSE, 0, dpi)` adds to a zero rect; the test window is created hidden (`CreateWindowExW`, no `WS_VISIBLE`) at `(work.left + 40, work.top + 40)` with outer size = client 800 x 523 (`ceil(800 * 640 / 980)`) plus `nc_oracle`. A work area smaller than 1000 x 760 fails with a message naming its size (not a skip).
  - `fitted(cw, ch)` = `0 <= ch - ceil(cw * 640 / 980) <= 1` (integer arithmetic: `(cw * 640 + 979) / 980`).
  - Every test destroys its window with `DestroyWindow` at the end.
  - Tests:
    - `hwnd_facts_reads_a_hidden_window`: `outer` equals the rect passed to `CreateWindowExW`; `outer size - client` equals `nc_oracle`; `client` equals 800 x 523; `work` equals the `SPI_GETWORKAREA` rect; `dpi > 0`; `maximized` and `minimized` false.
    - `apply_hwnd_resizes_without_moving`: `apply_hwnd` with `Resize { outer: (created.left, created.top, created.left + 900 + nc.w, created.top + 588 + nc.h), client_w: 900, client_h: 588, capped: false }` (588 = `ceil(900 * 640 / 980)`) returns true; afterwards `GetWindowRect` `left`/`top` equal the created ones and `GetClientRect` is `fitted` with width 900.
    - `apply_hwnd_moves_up_and_resizes_in_one_call`: `Resize` with `top` = created top - 20 and client 800 x 543; after one `apply_hwnd` call the outer top is created top - 20 and the client is 800 x 543.
    - `wmsz_literals_match_windows_sys`: `Edge::from_wmsz(WMSZ_LEFT)` .. `Edge::from_wmsz(WMSZ_BOTTOMRIGHT)` (windows-sys constants) give the 8 edges.
  - Run `cargo test --manifest-path src-tauri/Cargo.toml hwnd_tests > .claude-work/window-hug/logs/T5-red.log 2>&1`.
  - Expected red: the three facts/apply tests panic "not implemented"; `wmsz_literals_match_windows_sys` passes already (Task 4's literals), which is correct: it is a drift guard.
- [ ] **Step 3: Implement** `hwnd_facts`, `apply_hwnd`, `apply_fit` (Windows) and `other.rs` `apply_fit` per Interfaces. `unsafe` blocks are minimal and each carries a `// SAFETY:` comment naming the invariant (a valid HWND owned by this thread; out-pointers to stack locals).
- [ ] **Step 4:** Re-run the Step 2 command to `T5-green.log`. Expected: all four pass.
- [ ] **Step 5:** Run the five gates (`<n>` = 5). Expected: all `exit=0`.
- [ ] **Step 6:** `grep -rn "clamp(\|unwrap()\|expect(\|unimplemented!" src-tauri/src/platform` matches only inside `mod hwnd_tests` (and no `unimplemented!`).
- [ ] **Step 7:** Commit (procedure C): `Window hug 5: platform module with hwnd_facts and the shared apply`.

**Interim behaviour:** nothing calls `apply_fit` yet. `other.rs` is compiled only off Windows; the gates on this machine do not build it (UNSURE: no non-Windows build exists in the gates; the reviewer reads it against the tauri 2.12 `Window` API).

**Done when:** Steps 1 and 4-6 hold, `Cargo.lock` is unchanged, and the commit contains only the Files list.

---

### Task 6: The subclass proc, driven by the HWND integration test

**Spec:** §3.3 "Install" and "The proc", §6 (INFO/WARN lines), §7.3 (all rows not landed in Task 5). **Rulings:** R4, R5, R9, R13 (arch M2, M7), R19, R20, R22. **Agent:** `implementer`. **Tier:** logic (`reviewer`; Windows proc and pending/apply paths). **Risk:** high: an `extern "system"` callback that owns a raw pointer. **Depends on:** Task 5 (fixture, `hwnd_facts`, `apply_hwnd`).

**Why:** the drag lock and every resume path (size-move end, restore, DPI change) live in the proc. The HWND test against a real hidden window is its contract (R9): written first, then the proc is written to pass it.

**Files:** `src-tauri/src/platform/windows/aspect.rs`, `src-tauri/src/platform/windows/mod.rs` (re-export `install`), `src-tauri/src/platform/other.rs` (`install`).

**Interfaces:** Consumes: Task 4's `AspectState`, `step`, `Step`, `FitAction`, `fit_rect`, `client_bounds`, `Edge::from_wmsz`, `run_guarded`; Task 5's `hwnd_facts`, `apply_hwnd`, fixture. Produces:
- `const SUBCLASS_ID: usize = 0x4355_5441;` `static PANIC_WARNED: AtomicBool`.
- `pub struct ProcHooks { pub apply: fn(HWND, &FitPlan, &str) -> bool, pub after_forward: fn(u32) }` with `pub const REAL: ProcHooks = ProcHooks { apply: apply_hwnd, after_forward: |_| {} }`. Production code builds no other value.
- `struct SubclassData { state: Arc<AspectState>, label: String, hooks: ProcHooks }`.
- `pub fn install_hwnd(hwnd: HWND, state: Arc<AspectState>, label: String) -> Result<(), u32>` = `install_hwnd_with(hwnd, state, label, ProcHooks::REAL)`.
- `pub fn install_hwnd_with(hwnd, state, label, hooks) -> Result<(), u32>`: `Box::into_raw(Box::new(SubclassData { .. }))`, `SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, ptr as usize)`; on failure `drop(Box::from_raw(ptr))` and `Err(GetLastError())`.
- `pub fn install<R: Runtime>(window: &tauri::Window<R>, state: Arc<AspectState>) -> bool`: resolves the HWND, `install_hwnd`; `Ok` logs `info!(label, dpi = GetDpiForWindow(hwnd), "subclass installed")` and returns true; `Err(code)` logs `warn!(label, error = code, "subclass install failed")` and returns false.
- `unsafe extern "system" fn subclass_proc(hwnd, msg, wparam, lparam, _id: usize, data: usize) -> LRESULT`, shapes exactly as §3.3:
  - `WM_ENTERSIZEMOVE`: `run_guarded(&PANIC_WARNED, || state.begin_size_move(), || ())`, then `DefSubclassProc`.
  - `WM_SIZING`: `run_guarded(&PANIC_WARNED, rewrite, || false)`; `rewrite` returns true only when `IsZoomed(hwnd) == 0`, `ratio()` is `Some`, `Edge::from_wmsz(wparam as u32)` is `Some`, and `hwnd_facts` is `Some`; it sets `*(lparam as *mut RECT) = fit_rect(edge, rect, nc, ratio, &client_bounds(dpi, Some(work), nc))`. True returns `TRUE` (1) without forwarding; false forwards once. Not logged.
  - `WM_EXITSIZEMOVE`: `let r = DefSubclassProc(..)`, then guarded post-work: `(hooks.after_forward)(msg)`; `pending = end_size_move()`; when pending, `hwnd_facts` then `step`: `Apply(plan)` calls `(hooks.apply)(hwnd, &plan, &label)`, re-marking pending when it returns false; `Skip` stays pending. Logs one `info!(label, client_w, client_h, dpi, ratio_err_px, pending, "size-move end")` with `pending` one of `none | applied | unchanged | apply_failed | skipped:maximized | skipped:minimized | skipped:deferred` and `ratio_err_px = client_h - ceil(client_w / ratio)` read after the apply (omit the field as `None` when the ratio is unknown). Returns `r`.
  - `WM_SIZE` with `wparam == SIZE_RESTORED`: forward first; guarded post-work: `after_forward`; when `!in_size_move()` and `is_pending()`, run `step` and apply as above; on an apply log `info!(label, source = "restored", client_w, client_h, top, capped, ratio = format!("{ratio:.6}"), "window fitted")`.
  - `WM_DPICHANGED`: forward first; guarded post-work: `after_forward`; `mark_pending()`; when `!in_size_move()`, run `step` and apply, logging "window fitted" with `source = "dpi"`.
  - `WM_NCDESTROY`: `RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID)`, `let r = DefSubclassProc(..)`, `drop(Box::from_raw(data as *mut SubclassData))`, return `r`. No guarded work between removal and forward.
  - Everything else: `DefSubclassProc`.
  - A `None` from `hwnd_facts` in any post-work logs `warn!(label, error = GetLastError(), "window facts unavailable")` and leaves the pending flag set.
  - No message is forwarded twice; nothing panics out of the frame (every work closure runs inside `run_guarded`).
- `other.rs`: `install<R: Runtime>(_: &Window<R>, _: Arc<AspectState>) -> bool` logs `info!("live aspect lock unsupported on this platform")` once per process (a `static` `Once`) and returns false.

- [ ] **Step 1: Tests first** in `hwnd_tests`, using the Task 5 fixture with an `Arc<AspectState>` whose ratio is `980/640`. Declare `install_hwnd`, `install_hwnd_with`, `ProcHooks` and `SUBCLASS_ID` with `unimplemented!()` bodies only where a body exists. "Off-shape" below means a `SetWindowPos` of the hidden window to client 800 x 600 (77 px taller than fitted).
  - `sizing_rewrites_every_edge`: for each `WMSZ_*`, the created outer rect grown 40 px outward on the dragged edges; `SendMessageW(hwnd, WM_SIZING, wmsz, &mut rect as *mut RECT as LPARAM)` returns 1. Wiring: `rect == fit_rect(edge, grown, nc, ratio, &client_bounds(..))` computed from `hwnd_facts`. Independent geometry: `rect - nc_oracle` is `fitted`, the opposite edges equal the grown rect's, at least one dragged edge moved outward from the created rect.
  - `sizing_passes_through_without_a_ratio`: a fresh state (no ratio); the rect is unchanged and the class proc's `WM_SIZING` count is 1.
  - `exit_size_move_applies_pending_synchronously`: `WM_ENTERSIZEMOVE` makes `in_size_move()` true; `mark_pending()` and an off-shape `SetWindowPos`; `SendMessageW(WM_EXITSIZEMOVE)`: on return `in_size_move()` and `is_pending()` are false, `GetClientRect` is `fitted`, `forwards(WM_EXITSIZEMOVE) == 1`.
  - `size_restored_applies_pending`: off-shape, `mark_pending()`, `SendMessageW(WM_SIZE, SIZE_RESTORED, MAKELPARAM(cw, ch))`: the client is `fitted` and `is_pending()` is false.
  - `dpi_changed_applies_when_not_in_a_size_move`: off-shape, nothing pending, `SendMessageW(WM_DPICHANGED, MAKEWPARAM(dpi, dpi), &current_window_rect)`: the client is `fitted`, `is_pending()` false, `forwards(WM_DPICHANGED) == 1`.
  - `dpi_changed_in_a_size_move_marks_pending`: `WM_ENTERSIZEMOVE`, off-shape, `WM_DPICHANGED`: size unchanged and `is_pending()` true; then `WM_EXITSIZEMOVE`: `fitted`.
  - `apply_failure_in_the_proc_keeps_pending`: `install_hwnd_with(.., ProcHooks { apply: |_, _, _| false, after_forward: ProcHooks::REAL.after_forward })`; off-shape, `mark_pending()`, `WM_EXITSIZEMOVE`: size unchanged and `is_pending()` still true.
  - `a_panic_after_the_forward_forwards_once`: `install_hwnd_with(.., ProcHooks { apply: apply_hwnd, after_forward: |_| panic!("test") })`; `SendMessageW(WM_EXITSIZEMOVE)` returns; `forwards(WM_EXITSIZEMOVE) == 1`; then `WM_SIZE`/`SIZE_RESTORED` returns and `forwards(WM_SIZE) == 1`. The once-per-process WARN is not asserted here (`PANIC_WARNED` is process-wide; Task 4's `run_guarded_returns_fallback_on_panic_and_warns_once` owns it).
  - `zoomed_window_keeps_pending`: `SetWindowLongPtrW(hwnd, GWL_STYLE, style | WS_MAXIMIZE)`, off-shape, `mark_pending()`, `WM_EXITSIZEMOVE`: size unchanged and `is_pending()` true. Hypothesis - validate before building: `IsZoomed` reads the `WS_MAXIMIZE` style bit on a hidden window.
  - `destroy_frees_subclass_data`: `Arc::strong_count(&state)` is 2 after `install_hwnd` and 1 after `DestroyWindow`.
  - Run `cargo test --manifest-path src-tauri/Cargo.toml hwnd_tests > .claude-work/window-hug/logs/T6-red.log 2>&1`.
  - Expected red: every new test panics "not implemented" in `install_hwnd`/`install_hwnd_with`; the four Task 5 tests still pass.
- [ ] **Step 2: Implement** the Interfaces list. Each `unsafe` block carries a `// SAFETY:` comment; `data` is dereferenced only as `&*(data as *const SubclassData)` before `WM_NCDESTROY` frees it.
- [ ] **Step 3:** Run the HWND tests three times (pass^3, because they drive real window messages). Expected: `fails=0`.

  ```bash
  fails=0; for i in 1 2 3; do cargo test --manifest-path src-tauri/Cargo.toml hwnd_tests >> .claude-work/window-hug/logs/T6-green.log 2>&1 || fails=$((fails+1)); done; echo "fails=$fails"; tail -40 .claude-work/window-hug/logs/T6-green.log
  ```
- [ ] **Step 4: Settle the `zoomed_window_keeps_pending` hypothesis.** If it passes, write "IsZoomed reads WS_MAXIMIZE on a hidden window: confirmed" in REPORT. If it fails because `IsZoomed` is 0, delete the test (never `#[ignore]`), add H4b to Task 9's human list ("snap-maximize during a drag with a pending fit: the window stays maximized and the next restore fits it"), and write the observed `IsZoomed` value in REPORT.
- [ ] **Step 5:** Run the five gates (`<n>` = 6). Expected: all `exit=0`.
- [ ] **Step 6:** `grep -rn "clamp(\|unwrap()\|expect(\|unimplemented!" src-tauri/src/platform` matches only inside `mod hwnd_tests` (and no `unimplemented!`).
- [ ] **Step 7:** Commit (procedure C): `Window hug 6: WM_SIZING ratio lock and pending-fit resume in the subclass proc`.

**Interim behaviour:** nothing installs the subclass on the app window yet.

**Done when:** Steps 3-6 hold and the commit contains only the Files list.

---

### Task 7: The command and the plugin

**Spec:** §3.2 "Command", "`fit_now`", "`plugin`"; §5 "Cold start", "Close to tray and reopen"; §6; §7.2 command-glue rows; §8 task 7. **Rulings:** R3, R4, R6, R13 (arch M1), R15, R19. **Agent:** `implementer`. **Tier:** logic (`reviewer`; pending/apply path). **Risk:** medium: plugin order and the main-thread assumption. **Depends on:** Task 6.

**Why:** this wires the pure core and the proc to tauri: the frontend's report becomes a stored ratio and a fit, and every window (including one rebuilt after close-to-tray) gets the subclass and a fit from the stored ratio before the frontend reports again.

**Files:** `src-tauri/src/window_aspect.rs` (`facts_from_getters`, `handle_report`, `window_facts`, `set_content_height`, `fit_now`, `plugin`), `src-tauri/src/lib.rs` (`.plugin(window_aspect::plugin())` right after the `tauri_plugin_window_state` builder at lines 208-212; `window_aspect::set_content_height` in `generate_handler!` at lines 218-235).

**Interfaces:** Consumes: Tasks 4-6 (`accept_report`, `step`, `outcome_of`, `platform::install`, `platform::apply_fit`). Produces:
- `pub fn facts_from_getters(inner: (u32, u32), outer_pos: (i32, i32), outer: (u32, u32), scale: f64, work: Option<Rect>, maximized: bool, minimized: bool) -> WindowFacts`: `client = Size { w: inner.0, h: inner.1 }`, `outer = Rect { left: outer_pos.0, top: outer_pos.1, right: left + outer.0, bottom: top + outer.1 }`, `dpi = round(scale * 96)` as `u32`. `u32` to `i32` conversions saturate without a panic path: `i32::try_from(v).unwrap_or(i32::MAX)` (non-panicking; the Done-when grep targets `unwrap()` only).
- `pub fn handle_report(state: &AspectState, content_h: f64, facts: impl FnOnce() -> AppResult<WindowFacts>, apply: impl FnOnce(&FitPlan) -> AppResult<()>) -> AppResult<(FitOutcome, Option<FitPlan>)>`:
  1. `accept_report(state, content_h)?` (a rejection returns before `facts` is called);
  2. `let f = facts()?` (the ratio stays stored);
  3. `step(state, &f)`: `Apply(plan)` calls `apply(&plan)`; on `Err(e)`, `state.mark_pending()` and return `Err(e)`; on `Ok`, return `(outcome_of(..), Some(plan))`. `Unchanged` and `Skip` return `(outcome_of(..), None)`. `NoRatio` (unreachable after step 1) returns `Err(AppError::Internal("ratio missing after report".into()))`, never a panic.
- `fn window_facts<R: Runtime>(window: &tauri::Window<R>) -> AppResult<WindowFacts>`: `is_maximized`, `is_minimized`, `inner_size`, `outer_position`, `outer_size`, `scale_factor`, `current_monitor()` then `Monitor::work_area()` converted to `Rect` (`None` monitor gives `work: None`); a getter error logs `warn!(label, error = %e, "window facts unavailable")` and returns `AppError::Internal`. This is the one work-area owner outside the proc. Hypothesis - validate before building: `Monitor::work_area()` exists in tauri 2.12.1 and returns a `PhysicalRect<i32, u32>`; check with `grep -rn "fn work_area" ~/.cargo/registry/src/*/tauri-2.12.*/src/` and stop with the signature if absent.
- `#[tauri::command] pub fn set_content_height(window: tauri::Window, state: tauri::State<'_, Arc<AspectState>>, content_h: f64) -> AppResult<FitOutcome>` (sync, main thread): calls `handle_report(&state, content_h, || window_facts(&window), |plan| platform::apply_fit(&window, plan))` and logs:
  - rejection: `warn!(label, content_h, "content height rejected")`;
  - every report: `debug!(label, content_h, ratio = format!("{ratio:.6}"), outcome = <camelCase name>, "content height report")`; a failure logs the same line with `outcome = "error"` and `error = %e`;
  - `Applied`/`Capped`: `info!(label, source = "report", client_w, client_h, top, capped, ratio, "window fitted")`;
  - `SkippedMaximized`/`SkippedMinimized`: `info!(label, reason = "maximized" | "minimized", pending = true, "fit skipped")`; `Deferred`: `debug!(label, "fit deferred")`.
- `pub fn fit_now<R: Runtime>(window: &tauri::Window<R>, state: &AspectState, source: &str)`: `window_facts`, `step`, apply through `platform::apply_fit` with the same re-mark on failure; `NoRatio` logs `debug!(label, "fit skipped: ratio unknown")`; an apply logs "window fitted" with `source`.
- `pub fn plugin<R: Runtime>() -> TauriPlugin<R>`: `tauri::plugin::Builder::new("window-aspect")`, `.setup(|app, _| { app.manage(Arc::new(AspectState::default())); Ok(()) })`, `.on_window_ready(|window| { .. })`: `window.try_state::<Arc<AspectState>>()`; missing logs `warn!(label, "window aspect state missing")` and returns; otherwise `platform::install(&window, Arc::clone(&state))`, then `fit_now(&window, &state, "ready")`.

- [ ] **Step 1: Tests first** in `window_aspect.rs` `mod tests` (fixture: work 1920x1032 at (0, 0), nc (16, 39), dpi 96, ratio 980/640), with `facts_from_getters` and `handle_report` declared with `unimplemented!()` bodies:
  - `facts_from_getters_rounds_dpi`: scale 1.5 gives dpi 144; 1.25 gives 120; client and outer rect follow the inputs.
  - `facts_from_getters_without_a_monitor_has_no_work_area`: `work: None` in gives `work: None` out.
  - `handle_report_rejects_without_reading_facts`: content 0 gives `Err(AppError::OutOfRange(_))`; the `facts` closure panics if called.
  - `handle_report_maximized_skips_and_marks_pending`: maximized facts give `Ok((SkippedMaximized, None))` and `is_pending()` true.
  - `handle_report_fitted_window_is_already_fitted`: client 1000x654 at ratio 980/640 gives `Ok((AlreadyFitted, None))`; the `apply` closure panics if called.
  - `handle_report_off_shape_applies`: client 1000x700; the `apply` closure records the plan it was given; the result is `Ok((Applied, Some(plan)))` and the recorded plan equals it.
  - `handle_report_facts_error_keeps_the_ratio`: `facts` returns `Err(AppError::Internal(..))`; the call returns that error and `ratio()` is `Some(980/640)`.
  - `handle_report_apply_failure_errs_and_keeps_pending`: off-shape facts, `apply` returns `Err(AppError::Internal(..))`; the call returns `Err`, `is_pending()` is true, `ratio()` is stored.
  - Run `cargo test --manifest-path src-tauri/Cargo.toml window_aspect::tests > .claude-work/window-hug/logs/T7-red.log 2>&1`.
  - Expected red: the eight new tests panic "not implemented"; Task 4's tests still pass.
- [ ] **Step 2: Validate the two hypotheses** in Interfaces (`Window::hwnd` was checked in Task 5; check `Monitor::work_area` now). Write the found signatures in REPORT.
- [ ] **Step 3: Implement** the Interfaces list and the lib.rs registration: `.plugin(window_aspect::plugin())` on the line after the window-state builder's closing `)`, and `window_aspect::set_content_height` in `generate_handler!`. An app command needs no capability entry.
- [ ] **Step 4:** Re-run the Step 1 command to `T7-green.log`. Expected: all pass.
- [ ] **Step 5:** Run the five gates (`<n>` = 7). Expected: all `exit=0`.
- [ ] **Step 6:** `grep -n "window_aspect::plugin" src-tauri/src/lib.rs` shows it on the line after the window-state plugin; `grep -n "clamp(\|unwrap()\|expect(\|unimplemented!" src-tauri/src/window_aspect.rs` matches only inside `mod tests` (and no `unimplemented!`).
- [ ] **Step 7 (orchestrator): launch smoke.** `npm run tauri dev` (PowerShell) with Settings' log level DEBUG: the log has "subclass installed" with a `dpi` field and "fit skipped: ratio unknown" at cold start, no WARN from `window_aspect` or `platform`, and an edge drag is free-form (no ratio yet). Record in `.claude-work/window-hug/manual/T7-smoke.md`.
- [ ] **Step 8:** Commit (procedure C): `Window hug 7: set_content_height command and the window-aspect plugin`.

**Interim behaviour:** the command exists but the frontend does not call it, so the ready hook fits only from a ratio stored earlier in the same process (none on a cold start) and drags stay free-form.

**Done when:** Steps 4-7 hold and the commit contains only the Files list.

---

### Task 8: Frontend reporting and the fit reducer

**Spec:** §3.1 "The measured box", "`subscribeContentHeight` and `attachContentHeight`", "`fitReducer`", "`createFitController`", "`useContentHeight`", "App wiring", "Backend"; §7.1 contentHeight, fit, backend and mockBackend rows; §7.2 `fit_outcomes_match_fit_ts`; §7.5 R27 row. **Rulings:** R2, R3, R10, R14, R17, R18, R23 (T2-M7, T2-M8), R27, R29. **Agent:** `implementer`. **Tier:** logic (`reviewer`). **Risk:** medium: the no-flash ordering. **Depends on:** Tasks 2 and 7.

**Why:** the window starts hugging here. The measurement, the command and the zoom's content height are pure, tested modules, so the hook is wiring only and the no-flash claim holds in either arrival order of the resize event and the IPC reply.

**Files:** `src/lib/contentHeight.ts` (new), `src/lib/contentHeight.test.ts` (new), `src/lib/fit.ts` (new), `src/lib/fit.test.ts` (new), `src/hooks/useContentHeight.ts` (new), `src/lib/backend.ts`, `src/lib/backend.test.ts` (new), `src/lib/mockBackend.ts` (next to `setAlwaysOnTop` at line 514), `src/lib/mockBackend.test.ts`, `src/App.tsx`, `src-tauri/src/window_aspect.rs` (`fit_outcomes_match_fit_ts` in `mod tests`).

**Interfaces:** Consumes: `windowZoom(w, h, contentH)` (Task 2); the `set_content_height` command returning the camelCase `FitOutcome` (Task 7). Produces:
- `src/lib/contentHeight.ts` (imports nothing from `backend.ts` or `@tauri-apps/*`):
  - `export type MeasuredElement = { offsetHeight: number }`;
  - `export type ContentObserverCtor = new (cb: (entries: readonly { borderBoxSize: readonly { blockSize: number }[] }[]) => void) => { observe(el: MeasuredElement, opts: { box: "border-box" }): void; disconnect(): void }`;
  - `export function subscribeContentHeight(element: MeasuredElement, observerCtor: ContentObserverCtor | undefined, report: (h: number) => void): () => void`, contract §3.1 items 1-4: one observer, `observe(element, { box: "border-box" })`, first report from the initial callback; each callback reads the last entry's `borderBoxSize[0].blockSize`, applies `Math.ceil(h - 1/64)`, reports only when finite, `> 0` and different from the last reported value; the returned function calls `disconnect()` and later callbacks report nothing; with `observerCtor` undefined, `console.warn("content height: ResizeObserver unavailable; reporting once")` once, then one synchronous `report(Math.ceil(element.offsetHeight - 1/64))` under the same rule, and a no-op unsubscribe;
  - `export function attachContentHeight(enabled: boolean, element: MeasuredElement | null, observerCtor: ContentObserverCtor | undefined, report: (h: number) => void): () => void`: disabled or null element constructs nothing, reports nothing, returns a no-op; otherwise `subscribeContentHeight(...)`.
- `src/lib/fit.ts` (no tauri or backend import):
  - `export const FIT_OUTCOMES = ["applied", "alreadyFitted", "capped", "skippedMaximized", "skippedMinimized", "deferred"] as const; export type FitOutcome = (typeof FIT_OUTCOMES)[number];` (one line, which the Rust drift test reads);
  - `export type FitState = { cEff: number | null; transit: number | null; lastMeasured: number | null; retry: boolean; viewport: { w: number; h: number } | null }`; `export const initialFitState: FitState` = all null, `retry: false`;
  - `export type FitEvent = { kind: "measured"; c: number } | { kind: "outcome"; c: number; outcome: FitOutcome } | { kind: "failed"; c: number } | { kind: "viewport"; w: number; h: number }`;
  - `holds(state, c)` = `viewport !== null && viewport.h * 980 >= (viewport.w - 1) * c` (integer arithmetic; use `BASE_WIDTH`);
  - `export function fitReducer(state: FitState, event: FitEvent): FitState`, transitions exactly §3.1: `measured` sets `lastMeasured`; `outcome`/`failed` with `c !== lastMeasured` change nothing; `applied`: `retry = false`, `transit = c`, then if `holds(state, c)`: `cEff = c`, `transit = null`; `capped`: `retry = false`, `cEff = c`, `transit = null`; `alreadyFitted`: `cEff = c`, `transit = null`, `retry = false`; `skippedMaximized`/`skippedMinimized`: `cEff = c`, `transit = null`, `retry = true`; `deferred`: `retry = true` only; `failed`: `cEff = c`, `transit = null`, `retry = false`; `viewport`: store it, then if `transit !== null && holds(state, transit)`: `cEff = transit`, `transit = null`;
  - `export function zoomContentH(state: FitState): number | null`: null while `cEff` and `transit` are both null; otherwise the minimum of the non-null values among `cEff`, `transit`, `lastMeasured`;
  - `export function shouldResend(state: FitState): boolean` = `retry && lastMeasured !== null`;
  - `export const RESEND_DEBOUNCE_MS = 200`;
  - `export type FitController = { onMeasured(c: number): void; onViewport(v: { width: number; height: number }): void; dispose(): void }`;
  - `export function createFitController(deps: { send: (c: number) => Promise<FitOutcome>; publish: (state: FitState) => void; warn: (message: string, error: unknown) => void }): FitController`: owns `FitState`; `onMeasured` applies `measured`, publishes, sends; a resolution applies `outcome{c, ..}` and publishes; a rejection calls `warn("content height: fit failed", e)` once, applies `failed{c}`, publishes; `onViewport` applies `viewport`, publishes, and when `shouldResend` (re)arms a 200 ms `setTimeout`; the timer sends `lastMeasured` only when `shouldResend` holds and no call is in flight (in flight: re-arm 200 ms); the resend applies no `measured`; `dispose` clears the timer and later resolutions apply and publish nothing.
- `src/hooks/useContentHeight.ts`: `export function useContentHeight(ref: RefObject<HTMLElement | null>, enabled: boolean, viewport: { width: number; height: number }): number | null`, wiring only (§3.1): `useState<FitState>(initialFitState)`; `controllerRef = useRef<FitController | null>(null)`; effect 1 on `[]` creates the controller inside the effect (`send = (c) => backend().setContentHeight(c)`, `publish = setState`, `warn = console.warn`), stores it in the ref, cleanup `dispose()` and `controllerRef.current = null` (R27; never `useRef(createFitController(..))`); effect 2 on `[enabled]` returns `attachContentHeight(enabled, ref.current, globalThis.ResizeObserver, (c) => controllerRef.current?.onMeasured(c))`; effect 3 on `[viewport]` calls `controllerRef.current?.onViewport(viewport)`; returns `zoomContentH(state)`. If `globalThis.ResizeObserver` is not assignable to `ContentObserverCtor` under tsc, adjust the `ContentObserverCtor` type, never with `any` or a double cast.
- `Backend.setContentHeight(localPx: number): Promise<FitOutcome>` (`import type { FitOutcome } from "./fit"`); real: `tauriInvoke<FitOutcome>("set_content_height", { contentH: localPx })`; mock: `async (localPx) => { console.info("mock: setContentHeight", localPx); return "alreadyFitted"; }`.
- `src/App.tsx`: `const appRef = useRef<HTMLElement>(null)` and `const contentH = useContentHeight(appRef, dashboard !== null, viewport)` above the early return (line 57); `ref={appRef}` only on the loaded branch's `<main className="app">` (line 65); `const zoom = windowZoom(viewport.width, viewport.height, contentH)`.

- [ ] **Step 1: Tests first.**
  - `src/lib/contentHeight.test.ts`, with a `FakeObserver` class that captures its callback, counts constructions, and records `observe` args and `disconnect` calls:
    - `first report comes from the observer's initial callback` (`observe` got `{ box: "border-box" }`; nothing before the callback; a 212 callback gives `[212]`);
    - `reports the ceiling of the border box less one layout unit` (200.2 gives 201, 640 gives 640, 640.004 gives 640);
    - `dedupes against the last value reported` (212, 211.6, 212 give one report; then 300 gives a second);
    - `ignores non-finite and non-positive sizes` (0 and NaN give none);
    - `reads borderBoxSize` (the fake entry has no `contentRect`);
    - `unsubscribe disconnects and later callbacks report nothing`;
    - `no ResizeObserver: warns once and reports offsetHeight once` (`offsetHeight` 212.4 gives `[213]`; `vi.spyOn(console, "warn")` called once);
    - `attachContentHeight: disabled or detached constructs no observer` (`enabled` false, and a null element: construction count 0, no report, the returned function runs);
    - `attachContentHeight: enabled subscribes` (one observer; its callback reports).
  - `src/lib/fit.test.ts`:
    - `FIT_OUTCOMES lists the six outcomes`;
    - `fitReducer` table, one row per §3.1 transition, including `a stale outcome is ignored`; `failed sets cEff`; `applied settles at once when the stored viewport holds it` (viewport (1225, 1125), C 900 to 640); `applied waits in transit until a viewport that holds it` (viewport (1225, 800), C 640 to 900; a (1225, 800) event keeps the transit, a (1225, 1125) event settles it); `holds tolerates one width px` ((1226, 1125) holds 900); `holds rejects one height px short` ((1226, 1124) does not hold 900); `capped settles at once` (viewport (1225, 800), C 900: `cEff` 900, `transit` null); `two reports in flight: the first fit's resize does not settle the second` (from settled `cEff` 640 at (1225, 800): measured(653), measured(666), outcome `applied{653}` ignored, viewport (1225, 816.25), outcome `applied{666}` (`transit` 666, `cEff` 640), viewport (1225, 832.5) settles `cEff` 666 and `transit` null; at every step `windowZoom(w, h, zoomContentH(state))` is 1.25);
    - `zoomContentH`: null for the initial state; `null after measured with no outcome`; `null after a first deferred`; the minimum of the non-null heights once `cEff` or `transit` is set;
    - `shouldResend`: true after `deferred` and each `skipped*`; false after `alreadyFitted`, `applied`, `capped`, `failed`;
    - `no zoom flash on grow or shrink, in either arrival order`: fitted window at zoom 1.25 (w 1225) with a settled first outcome; C 640 to 900 and 900 to 640; outcome before the resize event and resize event before the outcome; `windowZoom(w, h, zoomContentH(state))` is 1.25 after `measured`, after the first event and after the second (h = old `C * 1.25` until the resize event, new after); the settled state has `cEff` = new C and `transit` null;
    - `createFitController` with `vi.useFakeTimers()`, a fake `send` returning promises the test resolves or rejects, and `publish`/`warn` spies: `onMeasured publishes measured, sends c, then publishes the outcome for c`; `a rejection warns once and publishes failed for c`; `a stale resolution is ignored`; `a viewport event with retry resends lastMeasured after 200 ms without a measured event` (199 ms: 1 call; 200 ms: 2 calls with `lastMeasured`); `N viewport events while deferred produce one resend` (20 events 10 ms apart, then 200 ms: exactly one more call); `no resend while a call is in flight`; `no resend without retry` (after `alreadyFitted`, viewport events and 1 s: no call); `a timer that fires after retry cleared sends nothing`; `dispose cancels the timer and drops later resolutions`.
  - `src/lib/backend.test.ts` (new): `vi.mock("@tauri-apps/api/core", ..)` with an `invoke` spy resolving `"applied"`, and `vi.mock` of `@tauri-apps/api/event` and `@tauri-apps/api/window`; `setContentHeight invokes set_content_height with the camelCase key`: `backend().setContentHeight(640)` resolves `"applied"` and `invoke` saw `("set_content_height", { contentH: 640 })`.
  - `src/lib/mockBackend.test.ts`: `setContentHeight records calls and resolves alreadyFitted` (`vi.spyOn(console, "info")`; `createMockBackend().setContentHeight(640)` resolves `"alreadyFitted"`; the spy saw `("mock: setContentHeight", 640)`).
  - `src-tauri/src/window_aspect.rs` `mod tests`: `fit_outcomes_match_fit_ts`: `include_str!("../../src/lib/fit.ts")`'s line starting `export const FIT_OUTCOMES` lists exactly the six `serde_json` names of the `FitOutcome` variants, in order (same pattern as tray.rs:666).
  - Create `contentHeight.ts` and `fit.ts` exporting the Interfaces' names with bodies that `throw new Error("not implemented")` (types complete), so the red run fails on behaviour, not on module resolution.
  - Run `cmd /c "npx vitest run src/lib/contentHeight.test.ts src/lib/fit.test.ts src/lib/backend.test.ts src/lib/mockBackend.test.ts > .claude-work\window-hug\logs\T8-red.log 2>&1"` and `cargo test --manifest-path src-tauri/Cargo.toml fit_outcomes_match_fit_ts >> .claude-work/window-hug/logs/T8-red.log 2>&1`.
  - Expected red: the contentHeight and fit rows fail with "not implemented" (except `FIT_OUTCOMES lists the six outcomes`, which passes on the constant); backend and mockBackend rows fail with "setContentHeight is not a function"; `fit_outcomes_match_fit_ts` passes (the constant line exists), which is correct for a drift guard.
- [ ] **Step 2: Implement** the Interfaces list, then wire `App.tsx`.
- [ ] **Step 3:** Re-run both Step 1 commands to `T8-green.log`. Expected: all pass. Then run the fit and contentHeight files three times in a row (pass^3, fake timers), PowerShell: `1..3 | ForEach-Object { cmd /c "npx vitest run src/lib/fit.test.ts src/lib/contentHeight.test.ts >> .claude-work\window-hug\logs\T8-green.log 2>&1"; "run $_ exit=$LASTEXITCODE" }`. Expected: three `exit=0`.
- [ ] **Step 4:** Run the five gates (`<n>` = 8). Expected: all `exit=0`.
- [ ] **Step 5:** `grep -n "from \"@tauri-apps\|from \"./backend\"" src/lib/fit.ts src/lib/contentHeight.ts` matches nothing (R14); `grep -n "useRef(createFitController" src/hooks/useContentHeight.ts` matches nothing (R27).
- [ ] **Step 6 (orchestrator): StrictMode dev check (R27).** `npm run tauri dev` (PowerShell; StrictMode is active, src/main.tsx:16). Maximize the window: the content shows whole with side slack and no vertical overflow. Restore, open a history drawer: the window grows in one step with no rescale. Record yes/no for each in `.claude-work/window-hug/reports/T8.md`. A failure (maximized content overflows the bottom) means a disposed controller is reused: return to the implementer.
- [ ] **Step 7:** Commit (procedure C): `Window hug 8: report the content height and drive the zoom from the fit outcome`.

**Interim behaviour:** none; this completes the feature. The window hugs its content from here on.

**Done when:** Steps 3-6 hold and the commit contains only the Files list.

---

### Task 9: M10 harness and run

**Spec:** §7.4 (every S and H criterion), §8 task 9 and the hypotheses list. **Rulings:** R12, R15, R16, R23 (T2-M4), R24, R28. **Agent:** `implementer` writes and selftests the harness; the orchestrator runs M10. **Tier:** tests-only (no per-task review; the whole-plan review is its net). **Risk:** medium: Win32 timing from PowerShell. **Depends on:** Task 8.

**Why:** drag feel, snap ordering, DPI changes and WebView2's real zoom numbers are observable only on the real window; every goal item without an automated observer has a numeric yes/no check here.

**Files** (untracked work folder; no tracked change, no commit): `.claude-work/window-hug/manual/harness.psm1`, `.claude-work/window-hug/manual/run-checks.ps1`, `.claude-work/window-hug/manual/selftest.ps1` (each copied from `.claude-work/optimize/manual/`, then extended), `.claude-work/window-hug/manual/M10.md`.

**Interfaces:** Consumes: the log lines of Tasks 6-7 ("content height report" DEBUG with `content_h` and a 6-decimal `ratio`, "window fitted", "size-move end", "subclass installed" with `dpi`; targets `cut_core::window_aspect` and `cut_core::platform::windows::aspect`); the optimize harness (`CutWin` at harness.psm1:19, `ConvertFrom-LogLine` at 78, `Set-WindowGeometry` at 236, `Use-Settings` at run-checks.ps1:66, `Start-AppInstance` at 111, `Stop-AppGraceful` at 172, `ValidateSet` at 26). Produces:
- `CutWin` gains `GetClientRect` and `ClientToScreen`.
- `Test-AspectShape -ClientW -ClientH -ContentH` (operands `[int64]`): `want = [math]::Floor(($ClientW * $ContentH + 979) / 980)`; passes exactly when `ContentH` is an integer `> 0` and `0 <= ClientH - want <= 1`.
- `Get-ContentReport -Since`: DEBUG "content height report" lines parsed to `{ content_h (int), ratio (string), outcome }`.
- `ConvertFrom-LogLine` gains `Target = [string]$o.target`; `Get-AspectWarnLines -Ctx`: WARN events since `$Ctx.Since` whose `Target` starts with `cut_core::window_aspect` or `cut_core::platform`.
- A sampler at about 2 ms: one `GetClientRect` per sample for the shape; a separate `GetWindowRect` stream for anchors only.
- `M10` in `run-checks.ps1`'s `-Check` `ValidateSet`.

- [ ] **Step 1: Copy** the three harness files from `.claude-work/optimize/manual/` to `.claude-work/window-hug/manual/` (`mkdir -p` first).
- [ ] **Step 2: Selftest cases first** in `selftest.ps1`, before the judge, parser and filter exist:
  - `Test-AspectShape`: with `ClientW 980, ContentH 640`, `ClientH 640` (+0) and `641` (+1) pass; `642` (+2) and `639` (-1) fail; `ContentH 0` and `-5` fail. `ClientW 1960, ContentH 1282`: `ClientH 2564` passes, `2563` fails. `ClientW 1960, ContentH 641`: `ClientH 1282` passes, `1281` fails.
  - `Get-ContentReport`: a sample DEBUG JSON line with `content_h` 654 and `ratio` "1.498471" parses to `content_h` `[int] 654` and `ratio` `"1.498471"`.
  - `Get-AspectWarnLines`: three sample lines; WARN with target `cut_core::window_aspect` and WARN with target `cut_core::platform::windows::aspect` are returned; WARN with target `cut_core::poller` is not.
  - Run (PowerShell): `powershell -NoProfile -File .claude-work\window-hug\manual\selftest.ps1 *> .claude-work\window-hug\logs\T9-red.log; "exit=$LASTEXITCODE"`. Expected red: the new cases fail with "The term 'Test-AspectShape' is not recognized" (and the same for `Get-ContentReport`, `Get-AspectWarnLines`); the copied optimize cases still pass.
- [ ] **Step 3: Implement** the Interfaces list, then the M10 check in `run-checks.ps1`, criteria exactly §7.4:
  - Settings per check (R28): S1 `Use-Settings -DebugLevel $true -CloseToTray $false`; S4 and S6 `-CloseToTray $true`; S2, S3, S5 the harness default `$true`.
  - **S1 cold start (R15):** preparation launch, record the first report's `content_h` C and client width `cw`, `Stop-AppGraceful`; back up `.window-state.json` in `%APPDATA%\<identifier>` (identifier from `tauri.conf.json`), set the `main` entry's `height` to `ceil(cw * C / 980) + 40`. S1a: launch; the first report says `applied`, a "window fitted" `{source: report}` follows, `Test-AspectShape` passes on `GetClientRect` with that `content_h`. S1b: stop gracefully, launch unedited; the first report says `alreadyFitted`, no "window fitted" `{source: report}` within 5 s, `Test-AspectShape` passes. Both launches: "subclass installed" present with `Target` starting `cut_core::platform`; `Get-AspectWarnLines` empty; each `Stop-AppGraceful` returns `Invoked` and `Exited` true (otherwise S1 FAILS; never force-kill). Restore the backup in a `finally` block.
  - **S2 drags:** centre with `Set-WindowGeometry`; for each of the 8 hit points `room` = free px outward to the work-area edge (corner: the smaller), `s = min(10, floor(room / 30))`, `s < 3` is INCONCLUSIVE and named; mouse down 2 px inside the edge, 30 `SetCursorPos` steps of `s` outward, 30 back, mouse up. Pass: every sampled client frame passes `Test-AspectShape` with the latest `content_h`; the opposite edges move 0 px; each drag logs one "size-move end" with `0 <= ratio_err_px <= 1`.
  - **S3 maximize:** `cap = (client top in screen px - window top) - (window bottom - client bottom in screen px)` before; after `ShowWindow(SW_MAXIMIZE)`: `IsZoomed` true, client width = work width +-2, client height = work height - `cap` +-2; after restore the window rect equals the pre-maximize rect +-2.
  - **S4 reopen after a custom width:** `SetWindowPos` to client width 1300, close to tray, reopen: client width 1300 +-2, `Test-AspectShape` passes, and either "window fitted" `{source: ready}` or no fit line.
  - **S5 zoom independence:** `s = dpi / 96` from this launch's "subclass installed"; client width `round(980 * s)`, wait for the fit, drag the right edge to `round(1470 * s)`; INCONCLUSIVE when that exceeds the work width less the frame, or when the drag's "size-move end" `dpi` differs. Pass: no new report, or each new `content_h` within +-1 of the pre-drag value; record the exact values.
  - **S6 loading does not collapse:** from tray reopen to 1 s after the first report, no sampled client height is below 0.9 x the pre-close height.
  - H list in `M10.md` as yes/no prompts, exactly §7.4: H1, H2, H4, H5, H6, H7, H8, H9, H10, H11 (both reports >= 100), H12; H4b only if Task 6 deleted `zoomed_window_keeps_pending`; H3 is retired (R16) and stays listed as retired.
- [ ] **Step 4:** Re-run the selftest to `T9-green.log`. Expected: `exit=0`, all cases pass.
- [ ] **Step 5 (orchestrator): build and run.** PowerShell: `cmd /c "npm run tauri -- build --no-bundle > .claude-work\window-hug\logs\T9-build.log 2>&1"; "exit=$LASTEXITCODE"`, then `powershell -NoProfile -File .claude-work\window-hug\manual\run-checks.ps1 -Exe src-tauri\target\release\claude-usage-tracker.exe -Check M10 *> .claude-work\window-hug\logs\T9-M10.log`. UNSURE: the optimize run's exact build flags are not recorded; the exe path is (run-checks.ps1:20).
- [ ] **Step 6 (orchestrator): human checks.** Answer H1, H2, H4-H12 (and H4b if present) yes or no in `M10.md`, with the log lines or numbers each cites.
- [ ] **Step 7:** `git status --porcelain` is empty (no tracked change), and the window-state file's hash equals the backup's.

**Interim behaviour:** none; no tracked file changes.

**Done when:** S1-S6 pass (an S2 hit point INCONCLUSIVE for lack of room is named in `M10.md` and re-run on a larger monitor); every H answer is recorded; the window-state file is restored; any "no" or FAIL is returned to the orchestrator as a design finding, never tuned in place.

---

### Task 10: Docs

**Spec:** §1 goal items 1-7, §2. **Agent:** `implementer`. **Tier:** docs (no per-task review). **Risk:** low. **Depends on:** Task 9 (the documented behaviour is the measured one).

**Why:** a user and the next developer read the README to learn why the window resizes itself and where its limits are.

**Files:** `README.md` (a new `### Window size and shape` subsection under `## How it works`, line 17), `CHANGELOG.md` (entries under the existing `## [Unreleased]`; no version heading).

- [ ] **Step 1:** README: the window hugs its content (no slack, no scrollbar); any edge or corner drag keeps the shape (Windows; other platforms only resize on a content change); content changes resize the window at the current width; maximized shows the 980-wide layout scaled and centred with slack; beyond 2450 x scale px width the content stops growing; content taller than the work area moves the window up, caps its height and shrinks the content, and at the 735 minimum width the root scrolls; the narrow and cards layouts are gone; logs to read: `rg '"window fitted"|"size-move end"|"fit skipped"'` in the log directory.
- [ ] **Step 2:** CHANGELOG under `## [Unreleased]`: `### Changed` (content-hugging, ratio-locked window; two-term zoom; `minWidth` 735; fixed 220 px history chart), `### Removed` (narrow and cards layouts; window-derived chart height).
- [ ] **Step 3:** PowerShell: `cmd /c "npm run build > .claude-work\window-hug\logs\T10-build.log 2>&1"; "exit=$LASTEXITCODE"`. Expected: `exit=0`.
- [ ] **Step 4:** Commit (procedure C): `Window hug 10: document the content-hugging window`.

**Done when:** Step 3 holds, the two files describe §1's behaviour, and the commit contains only the two files.

---

## Whole-plan review

- After Task 10, dispatch `hostile-reviewer` with DIFF = `git diff c683bec..HEAD > .claude-work/window-hug/review/whole-plan.diff`, RULINGS = `.claude-work/window-hug/review/RULINGS-window-hug.md`, the spec, this plan, `M10.md` and the task REPORTs. Lenses: proc memory safety and panic containment, every skip/defer/apply-failure path has a resume path, one owner per constant, no-flash ordering, logging per §6.
- Fix every Critical and Important; a scoped `reviewer` re-review of each fix diff; loop until a round returns 0 Critical and 0 Important; then fix the Minors together with one scoped re-review.
- Append one line per review to `~/.claude/review-ledger.jsonl` (repo `ClaudeUsageTracker`, plan `window-hug`).
- No merge: Josh decides (public repository).

## Goal coverage

| Spec §1 goal | Automated | M10 |
|---|---|---|
| 1. Shape 980 : H, no slack, no scrollbar | T1 css-contract `.app is content-sized and fixed-width`; T4 `plan_fit_table`, `fit_client_table`; T8 `contentHeight.test.ts` | S1, S5, H7 |
| 2. Drag keeps the ratio within 1 px | T4 `fit_rect_*`; T6 `sizing_rewrites_every_edge` | S2, H1, H6, H12 |
| 3. Follows content in one resize, no zoom flash | T4 `step_table`; T7 `handle_report_*`; T8 `fitReducer` table, `no zoom flash on grow or shrink, in either arrival order`, `createFitController` rows | H2, H10 |
| 4. Maximized keeps the monitor's shape; 2450 cap | T2 `windowZoom` `maximized: height term wins`; T4 `client_bounds_table` (4K), `fit_rect_zoom_max_cap` | S3, H8 |
| 5. Capped: move up, cap, min width wins, no panic | T4 `fit_client_table`, `plan_fit_table`, `fit_rect_height_cap`, `fit_rect_crossed_bounds`, `run_guarded_*`; T6 `a_panic_after_the_forward_forwards_once` | — (UNSURE: no M10 check drives content taller than the work area) |
| 6. Windows-only drag lock; other platforms compile | T5/T6 `hwnd_tests`; `other.rs` reviewed only (UNSURE: no non-Windows build in the gates) | S1 "subclass installed" |
| 7. Narrow and cards layouts removed | T3 css-contract `no card-layout rules remain`; `npm run build` | — |

Resume paths (R4, R19, R20): T4 `step_table`; T6 `exit_size_move_applies_pending_synchronously`, `size_restored_applies_pending`, `dpi_changed_*`, `apply_failure_in_the_proc_keeps_pending`, `zoomed_window_keeps_pending`; T7 `handle_report_apply_failure_errs_and_keeps_pending`; T8 `shouldResend` and the resend rows; M10 H4, H5, H9.

Hypotheses carried from spec §8 (none blocks a task; each has its check): DefWindowProc re-clamping (H12); snap and Win+Arrow ordering (H5); corner "cover" feel (H6); `IsZoomed` on a hidden window (Task 6 Step 4); `MIN_CONTENT_H` margin (H11); a content change during a held drag fits on release (rests on T6 `exit_size_move_applies_pending_synchronously` and T4 `step_table`; no end-to-end check, R16); `Math.ceil(h - 1/64)` stability (S5); `innerHeight` rounding at 125%/175% (not run; bounded to a zoom up to one width px low); `Window::hwnd` and `Monitor::work_area` signatures in tauri 2.12.1 (Task 5 and Task 7 greps).
