# SDD ledger — plan: docs/superpowers/plans/2026-09-15-claude-usage-tracker.md

Worktree: C:/Users/josh/ClaudeUsageTracker-v1 (branch feat/v1, base 4374e0c)
Spec: docs/2026-09-15-claude-usage-tracker-design.md (v5 final)
Model policy: implementers sonnet (opus for Task 20), task reviewers sonnet (opus for Tasks 15, 19, 20, 21), final review opus. Never fable.

## Pre-flight scan (see preflight-scan.md for the full tables)
3 MISMATCH rows, all header-vs-body summary gaps; 0 consume-without-producer; 0 Global Constraint conflicts.
- Ruling: Task 1 Files header omits error.rs and bin/fake_claude.rs which Steps 8/9 create — the steps are authoritative; implementer creates both — costs nothing if wrong (reviewer sees the diff).
- Ruling: Task 2 header says "Modify lib.rs" but no step touches it — no lib.rs change in Task 2 unless a step needs it — costs one compile error caught by the gate if wrong.
- Ruling: Task 19 Produces omits Dashboard/AccountRow/BinaryInfo/RawSnapshot DTOs that its body defines and Task 22 consumes — body is authoritative; Task 22 dispatch will name them — costs a reviewer ⚠️ if wrong.

## Task log
Task 1: dispatched (implementer sonnet, BASE 4374e0c)
Task 1: implementer DONE_WITH_CONCERNS (33ebd58). Notes: icon.png converted to RGBA (generate_context! requires it); vite `passWithNoTests` added; scaffold left src/App.css — Task 23 creates src/styles.css and must delete App.css (carry into Task 23 dispatch). Review dispatched (sonnet).
Task 1: review (sonnet) — spec ❌ 1 Important (cargo test gate not run, plan-mandated: brief step 12 said cargo check). ⚠️ trailers verified by controller via git log: OK. ⚠️ socket npm install: accepted on lockfile evidence. Minor deferred: scaffold extras (README, .vscode, public/*.svg, template App.tsx greet call, tsconfig.node.json) and App.css — clean up in Task 23.
Task 1: fix round 1/5 dispatched (resume implementer): run cargo test.
Task 1: fix round 1/5 (1 addressed, 0 open — cargo test run, 4 harnesses ok; no new commit)
Task 1: complete (commits 4374e0c..33ebd58, review clean after 1 fix round)
Task 2: dispatched (implementer haiku — single-file transcription, BASE 33ebd58)
Task 2: implementer DONE (bb8e2e4, 5 tests). Review dispatched (sonnet).
Task 2: complete (commits 33ebd58..bb8e2e4, review clean)
Task 3: dispatched (implementer haiku, BASE bb8e2e4)
Task 3: implementer DONE (2c71358, 8 tests). Review dispatched (sonnet).
Task 3: complete (commits bb8e2e4..2c71358, review clean)
Task 4: dispatched (implementer sonnet, BASE 2c71358)
Task 4: implementer DONE (bfbfe02, 19 new tests). Ruling: `.gitattributes` added outside the brief's file list to stop core.autocrlf stripping CR bytes from the CRLF fixture on commit — accepted; the fixture's bytes are the test, so the attribute is load-bearing — costs nothing if wrong. Review dispatched (sonnet).
Note: global git core.autocrlf=false, core.eol=lf set 2026-09-16 at Josh's request (system config had autocrlf=true). Carry into Task 24 dispatch: extend .gitattributes with '* text=auto eol=lf' + CRLF for *.cmd/*.bat/*.ps1.
Task 4: complete (commits 2c71358..bfbfe02, review clean)
Task 5: dispatched (implementer sonnet, BASE bfbfe02)
Task 5: implementer DONE (8c091b9, 9 tests). Note: sysinfo 0.39.6 System::new() infallible; wrapper keeps AppResult signature. Review dispatched (sonnet).
Task 5: complete (commits bfbfe02..8c091b9, review clean; minor deferred: loop var name proc_)
Task 6: dispatched (implementer haiku, BASE 8c091b9)
Task 6: implementer DONE (1ad6bc0, 7 tests). Review dispatched (sonnet).
Task 6: review clean (0 Critical/0 Important). Ruling: test module uses explicit imports instead of the brief's `use super::*` — accepted, behaviour identical — costs nothing if wrong.
Task 6: complete (commits 8c091b9..1ad6bc0, review clean)
Task 7: dispatched (implementer sonnet, BASE 1ad6bc0)
Task 7: implementer DONE (5e1bd5a, 18 tests; test-only clippy tweak from_ref). Review dispatched (sonnet).
Task 7: review clean. ⚠️ D17 default-first ordering: controller verified Task 10's list_accounts orders `is_default DESC, label COLLATE NOCASE` — resolved. Minor deferred: info! log literals duplicate BinarySource::as_str().
Task 7: complete (commits 1ad6bc0..5e1bd5a, review clean)
Task 8: dispatched (implementer sonnet, BASE 5e1bd5a)
Task 8: implementer DONE (4a242f2, 8 tests; test-only clippy tweak redundant_closure). Review dispatched (sonnet).
Task 8: complete (commits 5e1bd5a..4a242f2, review clean; minor deferred: migrate DDL + user_version bump not in one transaction — wrap before V2)
Task 9: dispatched (implementer haiku, BASE 4a242f2)
Task 9: implementer DONE (39d878c, 14 tests). Review dispatched (sonnet).
Task 9: fix round 1/5 dispatched (controller finding: trailer said 'Claude Haiku 4.5' — amend message only). Note for future haiku dispatches: state 'do not substitute your own model name in the trailer'.
Task 9: fix round 1/5 (1 addressed, 0 open — message-only amend 39d878c→3addccb; controller verified trailer, clean tree, zero content diff; no re-review needed for a message-only change)
Task 9: complete (commits 4a242f2..3addccb, review clean; minor deferred: get_u32/get_bool silent default on corrupt value — add DEBUG log)
Task 10: dispatched (implementer sonnet, BASE 3addccb)
Task 10: implementer DONE (8350487, 14 tests). Review dispatched (sonnet).
Task 10: review ⚠️ cascade-on-removal not tested in Task 10 — controller verified Task 11 brief has removing_an_account_cascades_its_snapshots; resolved. Awaiting issues/verdict tail.
Task 10: review — 2 Important (plan-mandated). Ruling: #2 cascade test is Task 11's (brief has removing_an_account_cascades_its_snapshots) — not a Task 10 defect. Ruling: #1 check-then-insert across two mutex acquisitions can surface UNIQUE violation as `db` instead of `duplicate` — real; fix by doing check+insert in one with_conn and mapping constraint errors to Duplicate — costs one small diff if wrong. Minor deferred: mark_guard_tripped no NotFound; corrupt disabled_reason → None silently; to_string_lossy on config_dir.
Task 10: fix round 1/5 dispatched (resume implementer)
Task 10: fix round 1/5 implemented (3bfea55: single-closure add/seed + constraint→Duplicate mapping, 1 new test); scoped re-review dispatched (sonnet)
Task 10: fix round 1/5 (1 addressed, 0 open; commits 8350487..3bfea55). Minor deferred: seed_accounts_if_empty holds the store mutex across build_account fs calls — hoist build_account out of the closure.
Task 10: complete (commits 3addccb..3bfea55, review clean after 1 fix round)
Task 11: dispatched (implementer sonnet, BASE 3bfea55)
Task 11: implementer DONE (7bb1b01, 13 tests). Review dispatched (sonnet).
Task 11: complete (commits 3bfea55..7bb1b01, review clean; minor deferred: latest_per_account per-row correlated subquery; corrupt outcome column → parse_error silently)
Task 12: dispatched (implementer sonnet, BASE 7bb1b01)
Task 12: implementer DONE (2806359, 35 tests). Review dispatched (opus — halt/backoff/escalation logic).
Task 12: review (opus) Approved; impl byte-identical to brief, 35 tests. Important (plan-mandated): decide()+begin_cycle() are two lock acquisitions so two concurrent callers could both start a cycle and CycleToken::drop clears busy without ownership check. Ruling: PARKED, load-bearing for Task 20 — the driver's single select! loop task is the ONLY caller of decide/begin_cycle (commands hold no Machine handle), so no concurrent caller exists; Task 20 dispatch must state this invariant in a code comment and the Task 20 reviewer must verify no second caller. If wrong, the cost is a duplicate cycle (bounded by per-poll timeout), not a quota spend. Minor deferred: CycleToken drop while holding guard deadlocks (driver must not); record re-escalates past 5th strike (moot after halt); dead `consecutive_failures > 0` checks; `Backoff` pub unused; `status` ignores now; missing tests: Startup obeys backoff, preview_manual parity vs decide(Manual).
Task 12: complete (commits 7bb1b01..2806359, review clean, 1 parked)
Task 13: dispatched (implementer haiku, BASE 2806359)
Task 13: implementer DONE (8343b73, 6 tests). Review dispatched (sonnet).
Task 13: complete (commits 2806359..8343b73, review clean; minor deferred: echo-env test inherits ambient env; json_escape untested for quote/backslash values)
Task 14: dispatched (implementer sonnet, BASE 8343b73)
Task 14: implementer DONE (06726ac, 19 tests). Review dispatched (opus — guard classification).
Task 14: review (opus) Approved on classification (13/13 adversarial envelopes correct). Important (plan-mandated): env strip list is case-sensitive; Windows env names are case-insensitive so `anthropic_api_key` would be inherited. Ruling: strip case-insensitively (uppercase the name before prefix test, return the original spelling); brief's test pinning lowercase-as-kept is inverted — spec D15 intent is "never inherit an Anthropic/Claude variable". Costs nothing if wrong (over-stripping a lowercase var on Unix is harmless). Minor deferred: advisory fields of wrong JSON type skipped silently; "has no local_command field" message wrong for present-non-string; leading stdout noise before JSON → Shape (carry to Task 15: stdout capture); test helper `shape` shadows impl helper.
Task 14: fix round 1/5 dispatched (resume implementer)
Task 14: fix round 1/5 implemented (da02fd1 case-insensitive strip; controller docs commit bc6fcdb amends D15); scoped re-review dispatched (sonnet)
Task 14: fix round 1/5 (1 addressed, 0 open; commits 06726ac..da02fd1)
Task 14: complete (commits 8343b73..da02fd1, review clean after 1 fix round)
Task 15: dispatched (implementer sonnet, BASE da02fd1)
Task 15: implementer DONE (6f254d9, 13 integration tests; 2 brief bugs fixed: stdout trim, pid_slot cleared too early; suite needs --test-threads=1). Review dispatched (opus).
Task 15: review (opus) Needs fixes — 4 Important: (1) PID slot never cleared on success path (implementer removed the clear; stale PID could exclude a recycled real claude PID); (2) read_to_string drops all output on one invalid UTF-8 byte (plan-mandated; spec says from_utf8_lossy); (3) env test would pass against env_clear() because fake's echo-env filters to the strip prefixes; (4) tests mutate process env concurrently with run_usage reading it → UB/flaky; --test-threads=1 is a hidden requirement. Rulings: fix all four. For (4): file-wide `static ENV_LOCK: Mutex<()>` taken by EVERY test in runner_guard.rs (serialises that binary only; other test binaries are separate processes) — cheaper than re-plumbing the fake to read a mode file, and sound because no code path in that binary reads env without holding the lock. Same pattern is required in Tasks 17/20/21/24 (carry forward). For (3): extend fake echo-env prefix filter with `CUT_TEST_` so a `CUT_TEST_SENTINEL` set by the test proves the parent env is inherited, not wiped. Costs if wrong: a flaky test binary, no runtime effect. Minor deferred: cancel reported as SpawnError; stderr tail persisted unredacted; three swappable &Path params; cwd/ensure_dir failure untested; sub-second timeout → Timeout(1); empty stdout stored as Some("").
Task 15: fix round 1/5 dispatched (resume implementer)
Task 15: fix round 1/5 implemented (3a8a158); test total 195→191 flagged for re-reviewer; scoped re-review dispatched (sonnet)
Task 15: fix round 1/5 (4 addressed, 0 open; commits 6f254d9..3a8a158; no tests deleted, 195 was a miscount)
Task 15: complete (commits da02fd1..3a8a158, review clean after 1 fix round). Carry-forward: ENV_LOCK pattern required in any later test file that mutates process env (Tasks 17/20/21/24); std MutexGuard held across .await in tests deserves a comment.
Task 16: dispatched (implementer haiku, BASE 3a8a158)
Task 16: implementer DONE (e2c509f, 12 tests). Review dispatched (sonnet).
Task 16: complete (commits 3a8a158..e2c509f, review clean; minor deferred: no halted+empty-list test)
Task 17: dispatched (implementer sonnet, BASE e2c509f)
Task 17: implementer DONE (cb9ceb0, 4 tests; merged two init tests to avoid global-subscriber double init; LogHandle owns WorkerGuard — must be held in Tauri state by wiring task). Review dispatched (sonnet).
Task 17: review — 1 Important: merged test never asserts a debug line is admitted after set_level("debug") (a no-op reload would pass). Minor: report text overclaims that coverage. Test-design deviation (merging the brief's two init tests) accepted by the reviewer as the correct fix.
Task 17: fix round 1/5 dispatched (resume implementer)
Task 17: fix round 1/5 implemented (a993795); scoped re-review dispatched (haiku)
Task 17: fix round 1/5 (1 addressed, 0 open; commits cb9ceb0..a993795)
Task 17: complete (commits e2c509f..a993795, review clean after 1 fix round). Carry to Task 21: LogHandle (owns WorkerGuard + reload handle) must be held in Tauri managed state for process lifetime.
Task 18: dispatched (implementer sonnet, BASE a993795)
Task 18: implementer DONE (d226406, 10 tests). Review dispatched (sonnet).
Task 18: complete (commits a993795..d226406, review clean, 0 findings). Carry to Task 21: startup empty_dir(login_script_dir) call.
Task 19: dispatched (implementer sonnet, BASE d226406)
Task 19: implementer DONE (a7827c1, 23 tests; 4 compile-driven tweaks). Review dispatched (opus — command surface + DTO contract).
Task 19: complete (commits d226406..a7827c1, review clean; minor deferred: sync fs/autostart calls off the blocking hop in open_log_dir/get_settings/set_settings; set_settings split state if log reload fails mid-save; 4 untested paths). Carry to Task 20: take_changed and its notify permit are independent — drain after consuming the permit, and treat an empty vector as a no-op.
Task 20: dispatched (implementer opus, BASE a7827c1)
Task 20: implementer DONE (63f344f, 10 driver_loop + unit tests; 2 brief design bugs fixed: busy status stale until next loop wake → generation-tagged completion channel; AccountChanged ids drained before decide → deferred+re-notified). Ruling (spec gap): Machine::reset_backoff (Manual/AccountChanged) and reset_all_backoff (settings) must clear next_allowed/consecutive_failures but PRESERVE unexpected_envelope_streak; the streak clears only on a non-strike outcome in record(). Reason: the streak is guard evidence; letting a Refresh click wipe it means five unclassifiable envelopes (each possibly a spent turn) never escalate. Costs if wrong: an over-eager halt after 5 bad envelopes across manual refreshes — recoverable via Clear halt. Task 12 test that asserts the streak is cleared by reset_backoff is inverted. Spec §6.3 step 5 to be amended. Dispatched to implementer as pre-review round 0.
Task 20: round 0 applied (22517bd: streak survives resets, 3 machine tests, five-strike integration test restored). Review dispatched (opus) over a7827c1..22517bd.
Task 20: review (opus) Needs fixes — 2 Important, both watchdog arm: (1) publish() right after JoinHandle::abort() reads busy=true (token not yet dropped) and the generation guard then discards the late CycleDone → Refresh refused for up to an interval after a stall; (2) aborted cycle leaves a dead PID in pid_slot → stale exclude_pid could mask a real claude. Minor→required (flake risk): tests at driver_loop.rs:247 (sleep 500ms then assert busy) and :336 (five manual polls at 700ms, results ignored). Changes 2a/2b judged sound.
Task 20: fix round 1/5 dispatched (resume implementer)
Task 20: R1 single-caller invariant verified by reviewer (decide/begin_cycle only from the loop task; documented driver.rs:600-608) — Task 12 parked item RESOLVED. Halt sequence order + single log site verified. Minor deferred: settings_rx.changed() Err would hot-spin (document Core owns sender); changed_deferred re-notify duplicated ×3; non-Busy Skip drains account-changed ids.
Task 20: fix round 1/5 implemented (6299b41); scoped re-review dispatched (opus — watchdog concurrency)
Task 20: fix round 1/5 (4 addressed, 0 open; commits 22517bd..6299b41). Minor deferred: watchdog join-timeout branch (>2 s abort) still publishes busy=true and drops LiveCycle so the late CycleDone is discarded — keep LiveCycle or skip publish on that branch; loop blocked ≤2 s during join (accepted); stalled_at cleared unconditionally by backstop reap/done_rx.
Task 20: complete (commits a7827c1..6299b41, review clean after round 0 + 1 fix round)
Task 21: dispatched (implementer sonnet, BASE 6299b41)
Task 21: implementer DONE (3880aa1; icon_rgba badge-slot bug fixed; release binary links; npm build passes; ExitRequested flat 2.5 s sleep flagged). Review dispatched (opus).
Task 21: review (opus) Needs fixes — Critical: tauri.conf.json `app.trayIcon` makes Tauri build a tray before setup; setup builds a second one with the same id; tray_by_id returns the first (inert) → duplicate icon, updates go to the dead one. Important: (2) tray not refreshed after account/settings/clear_halt commands (plan-mandated); (3) apply_tray makes 3 sync Store calls on the async runtime (plan-mandated); (4) polling_halted().unwrap_or_default() fails open + silent. Minor deferred: flat 2.5 s exit sleep; double-Quit second timer; setup `?` errors panic via Tauri.
Task 21: fix round 1/5 dispatched (resume implementer)
Task 21: spec verdicts OK for plugin order, LogHandle in Core→manage, token reachable, close-to-tray polarity, tray Refresh via core_poll_now, capabilities. Additional minor deferred: setup ? errors panic via Tauri; latest_per_account error swallowed in tray.rs; sync store read on UI thread in close handler (lib.rs:206); mid-file use statements.
Task 21: fix round 1/5 implemented (f6ff1ea); scoped re-review dispatched (sonnet)
Task 21: fix round 1/5 (4 addressed, 0 open; commits 3880aa1..f6ff1ea). Minor deferred: detached apply_tray tasks per event have no ordering guarantee (single-flight/debounce).
Task 21: complete (commits 6299b41..f6ff1ea, review clean after 1 fix round). BACKEND COMPLETE.
Task 22: dispatched (implementer sonnet, BASE f6ff1ea)
Task 22: implementer DONE (dee42ac, 36 vitest). TS type names Win/AppErrorShape per brief (not wire). Review dispatched (sonnet).
Task 22: complete (commits f6ff1ea..dee42ac, review clean; minor deferred: clockOf uses local time (intentional, comment); >HOUR gap threshold — resolved: Task 11 buckets are floored to exact hours)
Task 23: dispatched (implementer sonnet, BASE dee42ac)
Task 23: implementer DONE (71dd324; build+tsc clean; scaffold leftovers removed; index.html favicon fixed). Ruling: 'npm run tauri dev' fails on ambiguous cargo bin → add [package] default-run = "claude-usage-tracker" in Task 24 (Task 1 plan gap). Review dispatched (sonnet).
Task 23: complete (commits dee42ac..71dd324, review clean; minor deferred: Settings/FailureDetail mount fetch without cancel guard; two unlabeled inputs; pill-backoff/pill-pending lack CSS rules)
Task 24: dispatched (implementer sonnet, BASE 71dd324) with carry-forwards: Cargo default-run, .gitattributes text=auto eol=lf, no --test-threads=1 in docs
Task 24: implementer DONE (a0a79f3; 275 Rust tests, 36 vitest, tauri build MSI+NSIS, default-run fixed, .gitattributes extended, index.html renormalised to LF). Concern: 6 files CRLF in working tree only. Task review (sonnet) + FINAL whole-branch review (opus) dispatched in parallel.
Task 24: complete (commits 71dd324..a0a79f3, review clean). ALL 24 TASKS COMPLETE. Final whole-branch review (opus) in flight over 4374e0c..a0a79f3.
FINAL REVIEW (opus): Ready to merge WITH FIXES. Important: (F1) a failed perform_halt write leaves polling armed — add an in-memory AtomicBool halt latch on Core set before the blocking hop and OR'd into Driver::halted/core_poll_now/core_get_dashboard; (F2) get_settings reads autostart on the async runtime — move inside blocking. Minor triage: all deferred items → defer, except Task 19 autostart-off-hop → must fix (=F2). Rulings judged right: case-insensitive strip, streak survives resets, single-caller invariant (resolved), tray config removal (resolved).
Ruling: fix wave = F1, F2 + five cheap customer-visible minors: (M1) pill CSS: ok not amber, add pill-backoff/pill-pending rules; (M2) cache close_to_tray in an atomic on Core (no sync store read on UI thread); (M3) non-Busy Skip of AccountChanged must not drain/lose ids; (M4) core_poll_now spurious skipped:no_binary before the first refresh_binary — initialise the binary slot in setup; (M5) tray Refresh handler must not do sync SQLite on the main thread — spawn onto async_runtime. Deferred (recorded): quit flat 2.5 s; setup `?` panics; V1 DDL transaction; stderr tail unredacted; the rest per reviewer triage. Costs if wrong: none are quota-affecting except F1, which only adds a fail-closed latch.
FINAL FIX WAVE: dispatched (opus), BASE a0a79f3
FINAL FIX WAVE: implemented (e79e89e, 7 items + 2 one-liners: tray uses latch||stored; autostart write on hop). Ruling: M3 kept clear_halt non-polling; ids preserved to next decision — accepted. Scoped re-review dispatched (opus).
Controller independent gate run on e79e89e: cargo test 254+0+0+10+6+14 = 284 passed, 0 failed; clippy -D warnings clean; npm test 38/38; npm run build ok.
FINAL RE-REVIEW (opus): all 7 + extras ADDRESSED; F1 deep-check (a)-(f) pass; 2 CSS nits deferred (pill-tone-error hardcoded tint; dead .pill-outcome class). FINAL REVIEW CLEAN.
