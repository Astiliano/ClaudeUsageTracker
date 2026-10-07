# Content-hugging, ratio-locked window (Windows first)

Date: 2026-10-06. Branch: `followups` (on top of fb9c1d1, which made zoom window-derived and
removed the Text size setting). Research: `.claude-work/window-hug/reports/research-sizing.md`
(Tauri 2.12.1, tao 0.37.1, wry 0.57.0, windows-sys 0.61.2). Revision r1 applies rulings R1-R13 of
`.claude-work/window-hug/review/RULINGS-window-hug.md`; the mapping is in section 9. Revision r2
applies rulings R14-R23; the mapping is in section 10.

## 1. Goal

Josh (2026-10-06): "It should stick to the content and if it gets stretched the whole thing should
scale no matter what direction it's stretched in. As in if I pull down it should also stretch
right." And: "window height jumps when the drawer opens OR if it's maximized the content inside
scales to fit the extra."

1. The content is laid out at exactly 980 local (unzoomed) CSS px wide in every state. H is the
   height of that layout's border box (content plus the `.app` gutters) in local px. The window's
   client area always has the content's shape, width : height = 980 : H, unless it is capped
   (item 5). Nothing below the content is exposed and no scrollbar shows in a normal window.
2. Dragging any edge or corner keeps that ratio during the drag: no sampled frame is off the ratio
   by more than 1 px (the M10 sampler is the pass criterion).
3. When the content height changes (drawer opens, row added, Settings opens), the window follows in
   one resize, at the current width, and the UI is not rescaled during that resize (no zoom flash).
4. Maximized windows keep the monitor's shape. The zoom is the smaller of the width and height
   terms, so the 980-wide layout is centred and the slack in either axis is `body` background.
   Beyond 2450 x scale physical px of width (980 x ZOOM_MAX) the content stops growing and the
   slack grows instead.
5. Capped: when the content's shape at the current width would not fit the monitor's work area,
   the window first moves up (at most to the work area's top), then its height is capped at the
   work area and its width follows the ratio. When the ratio would make the window narrower than
   the minimum width (735 logical px), the minimum width wins, the ratio yields, the zoom rule
   (`min(w/980, h/C)`) shrinks the content to ZOOM_MIN (0.75), and below that the root scrolls.
6. Windows is the only platform with the live drag lock. On other platforms the same interface
   compiles and does only what Tauri's portable API allows (the app-set size, item 3). The new
   window-platform code lives in `src-tauri/src/platform/windows/` (Josh asked for a `windows`
   directory); a Linux implementation later goes in `src-tauri/src/platform/linux/`.
7. The `narrow` and `cards` layouts are removed. With the layout fixed at 980 local px they are
   unreachable (R7). Flagged to Josh as reversible; the basis is his "the whole thing should scale
   no matter what direction it's stretched".

## 2. Non-goals

- No Linux or macOS drag lock in this change.
- No change to the window-state plugin's persistence format.
- No scrolling layout redesign. Content taller than the work area follows goal item 5: the window
  moves up, caps at the work area with the ratio kept, and the content scales down with the window.
  Only when the minimum width binds does the content shrink to ZOOM_MIN, and below that the root
  scrolls. In that last case, at the minimum width, the classic 17 px vertical scrollbar also
  produces a horizontal one (the 735-px-wide zoomed layout no longer fits beside it). Accepted:
  every pixel stays reachable, and the case needs content taller than the work area / 0.75
  (more than 1376 local px on a 1032 px work area).
- No `scrollIntoView` when a drawer opens. In a fitted window nothing is below the fold, and a
  scroll issued before the async refit would jolt the page on every open. In the ZOOM_MIN scroll
  case of the previous item, a drawer on a low row can open below the fold. Accepted with that
  item.
- No frontend-to-log-file sink. A failed `setContentHeight` logs to `console.warn`. The Rust side
  logs every report it receives and every outcome (section 6), so a release build can still answer
  "why is the shape wrong" from the log file alone.

## 3. Shape of the solution

Rust owns the window. The frontend owns the content height. One number crosses the boundary in
each direction: `content_h` (local CSS px, an integer from `Math.ceil`) goes to Rust whenever the
measured box changes, and a `FitOutcome` comes back.

```
frontend                                     Rust (plugin-managed Arc<AspectState>)
--------                                     --------------------------------------
ResizeObserver on .app (border box,          validate_content_h (finite, [80, 10000])
  zoom-independent), only once the           ratio = 980 / content_h
  dashboard has loaded                       step(): minimized -> SkippedMinimized + pending
fitController (src/lib/fit.ts, no tauri)             maximized -> SkippedMaximized + pending
    --set_content_height(contentH)-->                in size-move -> Deferred + pending
                                                     else plan_fit -> AlreadyFitted | Applied | Capped
    <--FitOutcome or Err---------------      apply failure -> Err + pending again
fitReducer(event) -> zoomContentH            Windows subclass proc:
zoom = clamp(min(w/980, h/zoomContentH))       WM_SIZING        rewrite the drag RECT (fit_rect)
debounced resend while retry                   WM_EXITSIZEMOVE  apply pending through step()
                                               WM_SIZE RESTORED apply pending through step()
                                               WM_DPICHANGED    mark pending; apply unless in a size-move
```

### 3.0 Rejected alternatives

- Null design (no lock; fit the height on release only): rejected. A bottom-edge drag under a
  width-driven release rule snaps back, a max-of-both rule cannot shrink from the bottom edge, and
  knowing the dragged edge needs `WM_SIZING` anyway. Chromium/Electron (`OnSizing` ->
  `SizeWindowToAspectRatio`) and Hopp use the subclass (research Q1).
- Measuring `.app-inner` and dividing `getBoundingClientRect` by the zoom (r0 design): rejected.
  `.app-inner` is a stretched flex item, so its height contains the window height and the window
  can never shrink. The division also races a stale zoom. ResizeObserver box sizes and `offsetHeight`
  are in the element's own unzoomed px (orchestrator probe, RULINGS evidence), so no division is
  needed.
- A layout whose width follows the window (r0 `width: 100%`): rejected. The content height then
  depends on the zoom through `flex-wrap` and `auto-fit` grids, and `min(w/980, h/C)` oscillates
  near a wrap threshold. A fixed 980-px layout makes H independent of the window.
- A Rust "fitted" event to drive the zoom: rejected. The command's return value carries the same
  information without a second channel (R2).
- Moving the existing `cfg(windows)` code in `memory.rs`, `usage/runner.rs`, `login.rs` and
  `discovery.rs` under `platform/windows/`: rejected (R8). Those items include tests inside domain
  test modules and an inline block in a function body, and moving `memory.rs`'s probes would split
  one concept across two owners. `platform/` holds window-platform code only.

### 3.1 Frontend

**Shell CSS (R1).** `src/styles.css:40` becomes
`.app { width: var(--base-w); margin: 0 auto; padding: var(--gutter); display: flex; font-family: var(--ui); }`.
It has no `min-height` and no `height`. `--base-w` is `${BASE_WIDTH}px`, emitted by `shellVars()`
in `src/lib/layout.ts`, so 980 has one TS source. R1 wrote the literal `980px`; the variable
follows R11 and the existing css-contract rule that a shared length is never a literal. With
`* { box-sizing: border-box }` (styles.css:35), the border box is exactly 980 local px wide.
`.app-inner` is unchanged: its flex container `.app` is now content-sized, so stretching it adds
nothing. Slack in either axis is `body`, which already paints `--bg` (styles.css:36).

**The measured box (R1, R10).** H is `.app`'s border-box block size from ResizeObserver
(`entry.borderBoxSize[0].blockSize`). It includes the two gutters, it is in `.app`'s own unzoomed
px (Chrome 154 probe: `zoom: 1.5`, child 200 px, padding 6 gives `borderBoxSize` 212 and
`getBoundingClientRect` 318), and it never depends on the window. No division by the zoom, so no
stale-zoom race. Reports are `Math.ceil(h - 1/64)`, so content <= client. The `- 1/64` drops one
layout unit (Chromium sums in 1/64 px): at zooms that do not divide evenly, a 640-px content can
report 640.004 at one zoom and 640 at another, and a bare ceil would give 641 and 640 and refit the
window by 1 px while dragging across zoom steps (R23, T2-M7). UNSURE: whether WebView2 reports such
fractions at all; M10 S5 measures it.

**`subscribeContentHeight` and `attachContentHeight` (new, `src/lib/contentHeight.ts`).** The file
imports nothing from `backend.ts` or `@tauri-apps/*`, so its test cannot be broken by module loading
(R14). `subscribeContentHeight` is a pure function like `subscribeViewport`, typed by minimal
interfaces so the node-environment vitest can drive it with fakes:
`subscribeContentHeight(element: MeasuredElement, observerCtor: ContentObserverCtor | undefined, report: (h: number) => void): () => void`,
where `MeasuredElement = { offsetHeight: number }` and `ContentObserverCtor` is
`new (cb: (entries: readonly { borderBoxSize: readonly { blockSize: number }[] }[]) => void) => { observe(el: MeasuredElement, opts: { box: "border-box" }): void; disconnect(): void }`.
Contract:
1. With a constructor: it constructs one observer and calls `observe(element, { box: "border-box" })`.
   The first report comes from the observer's initial callback (a real ResizeObserver fires once on
   `observe`); there is no separate mount report.
2. Each callback reads the last entry's `borderBoxSize[0].blockSize`, applies
   `Math.ceil(h - 1/64)`, and calls `report` only when the result is finite, > 0 and different from
   the last value this subscription reported.
3. The returned function calls `disconnect()`. A callback delivered after it reports nothing.
4. `observerCtor` undefined: `console.warn("content height: ResizeObserver unavailable; reporting once")`
   once, then `report(Math.ceil(element.offsetHeight - 1/64))` once, synchronously, under the same
   finite/> 0 rule (`offsetHeight` is also unzoomed, per the probe). The returned function is a no-op.

`attachContentHeight(enabled: boolean, element: MeasuredElement | null, observerCtor: ContentObserverCtor | undefined, report: (h: number) => void): () => void`
is the gate the hook calls (R3, R14): when `enabled` is false or `element` is null it constructs no
observer, reports nothing and returns a no-op; otherwise it returns `subscribeContentHeight(...)`.

**`fitReducer` (new, `src/lib/fit.ts`, R2).** The pure owner of the zoom's content height.
- `FIT_OUTCOMES = ["applied", "alreadyFitted", "capped", "skippedMaximized", "skippedMinimized", "deferred"] as const`
  and `type FitOutcome = (typeof FIT_OUTCOMES)[number]`. The Rust enum serializes to exactly these
  strings (drift test in section 7).
- State `{ cEff: number | null; transit: number | null; lastMeasured: number | null; retry: boolean; viewport: { w: number; h: number } | null }`,
  initially `{ null, null, null, false, null }`. `viewport` is the last viewport event, in CSS px.
- `holds(state, c)`: `viewport !== null && viewport.h * 980 >= (viewport.w - 1) * c`. The window
  as last seen is tall enough for content `c` at the width term, within one width px (R17). It is
  integer arithmetic (R29, T3-M5): `viewport` comes from `innerWidth`/`innerHeight` and `c` from
  `Math.ceil`, all integers, and the products stay far below 2^53, so the boundary row
  (1226, 1125) with C 900 (1102500 >= 1102500) cannot flip on float rounding.
- Events and transitions:
  - `{ kind: "measured", c }`: `lastMeasured = c`.
  - `{ kind: "outcome", c, outcome }` with `c !== lastMeasured`: no change (a newer report is in
    flight and its outcome is the one that counts).
  - `outcome` `applied`: `retry = false`, `transit = c`; then, when `holds(state, c)`, `cEff = c`
    and `transit = null` (settled by geometry, R17). Otherwise the transit waits for a viewport event
    that holds it.
  - `outcome` `capped`: `retry = false`, `cEff = c`, `transit = null`, at once. A height-capped
    window never satisfies `holds` (that is what capped means), so a geometric settle would never
    come. Residual: on a grow into the cap whose reply arrives before the resize event, one frame
    shows the height term of the pre-grow window. Accepted: a capped content is rescaled by the
    zoom rule anyway (goal item 5), and it needs content taller than the work area or a window wider
    than the ZOOM_MAX cap.
  - `outcome` `alreadyFitted`: `cEff = c`, `transit = null`, `retry = false`.
  - `outcome` `skippedMaximized` or `skippedMinimized`: `cEff = c`, `transit = null`, `retry = true`.
  - `outcome` `deferred`: `retry = true`; `cEff` and `transit` unchanged.
  - `{ kind: "failed", c }` with `c === lastMeasured`: `cEff = c`, `transit = null`, `retry = false`.
    A rejected or failed fit (including an apply failure, section 3.2) leaves the window as it is,
    and the `min` zoom then keeps the whole content visible inside it.
  - `{ kind: "viewport", w, h }`: `viewport = { w, h }`; then, when `transit !== null` and
    `holds(state, transit)`, `cEff = transit` and `transit = null`.
- There is no order flag (round 1's `viewportSeen` is dropped, R17). The settle no longer depends on
  which resize event arrives when: with two reports in flight (a drawer opens at C1, then its axis
  row loads and it reports C2), the resize of fit C1 does not hold C2, so the transit for C2 waits
  for fit C2's own resize (A2-M1).
- `zoomContentH(state)`: null while `cEff` and `transit` are both null, that is, until the first
  outcome other than `deferred` (R17; `deferred`'s debounced resend then settles it). Otherwise the
  minimum of the non-null values among `cEff`, `transit` and `lastMeasured`.
  - While a fit is in flight, the smaller of the old and new heights keeps `h / C >= w / 980`
    before and after the resize, in either arrival order of the resize event and the IPC reply. On
    a grow, the old C holds until the window has grown; on a shrink, the new C applies from the
    moment it is measured, before the window shrinks. So the width term wins throughout and the UI
    never rescales (goal item 3).
  - A smaller C can only raise the height term, so it never shrinks a fitted window's content; it
    can overstate the zoom only in a window shorter than its content, and there every outcome
    (`capped`, `skipped*`) or holding resize event settles `cEff` within one IPC round trip.
  - Cold start: before the first outcome the zoom is the width term only (`windowZoom` with null),
    so the restored window is never rescaled by a guess. When the content grew since the last
    session, its bottom overflows the restored window for one IPC round trip, until the first fit
    lands. Accepted (A2-M7's recommended choice; section 5 says the same).
- This refines R2's "`Applied` sets `C_eff`". Without it, the no-flash claim would depend on
  WebView2 delivering the IPC reply after the resize event, which nobody has verified.
- UNSURE: `innerHeight` rounding at fractional scales (125%, 175%) may leave a fitted window a
  fraction of a CSS px short of `holds`. The `w - 1` tolerance absorbs one width px; a transit
  that still does not settle keeps the smaller C in the minimum, which is the no-flash side, and
  the next `alreadyFitted` settles it. M10 H7 runs at 100% and 150%; 125% is not covered.
- `shouldResend(state)`: `retry && lastMeasured !== null`. The dedupe in `subscribeContentHeight`
  is on measurements, so it never suppresses this resend (R2).

**`createFitController` (new, `src/lib/fit.ts`, R14, R18).** The glue between the measurement, the
command and the zoom, as a pure object with injected effects, so vitest tests it without React or
tauri. `fit.ts` imports nothing from `backend.ts` or `@tauri-apps/*`.
- `createFitController({ send, publish, warn }): FitController`, where
  `send: (c: number) => Promise<FitOutcome>`, `publish: (state: FitState) => void` and
  `warn: (message: string, error: unknown) => void`. `FitController` is
  `{ onMeasured(c: number): void; onViewport(v: { width: number; height: number }): void; dispose(): void }`.
- Refined from R14's `dispatch`: the controller is the one owner of `FitState`. It applies
  `fitReducer` itself and hands each new state to `publish` (the hook's `useState` setter). A React
  `useReducer` copy beside it would be a second owner, and the debounced resend must read the state
  at the moment its timer fires, which a render-time copy cannot give.
- `onMeasured(c)`: apply `measured{c}`, publish, then `send(c)`. A resolution applies
  `outcome{c, outcome}` and publishes; a rejection calls `warn("content height: fit failed", e)`
  once, applies `failed{c}` and publishes. It does not toast: a failed fit is cosmetic, not a user
  action.
- `onViewport(v)`: apply `viewport{w: v.width, h: v.height}`, publish; when `shouldResend`, (re)arm
  the resend timer.
- Resend (R18): one `send(lastMeasured)` `RESEND_DEBOUNCE_MS` = 200 ms after the last viewport
  event, never while a call is in flight: a timer that fires during a call re-arms for another
  200 ms, and a timer that fires when `shouldResend` no longer holds sends nothing. The resend does
  not apply `measured` (the value is `lastMeasured` already); its
  resolution applies `outcome{lastMeasured, ...}` like any other. During a deferring drag this is
  one call after the drag ends, not one per mouse move.
- `dispose()`: clears the timer; resolutions that arrive later apply and publish nothing.
- Timers are the global `setTimeout`/`clearTimeout`, so tests use `vi.useFakeTimers()`.

**`useContentHeight(ref, enabled, viewport): number | null` (new, `src/hooks/useContentHeight.ts`).**
Wiring only, with no logic of its own:
- `useState<FitState>(initialFitState)` and `controllerRef = useRef<FitController | null>(null)`.
- The first `useEffect`, on `[]`, creates the controller inside the effect (R27, T3-M3) with
  `send = (c) => backend().setContentHeight(c)`, `publish = setState`, `warn = console.warn`,
  stores it in `controllerRef.current`, and returns a cleanup that calls `dispose()` and sets
  `controllerRef.current = null`. The ref exists only so the effects below can reach the current
  controller. React 19 StrictMode (src/main.tsx:16) mounts, cleans up and remounts every effect in
  dev; because the controller is built in the effect, the remount builds a fresh one instead of
  reusing the disposed one, whose resolutions would publish nothing (so `zoomContentH` would stay
  null for the whole dev session). A controller held in `useRef(createFitController(...))` with
  dispose on unmount is the rejected form.
- `useEffect` on `[enabled]`, declared after it (effects run in declaration order, so the ref is
  set): `attachContentHeight(enabled, ref.current, globalThis.ResizeObserver, (c) => controllerRef.current?.onMeasured(c))`,
  returning its unsubscribe. The callback reads the ref at call time, so after a StrictMode
  remount it reaches the fresh controller; the re-attached observer's first callback measures
  again, so the fresh controller starts from `initialFitState` with a measurement.
- `useEffect` on `[viewport]`: `controllerRef.current?.onViewport(viewport)`. It also re-runs on
  the StrictMode remount, so the fresh controller receives the current viewport.
- It returns `zoomContentH(state)`.
- It has no vitest file: vitest runs in node without a renderer, and every decision it delegates is
  tested in `contentHeight.test.ts` and `fit.test.ts`. `npm run build` typechecks it; M10 S1 and S6
  exercise it.

**App wiring (R3), `src/App.tsx`.**
- The hooks stay above the early return.
- `appRef` attaches only to the loaded branch's `<main className="app">`, and
  `useContentHeight(appRef, dashboard !== null, viewport)` subscribes only once the dashboard is
  loaded. The loading and error branch never reports, and the window keeps its restored or stored
  shape until the first real report.
- `zoom = windowZoom(viewport.width, viewport.height, contentH)`.

**`windowZoom(widthPx, heightPx, contentH: number | null)` replaces `windowZoom(w, h)`.**
- A non-finite or <= 0 width or height gives 1, as today.
- A `contentH` that is null, non-finite or <= 0 gives the width term only, `w / 980`. Before the
  first outcome the window has its restored shape, and a guessed height term would shrink a
  correctly shaped window.
- Otherwise the zoom is `min(w / 980, h / contentH)`. Every result is clamped to
  `[ZOOM_MIN, ZOOM_MAX]` = `[0.75, 2.5]`. Clamping is ordered `Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, x))`
  as today.
- In a fitted window both terms are equal within 1 px. Maximized or capped, the smaller one wins
  (goal items 4 and 5).

**Window config, `src-tauri/tauri.conf.json` (R11).**
- `minWidth` becomes 735 (`BASE_WIDTH x ZOOM_MIN`), and `minHeight` is removed (the ratio derives
  the height).
- `width` 980 stays. `height` 640 stays as the first-run default before any ratio is known.
- `BASE_HEIGHT` is deleted from `layout.ts`, because nothing reads it after `windowZoom` changes.

**`--viewport-h` (R1, arch M6).**
- It survives only for `.modal-body`, whose `max-height` becomes
  `calc(var(--viewport-h) - 2 * var(--gutter))`. That rule is the floor: FailureDetail's `.modal`
  is a fixed, centred overlay (styles.css:316), so a fixed px floor taller than a short hugging
  window would push the body's top and bottom out of the viewport, out of reach. The floor is
  therefore the whole window less the gutters. A 250-local-px window gets a 238 px body instead of
  200.
- Settings is inline content (`section.panel.settings` inside `.app-inner`, Settings.tsx:139), so
  opening it grows the window like a drawer.
- `shellStyle` keeps emitting `--viewport-h`.

**History chart.**
- `CHART_HEIGHT = 220` local px in `layout.ts`, emitted as `--chart-h` by `shellVars()` and read by
  `.chart { height: var(--chart-h) }`.
- Deleted: `src/hooks/useChartHeight.ts` (with its `scrollIntoView`, section 2), `src/lib/chart.ts`
  and `src/lib/chart.test.ts`. Nothing remains in chart.ts once `chartHeightPx` goes.
- `HistoryDrawer` loses its `zoom` and `rowRef` props and the `useChartHeight` call. `AccountRow`
  loses its `zoom` prop and its `rowRef`, which only fed the drawer (AccountRow.tsx:47, 88, 108).
- `AccountsTable` keeps `zoom` for the reorder drag stride and stops passing it to `AccountRow`.
- The `ROW_BORDER` doc comment (layout.ts:17) becomes "the reorder drag stride adds it to
  ROW_HEIGHT" (AccountsTable.tsx:177).

**Backend (R10), `src/lib/backend.ts`.**
- `Backend.setContentHeight(localPx: number): Promise<FitOutcome>`. The real implementation is
  `tauriInvoke<FitOutcome>("set_content_height", { contentH: localPx })`, because tauri maps the
  Rust parameter `content_h` to the camelCase key `contentH`.
- The mock backend (`src/lib/mockBackend.ts`) records each call the way its `setAlwaysOnTop` does
  (mockBackend.ts:514): `console.info("mock: setContentHeight", localPx)`, then it resolves
  `"alreadyFitted"`. The test observes the record through a `console.info` spy (R23, T2-M8).
- `FitOutcome` is imported from `fit.ts` as a type; `fit.ts` imports nothing from `backend.ts`.

**Layouts (R7).**
- `layoutFor`, `Layout`, `BREAKPOINTS`, `NARROW_HIDDEN`, `autoHiddenColumns` and `SHELL_PADDING`
  are removed, together with everything only they reached: `AccountCard`, the `md` Ring and the
  card CSS, Header's `compact`, `countPlacement`'s `compact` parameter, and Settings' `layout` prop
  and width hints.
- The task list is in section 8, task 3. The local width is exactly 980 in every state, so these
  paths are dead.

### 3.2 Rust, shared (`src-tauri/src/window_aspect.rs`)

All geometry is physical px in `i32`, with plain `Rect { left, top, right, bottom }` and
`Size { w, h }` structs, so the module compiles and tests on every platform. No `clamp` (it panics
when min > max), no `unwrap`, no `expect` outside tests: bounds use ordered `min`/`max` steps.

**Constants (R11; one Rust source each, with drift tests in section 7).**
- `BASE_WIDTH_PX: f64 = 980.0` (logical). It equals tauri.conf.json `width` and TS `BASE_WIDTH`.
- `MIN_WIDTH_PX: f64 = 735.0` (logical). It equals tauri.conf.json `minWidth`, which is TS
  `BASE_WIDTH x ZOOM_MIN`.
- `ZOOM_MAX: f64 = 2.5`. It equals TS `ZOOM_MAX`.
- `MIN_CONTENT_H: f64 = 80.0` and `MAX_CONTENT_H: f64 = 10_000.0` (local px, R3).
  - The floor sits below the smallest real dashboard and above a one-line strip. A dashboard with
    no accounts is about 112: gutters 12, topbar about 32, header gap 10, system line about 20,
    inner gap 6, table head about 30, panel borders 2. A one-account dashboard adds a 41-px row.
    A loading strip is about 32.
  - UNSURE: these are estimates from styles.css, not measured. M10 H11 measures the zero- and
    one-account reports and fails if either is below `MIN_CONTENT_H + 20`; that failure returns to
    the orchestrator as a design finding.

**`AspectState` (R4).**
- Fields: `ratio_bits: AtomicU64` (f64 bits; 0 means unknown), `in_size_move: AtomicBool` and
  `pending_fit: AtomicBool`.
- Methods:
  - `ratio() -> Option<f64>`, `set_ratio(f64)`;
  - `begin_size_move()`;
  - `end_size_move() -> bool`, which clears `in_size_move` and returns whether a fit is pending;
  - `in_size_move() -> bool`, `is_pending() -> bool`, `mark_pending()`, `take_pending() -> bool`.
- It is window-independent on purpose. The window is destroyed on close-to-tray and rebuilt, and
  the rebuilt window is fitted from the stored ratio before the frontend reports again.
- The plugin's own `setup` manages it as `Arc<AspectState>` (R13, arch M1), because tauri builds the
  config windows before the app's `setup` closure runs (tauri-2.12.1 app.rs:2690-2697).
- Plugin `setup` runs before the first `on_window_ready`: source-verified in round 2 (tauri-2.12.1
  `Builder::build` calls `initialize_plugins`, app.rs:2607 -> manager/mod.rs:473-478 ->
  plugin.rs:907-916, before `setup()` builds the config windows, app.rs:2691-2693; both lenses
  agree). The hook still reads the state with `try_state` and, when it is missing, logs WARN
  "window aspect state missing" and returns without installing, so the read cannot panic. M10 S1
  checks for the "subclass installed" line at cold start.

**Pure functions (R4, R5), unit-tested without Win32.**
- `validate_content_h(h: f64) -> AppResult<f64>`: `Ok(h)` when `h` is finite and in
  `[MIN_CONTENT_H, MAX_CONTENT_H]`, else `AppError::OutOfRange` naming the value. A rejection
  leaves the stored ratio as it was.
- `decide_fit(maximized: bool, minimized: bool, in_size_move: bool) -> FitAction`, with
  `FitAction::{Fit, Defer, SkipMaximized, SkipMinimized}`. Precedence: minimized, then maximized,
  then in a size-move, else `Fit`.
- `min_client_px(min_logical: f64, dpi: u32) -> i32`: `ceil(min_logical x dpi / 96)`.
- `client_bounds(dpi: u32, work: Option<Rect>, nc: Size) -> ClientBounds { min_w, max_w, min_h, max_h }`:
  - `min_w = min_client_px(MIN_WIDTH_PX, dpi)`.
  - `max_w = min(work.w - nc.w, floor(BASE_WIDTH_PX x ZOOM_MAX x dpi / 96))`, so beyond
    2450 x scale the window stops widening (R13, tests M2).
  - `max_h = work.h - nc.h`. With no work area, `max_w` is the ZOOM_MAX term alone and `max_h` is
    `i32::MAX`.
  - `min_h = min_client_px(MIN_CONTENT_H x MIN_WIDTH_PX / BASE_WIDTH_PX, dpi)` (R3). This is the
    shortest client a validated ratio can produce at the minimum width, so it binds only for a
    ratio outside the validated range. R3 wrote `MIN_CONTENT_H x scale`, but that value is taller
    than a valid 80-px content at 735 wide (60 x scale), so it would break the ratio.
- `fit_client(target_w: f64, ratio: f64, b: &ClientBounds) -> ClientFit { w, h, capped }` is the
  one definition of the capped state (R5), used by `plan_fit` and `fit_rect`:
  ```
  upper  = min(b.max_w, floor(b.max_h x ratio))   width that keeps the ratio at the height cap
  w      = round(target_w); if w > upper { w = upper }; if w < b.min_w { w = b.min_w }   min wins
  want_h = ceil(w / ratio)
  h      = min(max(want_h, b.min_h), b.max_h)      the height cap applies last; the ratio yields
  capped = round(target_w) > upper || want_h > b.max_h
  ```
- `plan_fit(f: &WindowFacts, ratio: f64) -> FitPlan`:
  - `WindowFacts { client: Size, outer: Rect, work: Option<Rect>, dpi: u32, maximized: bool, minimized: bool }`
    and `nc = outer size - client`.
  - `fit = fit_client(client.w, ratio, client_bounds(...))`.
  - The window moves up only by the growth the fit itself adds, and only as far as that growth
    crosses the work area's bottom (R21, A2-M3):
    ```
    grow     = max(0, fit.h - client.h)
    overflow = max(0, outer.top + nc.h + fit.h - work.bottom)     0 with no work area
    top      = max(work.top, outer.top - min(grow, overflow))     at most to the work area's top
    ```
    A window the user parked partly below the work area keeps its position while its size holds,
    and a grow moves it up by the growth at most, never by the part that was already below.
  - `FitPlan::Unchanged` when `fit.w == client.w` and `0 <= client.h - fit.h <= 1`. The top is not
    compared: with the size unchanged `grow` is 0, so the top is unchanged by construction. A client
    1 px taller is tolerated; a shorter one never is.
  - Otherwise `FitPlan::Resize { outer: Rect, client_w, client_h, capped }`, keeping `outer.left`.
  - Accepted and stated: `outer` comes from `GetWindowRect`/`outer_size`, which on Windows 10/11
    include the invisible resize border below the visible frame (about 7 px at 100%). A move-up
    therefore stops with the visible bottom up to that border's height above the work area's
    bottom. No `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` call: it would add a second
    geometry source for a gap of a few px, and the move-up happens only on a grow past the work area.
- `step(state: &AspectState, f: &WindowFacts) -> Step`, with
  `Step::{NoRatio, Skip(FitAction), Unchanged, Apply(FitPlan)}`. It is the one decision path,
  shared by the command, the ready hook and the proc:
  - ratio unknown gives `NoRatio`;
  - otherwise `decide_fit(f.maximized, f.minimized, state.in_size_move())`. Any non-`Fit` action
    calls `mark_pending()` and returns `Skip` (R4: every skip and defer leaves a pending fit);
  - `Fit` calls `take_pending()` before planning, so the `WM_SIZE` that our own apply causes finds
    nothing pending, and then returns `plan_fit`'s answer.
- `Edge::{Left, Right, Top, TopLeft, TopRight, Bottom, BottomLeft, BottomRight}` with
  `Edge::from_wmsz(v: u32) -> Option<Edge>`: the Win32 `WMSZ_*` values 1-8 written as literals, so
  the mapping is portable and unit-tested; any other value gives `None`. The Windows test module
  asserts the literals equal the windows-sys constants (section 7.3).
- `fit_rect(edge: Edge, rect: Rect, nc: Size, ratio: f64, b: &ClientBounds) -> Rect` (section 4).
- `run_guarded<T>(warned: &AtomicBool, body: impl FnOnce() -> T, fallback: impl FnOnce() -> T) -> T`:
  runs `catch_unwind(AssertUnwindSafe(body))`; on a panic it logs WARN
  "window aspect subclass panicked; forwarding" only when `warned.swap(true)` was false, and returns
  `fallback()`. It is portable so it is unit-tested; the proc passes a process-wide static.
  The proc wraps only its own work in it, never the forward to `DefSubclassProc` (R22, section
  3.3), so a panic can never cause a second forward.
- `FitOutcome` (serde `rename_all = "camelCase"`):
  `Applied | AlreadyFitted | Capped | SkippedMaximized | SkippedMinimized | Deferred`.
  `outcome_of(&Step) -> Option<FitOutcome>` maps `Apply` by its `capped` flag, `Unchanged` to
  `AlreadyFitted`, and each `Skip` to its outcome; `NoRatio` has none, because the command always
  sets the ratio first.

**Command.** `#[tauri::command] pub fn set_content_height(window: tauri::Window, state: tauri::State<'_, Arc<AspectState>>, content_h: f64) -> AppResult<FitOutcome>`.
It is sync, so it runs on the main thread.
1. `accept_report(&state, content_h) -> AppResult<f64>` (pure, unit-tested): `validate_content_h`,
   then `set_ratio(BASE_WIDTH_PX / content_h)`. A rejection returns before `set_ratio`, and the
   command logs WARN "content height rejected" {label, content_h}.
2. The ratio is now stored, so `step` below never sees `NoRatio`.
3. `window_facts(&window)` gathers the facts from tauri getters: `is_maximized`, `is_minimized`,
   `inner_size`, `outer_position`, `outer_size`, `scale_factor` (as dpi, `round(scale x 96)`), and
   `current_monitor()` then `Monitor::work_area()`. That is the one owner of the work area outside
   the proc (R13, arch M3). A getter error logs WARN and returns `AppError::Internal`.
4. `step`. On `Apply`, `platform::apply_fit(&window, &plan)`. When the apply fails, the command
   calls `mark_pending()` again (`step` took the flag before planning) and returns the error, so
   the frontend applies `failed` (`cEff = c`; the `min` zoom fits the content into the unchanged
   window) and the next `SIZE_RESTORED`, size-move end, DPI change or report retries the fit (R19,
   A2-M6). It never reports `applied` for a fit that did not land.
5. It returns the outcome and logs DEBUG "content height report" {label, content_h, ratio, outcome},
   with `ratio` written as a fixed 6-decimal string (`format!("{ratio:.6}")`) for human readers
   (R15). The M10 judge does not recompute from the ratio: a 6-decimal ratio cannot reproduce
   `ceil(client_w / ratio)` where `client_w x content_h / 980` is an exact integer, so the judge
   takes the logged integer `content_h` and uses integer arithmetic (7.4, R24). A failed report
   logs the same line with `outcome: "error"` and the error.

Steps 1-4 live in the pure
`handle_report(state, content_h, facts: impl FnOnce() -> AppResult<WindowFacts>, apply: impl FnOnce(&FitPlan) -> AppResult<()>) -> AppResult<(FitOutcome, Option<FitPlan>)>`,
and step 3's conversion in `facts_from_getters(...)` (section 8, task 7), so they are unit-tested
without a tauri runtime, including the apply failure (R19). The command body is the getter calls,
the `apply_fit` closure and the log lines.

**`fit_now(window, state, source)`.** The same steps 3-4 without a report, for the ready hook, with
the same re-mark on an apply failure. `NoRatio` logs DEBUG "fit skipped: ratio unknown".

**`plugin<R: Runtime>() -> TauriPlugin<R>`.**
- `tauri::plugin::Builder::new("window-aspect")`. Its `setup` manages the state; its
  `on_window_ready` runs on the main thread (tauri manager/window.rs:122-130).
- The ready hook calls `platform::install(&window, Arc::clone(&state))`, then
  `fit_now(.., "ready")`.
- It is registered in `src-tauri/src/lib.rs` immediately after `tauri_plugin_window_state`
  (lib.rs:208-212), so its hook runs after the window-state restore. That restore's `set_size` is
  applied synchronously, because `SWP_ASYNCWINDOWPOS` posts only across input queues and both
  hooks run on the main thread (R6). So `inner_size()` reads the restored width.
- `set_content_height` joins `generate_handler!` in lib.rs. An app command needs no capability
  entry.
- An install failure logs WARN with the label and error code. The app keeps running with a
  free-form window, and the `min` zoom keeps the content inside it.

### 3.3 Rust, Windows (`src-tauri/src/platform/windows/aspect.rs`)

**Install.**
- `install_hwnd(hwnd: HWND, state: Arc<AspectState>, label: String) -> Result<(), u32>` is
  `install_hwnd_with(hwnd, state, label, ProcHooks::REAL)`.
- `install_hwnd_with(hwnd, state, label, hooks: ProcHooks) -> Result<(), u32>`:
  - `Box::into_raw(Box::new(SubclassData { state, label, hooks }))`, then
    `SetWindowSubclass(hwnd, Some(proc), SUBCLASS_ID, ptr as usize)` with
    `SUBCLASS_ID = 0x4355_5441`;
  - on failure it frees the box with `Box::from_raw` and returns `GetLastError()`.
- `ProcHooks { apply: fn(HWND, &FitPlan, &str) -> bool, after_forward: fn(u32) }` is the proc's
  test seam. `ProcHooks::REAL` is `{ apply: apply_hwnd, after_forward: no-op }`; production code
  never builds another value. The 7.3 tests install a failing `apply` (R19) and a panicking
  `after_forward` (R22), so both failure paths run against a real HWND without a `cfg(test)` field
  in the proc.
- `install(&Window, Arc<AspectState>) -> bool` resolves `window.hwnd()` and calls `install_hwnd`.
  On `Ok` it logs INFO "subclass installed" {label, dpi} with `dpi = GetDpiForWindow(hwnd)`
  (section 6; M10 S5 takes its scale from this line, R28); on `Err` it logs WARN
  "subclass install failed" {error}.
- Identity is the (proc, id) pair. wry and tauri-runtime-wry subclass the same HWND with their own
  ids and coexist (research Q1.3).
- It must run on the window's thread, which `on_window_ready` guarantees.

**Facts and apply, shared by the proc and `platform::apply_fit`.**
- `hwnd_facts(hwnd) -> Option<WindowFacts>` reads `GetWindowRect`, `GetClientRect`,
  `MonitorFromWindow(MONITOR_DEFAULTTONEAREST)` + `GetMonitorInfoW` `rcWork`, `GetDpiForWindow`,
  `IsZoomed` and `IsIconic`.
  - The proc uses Win32 here, never tauri getters, because those may re-enter tao's state lock from
    inside the proc. So inside the proc only, `MonitorFromWindow` is the work-area source (R13).
  - Any failed call gives `None`, and the caller logs WARN and forwards.
- `apply_hwnd(hwnd, &FitPlan, label) -> bool` makes one
  `SetWindowPos(hwnd, null, left, top, w, h, SWP_NOZORDER | SWP_NOACTIVATE)`, adding `SWP_NOMOVE`
  when the top is unchanged. Position and size travel in one call (R5). There is no
  `SWP_ASYNCWINDOWPOS`, because every caller is on the main thread.
  - A failure logs WARN "SetWindowPos failed" {label, error} (R13, arch M7) and returns false.
  - `platform::windows::apply_fit(&Window, &FitPlan) -> AppResult<()>` resolves the HWND, calls
    `apply_hwnd`, and maps false to `AppError::Internal("SetWindowPos failed")`, so the command and
    the proc share one apply and the command can re-mark pending (R19). Tauri `set_size` is never called from inside the
    proc.

**The proc.**
- Panic containment (R5, R22): `run_guarded(&PANIC_WARNED, work, fallback)` (section 3.2) wraps only
  the proc's own work, never the forward. `PANIC_WARNED` is a `static AtomicBool`, so the WARN
  appears once per process. Three shapes, one per message kind:
  - Forward-first messages (`WM_EXITSIZEMOVE`, `WM_SIZE`, `WM_DPICHANGED`):
    `let r = DefSubclassProc(..); run_guarded(&PANIC_WARNED, || post_work(), || ()); r`.
  - Work-first messages (`WM_ENTERSIZEMOVE`): `run_guarded(&PANIC_WARNED, || pre_work(), || ())`,
    then `DefSubclassProc(..)`.
  - `WM_SIZING`: `run_guarded(&PANIC_WARNED, || rewrite(), || false)`; when it returns true the proc
    returns TRUE, otherwise it forwards. A panic therefore forwards exactly once.
  - `WM_NCDESTROY` runs no guarded work between the removal and the forward (below).
  - Nothing panics out of an `extern "system"` frame (since Rust 1.81 that aborts the process), and
    no message is forwarded twice: a panic after the forward returns the forward's result.
- The post-forward work of every message starts with `(hooks.after_forward)(msg)` (a no-op in
  production) and applies through `(hooks.apply)(hwnd, &plan, &label)`.
- Apply failure (R19): whenever the proc's apply returns false, it calls `mark_pending()` again
  (`step` took the flag), so the next `SIZE_RESTORED`, size-move end, DPI change or report retries.
  The size-move line then says `pending: apply_failed`.
- Handled messages all forward to `DefSubclassProc`, except `WM_SIZING`, which returns TRUE without
  forwarding (R13, arch M2):
  - `WM_ENTERSIZEMOVE`: `begin_size_move()`, then forward.
  - `WM_SIZING`:
    - Rewrite when `IsZoomed == 0`, the ratio is known, `Edge::from_wmsz(wparam as u32)` maps one of
      the 8 `WMSZ_*` values, and `hwnd_facts` succeeds.
    - The rewrite is `*rect = fit_rect(edge, *rect, nc, ratio, &client_bounds(dpi, Some(work), nc))`,
      and the proc returns TRUE.
    - In every other case it forwards. Rewrites are not logged: there is one per mouse move.
  - `WM_EXITSIZEMOVE`: forward first, so tao clears `MARKER_IN_SIZE_MOVE` (tao event_loop.rs:983)
    before our resize reaches it (R4, arch I5).
    - Then `pending = end_size_move()`. When pending, `step(state, hwnd_facts)`: `Apply` calls
      the apply, and `Skip` keeps the fit pending (zoomed or iconic after a snap-to-maximize).
    - It logs one INFO "size-move end" {label, client_w, client_h, dpi, ratio_err_px,
      pending: none | applied | unchanged | apply_failed | skipped:<reason>}, with `dpi` from
      `GetDpiForWindow` (M10 S5 reads it, R28), where
      `ratio_err_px = client_h - ceil(client_w / ratio)` measured after the apply, a signed integer
      that is 0 or 1 in a fitted window (the same judge as M10, R23; R13, arch M7).
  - `WM_SIZE` with `wparam == SIZE_RESTORED`: forward first. Then, when `!in_size_move()` and
    `is_pending()`, run `step` and apply as above, and log INFO "window fitted" {source: "restored"}.
    - This is the resume path after maximize or minimize (R4).
    - During a drag every `WM_SIZE` is `SIZE_RESTORED`, but the `in_size_move` check skips them.
    - The `WM_SIZE` caused by our own apply finds nothing pending, because `step` took it.
  - `WM_DPICHANGED` (R20, A2-M2): forward first, so tao applies its logical-size-preserving resize
    (tao event_loop.rs:1916-1923) before we read the geometry. Then `mark_pending()`, and when
    `!in_size_move()`, run `step` and apply at once and log INFO "window fitted" {source: "dpi"}.
    Inside a size-move (a drag across monitors) the fit stays pending and `WM_EXITSIZEMOVE` applies
    it. `client_bounds` then uses the new dpi and the new monitor's work area, so a window wider than
    the new cap is brought inside it.
  - `WM_NCDESTROY`: `RemoveWindowSubclass(hwnd, Some(proc), SUBCLASS_ID)`, forward, then
    `drop(Box::from_raw(data))`. The rebuilt window gets a fresh install from the hook.
  - Everything else: forward.
- Minimum and maximum: `client_bounds` with `MIN_WIDTH_PX` and the dpi from `GetDpiForWindow`
  (through the pure `min_client_px`), and the monitor's `rcWork`. tao's own `WM_GETMINMAXINFO` floor
  (tauri.conf.json `minWidth` 735) stays as the hard floor. It equals `min_w` within 1 px: tao
  rounds the logical-to-physical conversion and `min_client_px` takes the ceiling, so ours is never
  below tao's and the `WM_SIZING` floor binds first (arch r2, checked and fine).

**Cargo.** In `src-tauri/Cargo.toml`, `[target.'cfg(windows)'.dependencies] windows-sys` 0.61
gains `Win32_UI_Shell` (subclass), `Win32_UI_WindowsAndMessaging`, `Win32_UI_HiDpi` and
`Win32_Graphics_Gdi` (monitor info). The windows dev-dependency gains
`Win32_System_LibraryLoader` (`GetModuleHandleW` for the test window class). No new crate.

### 3.4 Platform module layout

```
src-tauri/src/platform/mod.rs        #[cfg(windows)] pub use windows::*; #[cfg(not(windows))] pub use other::*;
src-tauri/src/platform/windows/mod.rs
src-tauri/src/platform/windows/aspect.rs
src-tauri/src/platform/other.rs      install() -> false with one INFO "live aspect lock unsupported on this platform";
                                     apply_fit() -> set_position (only when the top changes) then set_size
src-tauri/src/window_aspect.rs       shared: constants, AspectState, pure functions, command, plugin, fit_now
```

The interface the rest of the crate sees (R8, R13):
- `platform::install(&Window, Arc<AspectState>) -> bool`;
- `platform::apply_fit(&Window, &FitPlan) -> AppResult<()>`.

There is no `platform::work_area_height`: outside the proc the work area comes from
`Monitor::work_area()`. `lib.rs` declares `pub mod platform;` and `pub mod window_aspect;`, next to
the existing `pub mod` list (lib.rs:1-15). Both are `pub` like their siblings, so items that land a
task before their caller raise no dead-code lint.

The existing `cfg(windows)` items in `memory.rs`, `usage/runner.rs`, `login.rs` and `discovery.rs`
stay where they are (R8; section 3.0).

## 4. `fit_rect` contract

Inputs are physical px. `ratio = 980 / content_h` (client width over client height). `b` comes
from `client_bounds` (section 3.2), so a drag and an app-set fit agree on every bound.

```
cw = rect.w - nc.w ; ch = rect.h - nc.h                       proposed client size
target_w = LEFT|RIGHT -> cw ; TOP|BOTTOM -> ch x ratio ; corners -> max(cw, ch x ratio)
fit = fit_client(target_w, ratio, b)                          min width wins, then the height cap
W' = fit.w + nc.w ; H' = fit.h + nc.h
left-moving edges (LEFT, TOPLEFT, BOTTOMLEFT): rect.left = rect.right - W'  else rect.right = rect.left + W'
top-moving edges  (TOP, TOPLEFT, TOPRIGHT):    rect.top  = rect.bottom - H' else rect.bottom = rect.top + H'
```

- Anchoring:
  - Dragged edges move, and the opposite edges keep their coordinate.
  - Pure LEFT/RIGHT keeps the top fixed, and pure TOP/BOTTOM keeps the left fixed (Chromium's and
    Hopp's rule).
  - Corners use "cover", so the cursor corner stays inside the frame.
- `WM_SIZING` never moves the window up. Only an app-set fit (`plan_fit`) does.
- Worked case (research): ratio 980/640, nc (16, 39), RIGHT, outer 1016x700, 100% DPI, work area
  1920x1032.
  - The proposed client is 1000x661.
  - The fit is w 1000 and h `ceil(653.06)` = 654, so the outer height is 693 and the right edge is
    unchanged.
  - With `ceil` the client is never shorter than the content (R1, arch M5). The r0 text rounded to
    653/692.

Table rows (section 7) cover all 8 edges, the min-width bound, the height cap with the ratio kept
on all 8 edges (the top-moving edges set `rect.top = rect.bottom - H'` with a capped `H'`), crossed
bounds (`min_w > floor(max_h x ratio)`) on all 8 edges, the ZOOM_MAX width cap, and
`0 <= fit.h - ceil(fit.w / ratio) <= 1` when the ratio is kept.

## 5. Lifecycle and edge cases

- **Cold start.** Window-state restores the last inner size (both dimensions, physical). The ratio
  is unknown (it lives in memory only), so the ready hook does not fit (DEBUG "fit skipped: ratio
  unknown").
  - The loading and error branch never reports (R3). Until the first outcome that sets `cEff` or
    `transit`, `zoomContentH` is null and the zoom is the width term only (R17), so the restored
    window shows the layout at its restored width and is never rescaled by a guess. When the
    content grew since the last session, its bottom overflows the restored window for one IPC
    round trip, until the fit lands; there is one height change and no rescale.
  - The first report after the dashboard loads fits the window: one visible height change when the
    content differs from the last session. That happens when the account count changed, and also
    when the last session ended with a drawer or Settings open, because open state is React state
    and is not persisted. Accepted (research risk 3); logged at INFO "window fitted"
    {source: "report"}.
- **Close to tray and reopen.** The HWND is destroyed, and the subclass removes itself and frees its
  data on `WM_NCDESTROY`.
  - The rebuilt window runs window-state's restore, then our hook, both on the main thread. The
    restore is synchronous (R6), so `fit_now(.., "ready")` reads the restored width and fits the
    stored ratio at that width before the frontend loads.
  - The loading render does not report, so the shape holds until the dashboard reports the same H,
    which gives `alreadyFitted`.
  - M10 checks the width survives: 1300 +-2.
- **Content changes mid-drag (size or move loop).** The command stores the ratio, `step` returns
  `Skip(Defer)` with the fit pending, and the frontend gets `deferred`, keeps its C_eff and sets
  retry.
  - A size drag uses the new ratio from its next `WM_SIZING`.
  - At `WM_EXITSIZEMOVE`, after forwarding, the proc runs `step`. A window Windows has just
    maximized (drag to the top edge) is skipped and stays pending; any other window is fitted.
  - The frontend's resend is debounced (R18): one `set_content_height` 200 ms after the last
    resize event, never while a call is in flight. So the rest of the drag sends nothing, and the
    single resend after release answers `alreadyFitted` and settles C_eff.
  - No harness trigger can change the content while a drag is held (R16, section 7.4), so this
    path's end-to-end ordering is a listed hypothesis (section 8) resting on
    `exit_size_move_applies_pending_synchronously` (7.3) and the tao source reading.
- **Maximized or minimized.**
  - The command never resizes either. `set_size` would un-maximize (tao window.rs:264-270), and
    `step` returns `SkippedMaximized` or `SkippedMinimized` with the fit pending.
  - The frontend sets C_eff to the report, so the maximized zoom fits the new content, and sets
    retry.
  - On restore the proc sees `WM_SIZE`/`SIZE_RESTORED` and applies the pending fit synchronously,
    before the webview's resize. The frontend's retry then gets `alreadyFitted`.
  - Restoring from minimized to maximized sends `SIZE_MAXIMIZED`, so the fit stays pending until
    the next restore.
  - `WM_SIZING` is not sent for maximize or snap.
- **Snapped.** For tao a snapped window is not maximized, so a fit (from a report or a deferred
  apply) leaves the snap. Accepted: the user asked for the window to follow the content.
  - UNSURE: the order of snap or maximize relative to `WM_EXITSIZEMOVE` has not been observed. M10
    human check H5 records it.
- **Capped** (goal item 5): `plan_fit` moves the window up by the growth that crosses the work
  area's bottom (R21) and caps it, and the outcome is `capped`. The frontend settles `cEff` at once
  (section 3.1), because a height-capped window never holds the content at the width term.
- **Apply failure.** A failed `SetWindowPos` (WARN "SetWindowPos failed") re-marks the fit pending.
  The command returns the error and the frontend applies `failed`; the proc logs
  `pending: apply_failed`. The next restore, size-move end, DPI change or report retries (R19).
- **DPI change across monitors or of the display scale.** tao preserves the logical inner size on
  `WM_DPICHANGED` (event_loop.rs:1916-1923), which keeps the client ratio only up to rounding and
  can leave a window wider than the new monitor's work area or `980 x ZOOM_MAX x scale`. The
  frontend sees the same CSS-px viewport, so no report follows. The proc therefore marks the fit
  pending on `WM_DPICHANGED`, after forwarding: outside a size-move (a scale change in Windows
  Settings) it fits at once; during a drag across monitors `WM_EXITSIZEMOVE` fits it at release,
  with `client_bounds` at the new dpi and work area (R20).
- **Synchronous resizes.** Every resize we issue is a same-thread `SetWindowPos` without
  `SWP_ASYNCWINDOWPOS`, or a tauri setter on the main thread. So `step` always reads the current
  geometry, and a target is always computed from the current client width.
- **Platforms other than Windows.** Install logs once and returns false. The command still fits on a
  content change through `set_position`/`set_size`.
  - With no proc, the resume path after maximize, minimize or a drag is the frontend's debounced
    retry after the next resize event, and after an apply failure the next report.
  - The user can drag the window out of shape, and the `min` zoom keeps the content inside it.

## 6. Logging

All lines are structured `tracing` fields tagged with `label`, written to the existing log file
(logging.rs). DEBUG is switched on at runtime through Settings' log level.
- INFO:
  - "subclass installed" {dpi}, with `dpi` from `GetDpiForWindow` at install (M10 S5's scale, R28);
  - "live aspect lock unsupported on this platform" (other platforms, once);
  - "window fitted" {source: report | ready | restored | dpi, client_w, client_h, top, capped, ratio};
  - "fit skipped" {reason: maximized | minimized, pending: true};
  - "size-move end" {client_w, client_h, dpi, ratio_err_px, pending: none | applied | unchanged |
    apply_failed | skipped:<reason>}, with `ratio_err_px = client_h - ceil(client_w / ratio)`
    (signed; 0 or 1 in a fitted window). There is one per drag or move, which is the number the
    hypotheses in section 8 need.
  - Every `ratio` field is a fixed 6-decimal string (R15).
- WARN:
  - "subclass install failed" {error};
  - "content height rejected" {content_h};
  - "window facts unavailable" {error};
  - "SetWindowPos failed" {error};
  - "window aspect state missing";
  - "window aspect subclass panicked; forwarding" (once per process).
- DEBUG:
  - "content height report" {content_h, ratio, outcome}, `outcome: "error"` with the error when the
    command fails. It is the anchor of M10 S1 and S2 (R15): it is written on every report, fitted
    or not, while "window fitted" is written only when a fit is applied;
  - "fit deferred" (size-move in progress);
  - "fit skipped: ratio unknown".
- Not logged: `WM_SIZING` rewrites, one per mouse move.
- Write volume: content changes come from user actions and poll cycles, plus one line per drag.
  The frontend's resend is debounced (R18), so a content change during a drag adds one report after
  release, not one per resize event. Nothing logs per frame.
- How to read: `rg '"window fitted"|"size-move end"|"fit skipped"' <log_dir>`. The log directory
  opens through the existing `open_log_dir` command.

## 7. Tests

Every behaviour has a named test in a layer that can observe it, or a numeric yes/no M10 check
(RULINGS bar). Vitest runs in node with no jsdom (`src/**/*.test.ts`); fakes follow
`useViewport.test.ts`.

### 7.1 Vitest

- `src/lib/css-contract.test.ts`:
  - `.app is content-sized and fixed-width`: `.app` has no `height` and no `min-height`, its
    `width` is `var(--base-w)` and its `margin` is `0 auto`. This replaces the `min-height`
    assertion at line 38.
  - `.chart height reads --chart-h`: `decl(rule(".chart"), "height") === "var(--chart-h)"`. This
    inverts lines 81-82.
  - `.modal-body max-height is the window less the gutters`:
    `calc(var(--viewport-h) - 2 * var(--gutter))`. This replaces line 41.
  - `ASSERTED_VARS` (line 21) gains `--base-w` and `--chart-h` (task 1) and loses `--ring-min` and
    `--ring-max` (task 3).
  - `no card-layout rules remain` (task 3): styles.css matches none of
    `/^\.(cards|card|card-rings|ring|ring-label|ring-value)(?![\w-])/m` (R25, T3-M1). The
    lookahead, not `\b`, is what lets `.ring-sm` (the SystemLine glyph, which stays) through: `\b`
    matches between `g` and `-`. In the same task the rule `.ring svg, .ring-sm svg { ... }`
    (styles.css:205) becomes `.ring-sm svg { ... }` with the same declarations, so no surviving
    line starts with `.ring` followed by a space or comma.
  - Task 3 deletes or rewrites these existing tests, which read rules task 3 deletes (R26,
    T3-M2): `.card has no border-radius (flat look)`, `.ring width clamps between the ring
    variables`, `.card-rings is an inline-size container` and `.ring-label may use the full ring
    width` are deleted; `.ring svg fills its wrapper` is rewritten as `.ring-sm svg fills its
    wrapper`, selecting the rule by its new full text `.ring-sm svg`. `.ring-sm is a fixed 20px
    glyph` stays unchanged.
  - The existing "no viewport units" test stays.
- `src/lib/layout.test.ts`:
  - `shellVars` `toEqual` gains `"--base-w": "980px"` and `"--chart-h": "220px"` (task 1) and loses
    the ring variables (task 3).
  - `windowZoom`:
    - `fitted window: both terms agree`: (1470, 960, 640) gives 1.5 (960 / 640 = 1470 / 980);
    - `taller than needed: width term wins`: (1470, 980, 640) gives 1.5;
    - `maximized: height term wins`: (1920, 1032, 900) gives 1032/900;
    - `null contentH: width term only`: (1470, 300, null) gives 1.5;
    - `bad contentH falls back to the width term`: each of 0, -1, NaN and Infinity gives 1.5 at
      (1470, 300);
    - `bad width or height gives 1`: the existing cases, now with a third argument;
    - `clamps`: (500, 2000, 640) gives 0.75, and (4000, 4000, 640) gives 2.5.
  - `the window config matches the base canvas` (R11): `conf.width === BASE_WIDTH`,
    `conf.minWidth === BASE_WIDTH * ZOOM_MIN` and `conf.minHeight === undefined`. This replaces
    the `BASE_HEIGHT` assertion.
  - Task 3 deletes the `layoutFor`, `autoHiddenColumns`, `BREAKPOINTS` and `SHELL_PADDING` describes.
- `src/lib/contentHeight.test.ts` (new; the file under test imports no backend, R14):
  `subscribeContentHeight` with a `FakeObserver` that captures its callback and records `observe`
  and `disconnect`.
  - `first report comes from the observer's initial callback`: `observe` gets
    `{ box: "border-box" }`, nothing is reported before the callback, and a 212 callback gives
    `[212]`.
  - `reports the ceiling of the border box less one layout unit`: 200.2 gives 201, 640 gives 640,
    and 640.004 gives 640 (R23, T2-M7).
  - `dedupes against the last value reported`: 212, 211.6, 212 give one report; 300 gives a second.
  - `ignores non-finite and non-positive sizes`: 0 and NaN give none.
  - `reads borderBoxSize`: the fake entry has no `contentRect`.
  - `unsubscribe disconnects and later callbacks report nothing`.
  - `no ResizeObserver: warns once and reports offsetHeight once`: `offsetHeight` 212.4 gives
    `[213]`, and the `console.warn` spy is called once.
  - `attachContentHeight: disabled or detached constructs no observer`: with `enabled` false, and
    with a null element, the `FakeObserver` constructor count stays 0, nothing is reported, and the
    returned function runs without error (R3: no report while loading).
  - `attachContentHeight: enabled subscribes`: one observer, and its callback reports.
- `src/lib/fit.test.ts` (new):
  - `FIT_OUTCOMES lists the six outcomes`.
  - `fitReducer` table: one row per transition in section 3.1, including:
    - `a stale outcome is ignored`;
    - `failed sets cEff`;
    - `applied settles at once when the stored viewport holds it` (a shrink: viewport (1225, 1125),
      C 900 to 640);
    - `applied waits in transit until a viewport that holds it` (a grow: viewport (1225, 800), C 640
      to 900; a viewport (1225, 800) event keeps the transit, a (1225, 1125) event settles it);
    - `holds tolerates one width px` (viewport (1226, 1125) holds 900) and
      `holds rejects one height px short` (viewport (1226, 1124) does not hold 900) (R29);
    - `capped settles at once` (viewport (1225, 800), C 900: `cEff` 900, `transit` null);
    - `two reports in flight: the first fit's resize does not settle the second` (A2-M1, R17):
      from a settled `cEff` 640 at viewport (1225, 800), the sequence is measured(653),
      measured(666), outcome `applied{653}` (stale, ignored), viewport (1225, 816.25) (fit 653's
      resize), outcome `applied{666}` (`transit` 666 waits, `cEff` stays 640), viewport
      (1225, 832.5) (settles: `cEff` 666, `transit` null). At every step
      `windowZoom(w, h, zoomContentH(state))` is 1.25. Round 1's order flag fails this row at the
      `applied{666}` step (zoom 816.25 / 666 = 1.226).
  - `zoomContentH`:
    - null for the initial state;
    - `null after measured with no outcome` (cold start, R17, A2-M7, T2-M5);
    - `null after a first deferred` (both `cEff` and `transit` still null);
    - the minimum of the non-null heights once `cEff` or `transit` is set.
  - `shouldResend`: true after `deferred` and after each `skipped*`, false after `alreadyFitted`,
    `applied`, `capped` and `failed`.
  - `no zoom flash on grow or shrink, in either arrival order`: for a fitted window at zoom 1.25
    (w = 1225) with a first outcome already settled, C goes 640 to 900 and 900 to 640. With the
    outcome before the resize event and with the resize event before the outcome,
    `windowZoom(w, h, zoomContentH(state))` is 1.25 after `measured`, after the first event and
    after the second, with h the old `C x 1.25` until the resize event and the new one after it.
    The settled state has `cEff` equal to the new C and `transit` null.
  - `createFitController` (R14, R18), with a fake `send` that returns promises the test resolves or
    rejects, `publish` and `warn` spies, and `vi.useFakeTimers()`:
    - `onMeasured publishes measured, sends c, then publishes the outcome for c`;
    - `a rejection warns once and publishes failed for c` (`warn` called with
      `"content height: fit failed"`, `cEff` = c);
    - `a stale resolution is ignored`: `onMeasured(640)`, `onMeasured(700)`, then 640's `applied`
      resolves; the published state's `transit` and `cEff` are untouched by it;
    - `a viewport event with retry resends lastMeasured after 200 ms without a measured event`:
      after `deferred`, one `onViewport`; at 199 ms `send` has 1 call, at 200 ms 2 calls with
      `lastMeasured`, and `lastMeasured` was not re-applied;
    - `N viewport events while deferred produce one resend` (R18): 20 `onViewport` calls 10 ms
      apart, then 200 ms; `send` gains exactly one call;
    - `no resend while a call is in flight`: the timer fires while the first send is unresolved;
      `send` gains no call; after it resolves `deferred` and another 200 ms, one resend;
    - `no resend without retry`: after `alreadyFitted`, viewport events and 1 s give no call;
    - `a timer that fires after retry cleared sends nothing`;
    - `dispose cancels the timer and drops later resolutions`: no `send` and no `publish` after
      `dispose()`.
- `src/lib/backend.test.ts` (new, R10): `vi.mock` of `@tauri-apps/api/core`, `@tauri-apps/api/event`
  and `@tauri-apps/api/window`.
  - `setContentHeight invokes set_content_height with the camelCase key`:
    `invoke("set_content_height", { contentH: 640 })`, resolving the mocked `"applied"`.
- `src/lib/mockBackend.test.ts`: `setContentHeight records calls and resolves alreadyFitted`: with
  `vi.spyOn(console, "info")`, `setContentHeight(640)` resolves `"alreadyFitted"` and the spy saw
  `("mock: setContentHeight", 640)` (R23, T2-M8).
- `src/lib/present.test.ts` (task 3): the `countPlacement` cases lose the `compact` argument. The
  three `compact === true` rows are deleted.
- `src/lib/gauge.test.ts` (task 3, R26, T3-M2): `ringDash` stays in use by Ring, so its describe
  is re-pointed, not deleted: `c = 2 * Math.PI * RING_SIZES.sm.radius`, and its six rows (null,
  0, 50, 100, 140 clamped to 100, -5 clamped to 0) pass `RING_SIZES.sm.radius` instead of
  `RING.radius`. Deleted: the `ring geometry fits the 44px box with a 5px stroke` case (it tests
  `RING`), the `RING_SIZES.md` line of `keeps the md geometry and adds a 20px variant` (the case is
  renamed `sm is the 20px variant` and keeps its `sm` line), and the `ringViewBox(RING_SIZES.md)`
  line. `dashes the small radius at 0, 50 and 100 percent` and the `sm` `ringViewBox` line stay.

### 7.2 Rust unit (`src-tauri/src/window_aspect.rs` `mod tests`)

- Validation and state:
  - `validate_content_h_accepts_the_range`: 80, 640, 10000.
  - `validate_content_h_rejects`: 79.9, 10000.1, 0, -1, NaN, +inf, -inf and 1e-300.
  - `default_ratio_is_unknown`.
  - `rejected_report_keeps_old_ratio`, through `accept_report(state, h)`, the validate-and-store
    helper the command calls.
  - `end_size_move_reports_pending_and_step_takes_it_once`.
- `decide_fit_table`: all 8 combinations, minimized > maximized > size-move.
- `step_table`:
  - no ratio gives `NoRatio`;
  - minimized, maximized and in a size-move each give `Skip`, with `is_pending()` true;
  - normal and within 1 px gives `Unchanged`, with the pending flag cleared;
  - normal and off-shape gives `Apply`, with the pending flag cleared.
- `min_client_px_table`: (735, 96) gives 735; (735, 144) gives 1103; (735, 120) gives 919.
- `client_bounds_table`:
  - work 1920x1032, nc (16, 39), dpi 96 gives min_w 735, max_w 1904, max_h 993 and min_h 60;
  - work 3840x2112 at dpi 96 gives max_w 2450 (R13);
  - no work area gives max_h `i32::MAX`.
- `fit_client_table`:
  - inside the bounds;
  - above `upper`, which gives `upper` and `capped`;
  - height-capped with the ratio kept;
  - crossed (`min_w > floor(max_h x ratio)`), which gives w = min_w, h = max_h and `capped`;
  - a NaN target, which gives min_w;
  - an out-of-range ratio (100, far below the validated range's shortest content) at w = min_w,
    which gives h = `min_h` (want_h 8 < min_h 60 at dpi 96), so the `min_h` branch is exercised
    (R23, T2-M3).
- `plan_fit_table` (work 1920x1032 at (0, 0), nc (16, 39), dpi 96, ratio 980/640 unless stated):
  - within 1 px gives `Unchanged`;
  - a client 1 px short gives `Resize` (never shorter);
  - a grow with room keeps the top;
  - a grow past the work bottom from a window fully inside it moves up by the overflow (R5, tests
    M5): outer.top 400, client 1000x400 grows to 1000x654, so the outer bottom 400 + 39 + 654 = 1093
    crosses 1032 by 61, grow is 254, and top becomes 339;
  - `a same-size window below the work area is Unchanged` (R21, A2-M3): outer.top 900 with a fitted
    client, outer bottom past 1032; no move;
  - `a grow from an already-overflowing position moves up by the growth only` (R21): outer.top 500
    with a fitted 1000x654 client (bottom 1193, 161 already below) and ratio 980/660, so fit.h is
    `ceil(673.47)` = 674; grow 20, overflow 181, top becomes 480;
  - an overflow beyond the room gives top = work.top, the cap and `capped`;
  - a shrink;
  - a width out of bounds after a DPI change is resized into bounds.
- `fit_rect`:
  - `fit_rect_worked_case`: section 4;
  - `fit_rect_edges_anchor`: 8 rows, each asserting the moving and fixed coordinates;
  - `fit_rect_min_width`: 8 rows;
  - `fit_rect_height_cap`: 8 rows with the height cap binding and the bounds not crossed, each
    giving client h = max_h and w = floor(max_h x ratio) (the ratio kept within 1 px), the opposite
    edges fixed, and for TOP, TOPLEFT and TOPRIGHT `rect.top = rect.bottom - H'` (R23, T2-M3);
  - `fit_rect_crossed_bounds`: 8 rows, none panicking, each giving w = min_w and h = max_h;
  - `fit_rect_zoom_max_cap`;
  - `fit_rect_keeps_ratio_within_1px`: every edge x client widths 735..=2400 step 15 x three
    ratios.
- `edge_from_wmsz_table`: 1-8 map to the 8 edges (`WMSZ_LEFT` 1 through `WMSZ_BOTTOMRIGHT` 8);
  0 and 9 give `None`.
- `run_guarded_returns_fallback_on_panic_and_warns_once`: with a test-local `AtomicBool`, two
  panicking bodies give the fallback twice and exactly one WARN line, captured with
  `crate::test_log::captured`.
- `run_guarded_returns_the_body_value_without_a_panic`: the fallback is not called and nothing is
  logged.
- `fit_outcome_serializes_camel_case`: each variant's `serde_json` string.
- Command glue (task 7), through `facts_from_getters` and `handle_report`, because the crate has no
  `tauri::test` runtime: `facts_from_getters_rounds_dpi`,
  `facts_from_getters_without_a_monitor_has_no_work_area`,
  `handle_report_rejects_without_reading_facts`, `handle_report_maximized_skips_and_marks_pending`,
  `handle_report_fitted_window_is_already_fitted`, `handle_report_off_shape_applies`,
  `handle_report_facts_error_keeps_the_ratio` and `handle_report_apply_failure_errs_and_keeps_pending`
  (R19). Criteria in section 8, task 7. The command body left untested is getter calls, the
  `apply_fit` call inside the closure and logging; M10 S1 and S4 cover it.
- Drift tests:
  - `config_matches_constants` (R11): `include_str!("../tauri.conf.json")` parsed with `serde_json`;
    `width == BASE_WIDTH_PX`, `minWidth == MIN_WIDTH_PX`, and `minHeight` is absent.
  - `zoom_max_matches_layout_ts`: `include_str!("../../src/lib/layout.ts")` contains
    `format!("export const ZOOM_MAX = {ZOOM_MAX};")`.
  - `fit_outcomes_match_fit_ts` (task 8): `include_str!("../../src/lib/fit.ts")`'s `FIT_OUTCOMES`
    line lists exactly the six serialized names. This is the same pattern as tray.rs:666.

### 7.3 Windows HWND integration test (R9)

`#[cfg(all(windows, test))] mod hwnd_tests` in `src-tauri/src/platform/windows/aspect.rs`.
- Fixture:
  - Each test registers (once) a class `CUT_ASPECT_TEST` whose window proc is `counting_wndproc`:
    it increments a thread-local per-message counter, then returns `DefWindowProcW(..)`. Tests run
    on separate threads and `SendMessageW` to a same-thread window calls the procs on that thread,
    so the counters never race. The counter is the forward count (the subclass forwards into the
    class proc).
  - Geometry comes from the host, never from constants (R23, T2-M3): the test reads the primary
    work area with `SystemParametersInfoW(SPI_GETWORKAREA)` and computes the outer size for a
    client of 800 x `ceil(800 x 640/980)` = 800 x 523 with
    `AdjustWindowRectExForDpi(WS_OVERLAPPEDWINDOW, FALSE, 0, dpi)` (dpi from `GetDpiForWindow` on a
    probe window). The window is created hidden at (work.left + 40, work.top + 40) on the primary
    monitor. Every test's grown rect then stays at least 40 px inside the work area. A work area
    smaller than 1000 x 760 fails the test with a message naming its size (not a skip).
  - It holds an `Arc<AspectState>` with ratio 980/640 and calls `install_hwnd`, or
    `install_hwnd_with` and a test `ProcHooks` where a row says so.
- Oracles: `nc_oracle` is the `AdjustWindowRectExForDpi` frame, and `fitted(cw, ch)` is
  `0 <= ch - ceil(cw x 640/980) <= 1`. Neither reads the code under test.
- `sizing_rewrites_every_edge`: for each `WMSZ_*`, the rect grown 40 px on the dragged edges gives
  TRUE from `SendMessageW(WM_SIZING, ..)`.
  - Wiring: the rect equals `fit_rect(edge, ..)` computed from `hwnd_facts`.
  - Geometry, independent (R23): the client `rect - nc_oracle` is `fitted`, the opposite edges are
    unchanged, and at least one dragged edge moved outward.
- `sizing_passes_through_without_a_ratio`: the rect is unchanged.
- `exit_size_move_applies_pending_synchronously`:
  - `WM_ENTERSIZEMOVE` sets `in_size_move()`.
  - Then `mark_pending` and an off-shape `SetWindowPos`.
  - `WM_EXITSIZEMOVE` clears both flags. When `SendMessageW` returns, the `GetClientRect` size is
    `fitted`, and the forward count for `WM_EXITSIZEMOVE` is 1.
- `size_restored_applies_pending`: an off-shape window with a pending fit is fitted by
  `WM_SIZE`/`SIZE_RESTORED`, and the flag is cleared.
- `dpi_changed_applies_when_not_in_a_size_move` (R20): an off-shape window, nothing pending;
  `SendMessageW(WM_DPICHANGED, MAKEWPARAM(dpi, dpi), &current_window_rect)`. On return the client
  is `fitted`, `is_pending()` is false, and the forward count for `WM_DPICHANGED` is 1.
- `dpi_changed_in_a_size_move_marks_pending` (R20): `WM_ENTERSIZEMOVE`, then an off-shape
  `SetWindowPos`, then `WM_DPICHANGED`: the size is unchanged and `is_pending()` is true;
  `WM_EXITSIZEMOVE` then fits it.
- `apply_failure_in_the_proc_keeps_pending` (R19): installed with
  `ProcHooks { apply: |_, _, _| false, .. }`; an off-shape window with a pending fit gets
  `WM_EXITSIZEMOVE`: the size is unchanged and `is_pending()` is still true.
- `a_panic_after_the_forward_forwards_once` (R22): installed with `ProcHooks { after_forward: panics, .. }`;
  `SendMessageW(WM_EXITSIZEMOVE)` returns, the forward count for `WM_EXITSIZEMOVE` is 1, and the
  next message (`WM_SIZE`/`SIZE_RESTORED`) is still forwarded once (the window keeps working). The
  once-per-process WARN is not asserted here, because `PANIC_WARNED` is process-wide and test order
  is not fixed; `run_guarded_returns_fallback_on_panic_and_warns_once` (7.2) owns that assertion.
- `zoomed_window_keeps_pending`: `WS_MAXIMIZE` is set with `SetWindowLongPtrW`, then
  `WM_EXITSIZEMOVE` leaves the size unchanged and the fit pending.
  - Hypothesis - validate before building: `IsZoomed` reads the style bit of a hidden window. If it
    does not, this row moves to the M10 human list as H4b and the test is deleted, not skipped.
- `destroy_frees_subclass_data`: `Arc::strong_count` is 2 after install and 1 after `DestroyWindow`.
- Facts and apply (task 5): `hwnd_facts_reads_a_hidden_window`,
  `apply_hwnd_resizes_without_moving`, `apply_hwnd_moves_up_and_resizes_in_one_call` (criteria in
  section 8, task 5, against the fixture's own numbers: the outer rect it created, `nc_oracle` and
  the `SPI_GETWORKAREA` rect, not `GetWindowRect`/`GetClientRect` restated).
- `wmsz_literals_match_windows_sys`: `Edge::from_wmsz(WMSZ_LEFT)` through
  `Edge::from_wmsz(WMSZ_BOTTOMRIGHT)` give the 8 edges, using the windows-sys constants.
- It cannot cover DefWindowProc re-clamping, snap or the modal loop. Those are M10 H5 and H12.

### 7.4 M10 (manual harness, release build)

- Location: `.claude-work/window-hug/manual/`. `harness.psm1`, `run-checks.ps1` and `selftest.ps1`
  are copied from `.claude-work/optimize/manual/`, because each plan has its own work folder.
- Harness additions:
  - `M10` in the `-Check` ValidateSet, and an `M10.md` like M0-M5;
  - `GetClientRect` and `ClientToScreen` in `CutWin`;
  - a pure judge `Test-AspectShape -ClientW -ClientH -ContentH` in integer arithmetic (R24,
    T3-I1): `want = [math]::Floor(($ClientW * $ContentH + 979) / 980)` (that is
    `ceil(ClientW x ContentH / 980)` with no float division), and it passes exactly when
    `0 <= ClientH - want <= 1` and `ContentH` is an integer > 0 (R23, T2-M4). It matches
    `plan_fit`'s tolerance (a client up to 1 px taller is fitted, a shorter one never is), so the
    harness and the code agree on "fitted". The operands are `[int64]`, so the product cannot
    overflow and `Floor` sees an exact quotient numerator;
  - `selftest.ps1` cases for the judge, written first: pass at +0 and at +1 (the 1-px-taller case),
    fail at +2, fail at -1 (1 px short), and fail on a zero or negative `ContentH`; plus two rows at
    exact-integer points whose ratio is not 6-decimal exact (R24): `ClientW 1960, ContentH 1282`
    passes at `ClientH 2564` (+0) and fails at 2563 (-1), and `ClientW 1960, ContentH 641` passes at
    `ClientH 1282` (+0) and fails at 1281 (-1);
  - `Get-ContentReport -Since`: the DEBUG "content height report" lines parsed to
    {content_h, ratio, outcome}; `content_h` is the integer the judge uses, and the ratio is the
    logged 6-decimal string, kept for humans only (R15, R24);
  - a sampler thread at about 2 ms that takes the client size from one `GetClientRect` call per
    sample (the shape criterion needs no second call, so no torn sample), and in a separate stream
    `GetWindowRect` for the anchor check only (R23, T2-M4);
  - `ConvertFrom-LogLine` gains `Target = [string]$o.target` (logging.rs writes the target,
    `.with_target(true)`), and `Get-AspectWarnLines -Ctx` returns the WARN events since
    `$Ctx.Since` whose target starts with `cut_core::window_aspect` or `cut_core::platform` (R28,
    T3-M4). WARN lines from other modules (poller, network) do not count against M10.
- Each check sets its own settings before launch with `Use-Settings -DebugLevel $true
  -CloseToTray <value>` (R28, T3-M4): S1 uses `-CloseToTray $false`, because `Stop-AppGraceful`
  exits only an instance started with close_to_tray off (run-checks.ps1:177) and S1 needs a real
  exit so window-state saves its file; S4 and S6 use `-CloseToTray $true`, because they hide to the
  tray and reopen; S2, S3 and S5 use the harness default `$true`. Results are INCONCLUSIVE until
  the copied harness has run once on this machine.
- The `content_h` for every judge is the one in the latest "content height report" line (R15,
  R24). That line is written on every report, fitted or not; "window fitted" is written only when a
  fit is applied.
- Scriptable, with numeric pass criteria:
  - **S1 cold start, two branches (R15):**
    - Preparation: launch, wait for the first "content height report", record its `content_h` C
      and the client width `cw` (physical), stop gracefully. With no instance running, edit the
      `main` entry of `.window-state.json` in tauri's `app_config_dir()` (`%APPDATA%\<identifier>`;
      file name from tauri-plugin-window-state lib.rs:36) so `height = ceil(cw x C / 980) + 40`
      physical px, off-shape by 40 (>= 20). The original file is restored after M10.
    - **S1a forced fit:** launch. The first "content height report" says `applied`, a
      "window fitted" {source: report} line follows, and `Test-AspectShape` on the `GetClientRect`
      size passes with that report's `content_h`.
    - **S1b already fitted:** stop gracefully and launch again without editing. The first
      "content height report" says `alreadyFitted`, no "window fitted" {source: report} line
      appears within 5 s of it, and `Test-AspectShape` passes.
    - Both launches: "subclass installed" is present and its `Target` starts with
      `cut_core::platform` (expected `cut_core::platform::windows::aspect`; this positive control
      proves the target prefix the WARN filter relies on), and `Get-AspectWarnLines` returns nothing
      (R28: WARN lines from the `window_aspect` and `platform` targets only).
    - Both stops use `Stop-AppGraceful`, and each must return `Invoked` and `Exited` true; a stop
      that does not is a FAIL of S1, never a force-kill (a killed process does not save the state
      file the next launch reads).
  - **S2 drags at the current DPI:** first centre the window in the work area with
    `Set-WindowGeometry` (R23, T2-M4). For each of the 8 hit points, `room` is the free space in px
    between the dragged edge and the work area's edge on the outward side (for a corner, the smaller
    of its two edges), and the step is `s = min(10, floor(room / 30))`; a hit point with `s < 3` is
    INCONCLUSIVE and named. Mouse down 2 px inside the edge, 30 `SetCursorPos` steps of `s` px
    outward, 30 back, mouse up.
    - Every sampler client frame passes `Test-AspectShape`, and the opposite edges (window-rect
      stream) move by 0 px.
    - Each drag logs one "size-move end" with `0 <= ratio_err_px <= 1`.
    - This is the objective no-flicker criterion (goal item 2).
  - **S3 maximize:** before maximizing, read the caption height
    `cap = (client top in screen px - window top) - (window bottom - client bottom in screen px)`
    with `ClientToScreen` and `GetWindowRect`. After `ShowWindow(SW_MAXIMIZE)`, `IsZoomed` is true,
    the client width equals the work area width +-2, and the client height equals the work area
    height minus `cap` +-2 (R23, T2-M4). After restore, the window rect equals the pre-maximize
    rect +-2 px.
  - **S4 reopen after a custom width:** `SetWindowPos` to client width 1300, close to tray, reopen.
    - The client width is 1300 +-2, and `Test-AspectShape` passes.
    - A "window fitted" {source: ready} line or no fit line appears. No fit line is right when the
      shape already matched.
  - **S5 zoom independence:** widths are physical px at the logged scale (R28, T3-M4):
    `s = dpi / 96`, with `dpi` from the "subclass installed" line of this launch. `SetWindowPos`
    to client width `round(980 x s)` (zoom 1), wait for the fit, then drag the right edge to client
    width `round(1470 x s)` (zoom 1.5). When `round(1470 x s)` exceeds the work-area width minus
    the frame, S5 is INCONCLUSIVE and the run names the dpi and the work area. The drag's
    "size-move end" line carries the same `dpi`; a different value (the window crossed monitors)
    makes S5 INCONCLUSIVE.
    No new "content height report" appears, or each new one carries `content_h` within +-1 of the
    value before the drag (the WebView2 counterpart of the R1 probe; the +-1 covers the UNSURE
    layout-unit fraction of section 3.1, R23, T2-M7). The run records the exact values, so a
    nonzero difference is visible.
  - **S6 loading does not collapse:** from the tray reopen to 1 s after the first report, the sampler
    never sees a client height below 0.9 x the pre-close height.
- Human-only, each a yes/no:
  - **H1** At 150% display scale, S2 by eye: every frame keeps the shape, with no flicker.
  - **H2** Open a row's history drawer, then close it. The window grows, then returns to the
    pre-drawer client height +-2 px (from the "window fitted" lines).
  - **H3** Retired in r2 (R16): no harness or human input can change the content while an edge drag
    is held (the size-move loop owns the mouse and the keyboard, and the optimize harness has no
    channel into the running app's data; section 10). The deferred path is a listed hypothesis in
    section 8. The number stays so later checks keep their names.
  - **H4** Maximize, open a drawer, restore. The restored window passes `Test-AspectShape`.
  - **H5** Drag the title bar to the top edge while a fit is pending. The window stays maximized
    (`IsZoomed`) and the line says `pending: skipped:maximized`. Then Win+Left snap: record whether
    the next fit leaves the snap (accepted either way; recorded for the UNSURE ordering in section 5).
  - **H6** Corner "cover": on each corner drag the cursor stays on the frame.
  - **H7** A fitted window at 100% and at 150% shows no scrollbar.
  - **H8** On a 4K monitor at 100%, or with a window driven past 2450 px wide: the window stops
    widening at 2450 + frame. Maximized shows centred content with side slack, and the log shows no
    repeating "window fitted" lines.
  - **H9** After a drag across monitors of different DPI, the "size-move end" line shows
    `pending: applied` or `unchanged` and `0 <= ratio_err_px <= 1`. With the window open, changing
    the display scale in Windows Settings logs "window fitted" {source: dpi} (or nothing when the
    shape held), and `Test-AspectShape` then passes (R20).
  - **H10** Opening Settings grows the window. The FailureDetail modal in a one-account window is
    fully on screen and scrolls inside.
  - **H11** With one account and with zero accounts, the DEBUG "content height report" is at least
    100 (`MIN_CONTENT_H + 20`).
  - **H12** After a drag at the minimum width, the "size-move end" line shows
    `0 <= ratio_err_px <= 1`. This checks whether DefWindowProc re-clamps the rewritten rect.

### 7.5 Behaviour to test map

| Behaviour | Test |
|---|---|
| Goal 1: shape 980 : H, no exposed slack, no scrollbar | `plan_fit_table`, `fit_client_table`, css-contract `.app is content-sized and fixed-width`; S1, H7 |
| Goal 1: H is the border box, zoom-independent, ceil(h - 1/64) | `contentHeight.test.ts` (ceil, layout-unit fraction, `reads borderBoxSize`); S5 |
| Goal 2: drag keeps the ratio | `fit_rect_*`, `sizing_rewrites_every_edge`; S2, H1, H6, H12 |
| Goal 3: window follows content, no zoom flash | `step_table`, `fitReducer` table (`applied waits in transit until a viewport that holds it`, `two reports in flight: ...`), `no zoom flash on grow or shrink, in either arrival order`, `createFitController` `onMeasured publishes measured, sends c, then publishes the outcome for c`; H2, H10 |
| R17: cold start zooms by width until the first outcome | `zoomContentH` `null after measured with no outcome`, `null after a first deferred`; `windowZoom` `null contentH: width term only` |
| R14: hook glue is a pure, tested controller | `createFitController` rows (all nine); `contentHeight.test.ts` imports no backend |
| R27: StrictMode's dev double mount gets a fresh controller | No automated test: vitest has no renderer, and M10 runs a release build, where StrictMode does nothing. Covered by the structure in 3.1 (controller built inside the `[]` effect, disposed by its cleanup), by `dispose cancels the timer and drops later resolutions` (a disposed controller is inert, which is why reuse is the bug), and by task 8's recorded dev check (maximize under `tauri dev`: no vertical overflow) |
| Goal 4: maximized zoom and slack; 2450 cap | `windowZoom` `maximized: height term wins`, `client_bounds_table` (4K); S3, H8 |
| Goal 5: capped, move-up, crossed bounds, no panic | `fit_client_table`, `plan_fit_table`, `fit_rect_crossed_bounds`, `run_guarded_*` |
| Goal 6: other platforms compile, portable fit | `cargo clippy --all-targets` on Windows only (UNSURE: no non-Windows build in the gates); `other.rs` is two functions |
| Goal 7: dead layouts removed | css-contract `no card-layout rules remain`; `npm run build` (tsc) fails on any surviving import |
| R3: no report while loading; range validation | `attachContentHeight: disabled or detached constructs no observer`, `attachContentHeight: enabled subscribes`; App passes `enabled` only in the loaded branch (S6); `validate_content_h_*` |
| R4: every skip and defer resumes | `step_table`, `exit_size_move_applies_pending_synchronously`, `size_restored_applies_pending`, `dpi_changed_in_a_size_move_marks_pending`, `zoomed_window_keeps_pending`, `shouldResend`, `a viewport event with retry resends lastMeasured after 200 ms without a measured event`; H4, H5 (the held-drag content change is a hypothesis, section 8) |
| R18: resends are debounced, never concurrent | `N viewport events while deferred produce one resend`, `no resend while a call is in flight`, `no resend without retry`, `a timer that fires after retry cleared sends nothing`, `dispose cancels the timer and drops later resolutions` |
| R19: an apply failure errs and keeps the fit pending | `handle_report_apply_failure_errs_and_keeps_pending`, `apply_failure_in_the_proc_keeps_pending`, `a rejection warns once and publishes failed for c` |
| R20: a DPI change refits | `dpi_changed_applies_when_not_in_a_size_move`, `dpi_changed_in_a_size_move_marks_pending`; H9 |
| R21: move up by the growth only; same-size windows stay put | `plan_fit_table` rows `a same-size window below the work area is Unchanged` and `a grow from an already-overflowing position moves up by the growth only`; `apply_hwnd_moves_up_and_resizes_in_one_call` |
| R22: a panic never doubles the forward | `a_panic_after_the_forward_forwards_once`, `run_guarded_returns_fallback_on_panic_and_warns_once` |
| R6: reopen keeps a custom width | S4 |
| R9: subclass memory safety | `destroy_frees_subclass_data` |
| R10: invoke key | `setContentHeight invokes set_content_height with the camelCase key` |
| R11: one source for 980, 735, 2.5 and the outcomes | `the window config matches the base canvas`, `config_matches_constants`, `zoom_max_matches_layout_ts`, `fit_outcomes_match_fit_ts` |
| MIN_CONTENT_H estimate | H11 |
| Logging | S1 ("content height report" with `content_h` and a 6-decimal ratio, "window fitted" {source: report}, no WARN from the `window_aspect`/`platform` targets), S2 (size-move end, `ratio_err_px`), S5 ("subclass installed" and "size-move end" `dpi`), H5, H9 ({source: dpi}); selftest `Get-AspectWarnLines` rows |
| M10 judge agrees with `plan_fit` at exact-integer points (R24) | selftest `Test-AspectShape` rows `ClientW 1960` with `ContentH 1282` and `641` |
| `holds` boundary is exact (R29) | `holds tolerates one width px`, `holds rejects one height px short` |
| Task 3 leaves no test reading a deleted rule (R25, R26) | css-contract `no card-layout rules remain`, `.ring-sm svg fills its wrapper`, `.ring-sm is a fixed 20px glyph`; gauge.test `ringDash` rows on `RING_SIZES.sm.radius` |

## 8. Tasks (for the plan)

The plan expands each task into steps in the optimize plan's format
(`docs/superpowers/plans/2026-10-06-optimize.md`). Shared rules for every task:
- **Order inside a task (TDD):** write the named tests, run them and record the expected failure in
  `.claude-work/window-hug/logs/T<n>-red.log`, implement, run them green, then the four gates.
- **The four gates**, in this order, one at a time, each to a log with `echo "exit=$?"` and
  `tail -40`:
  ```
  cargo test --manifest-path src-tauri/Cargo.toml > .claude-work/window-hug/logs/T<n>-test.log 2>&1
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings > .claude-work/window-hug/logs/T<n>-clippy.log 2>&1
  npm test > .claude-work/window-hug/logs/T<n>-vitest.log 2>&1
  npm run build > .claude-work/window-hug/logs/T<n>-build.log 2>&1
  ```
  A gate passes only with `exit=0`; a skipped test counts as failing.
- Every task leaves all four gates green and is one commit. Before the commit, compare
  `git diff --cached --stat` with `git diff --cached --stat --ignore-cr-at-eol` (CRLF-flip check).
- Interim behaviour between tasks is named in each task, so a reviewer can tell an expected
  intermediate state from a defect. The window starts hugging only at task 8.

**Task 1: shell box, modal floor, fixed chart height.** Agent `implementer`, tier logic, risk low.
Depends on: nothing.
- Why: R1 and R13 (tests M4, arch M6). The measured box must be content-sized before anything
  reports it, and the chart must stop depending on the window.
- Files: `src/styles.css` (`.app` 40, `.chart` 226, `.modal-body` 319), `src/lib/layout.ts`
  (`shellVars`, `CHART_HEIGHT`, `ROW_BORDER` comment), `src/lib/layout.test.ts`,
  `src/lib/css-contract.test.ts`, `src/components/HistoryDrawer.tsx`,
  `src/components/AccountRow.tsx` (47, 88, 108), `src/components/AccountsTable.tsx` (241); deleted:
  `src/hooks/useChartHeight.ts`, `src/lib/chart.ts`, `src/lib/chart.test.ts`.
- Tests first: css-contract `.app is content-sized and fixed-width`, `.chart height reads
  --chart-h`, `.modal-body max-height is the window less the gutters`, `ASSERTED_VARS` gains
  `--base-w` and `--chart-h`; layout.test `shellVars` with `--base-w` and `--chart-h`.
- Interim: the window keeps its current free shape; short content leaves `body` background below
  it, and the drawer chart is 220 local px.
- Done when: the four gates pass, and `grep -rn "useChartHeight\|chartHeightPx" src` matches nothing.

**Task 2: the two-term zoom and the window config.** Agent `implementer`, tier logic, risk low.
Depends on: task 1.
- Why: R11 and goal items 4-5. The zoom needs the height term, and the config must agree with the
  constants before the Rust drift test lands in task 4.
- Files: `src/lib/layout.ts` (`windowZoom`, delete `BASE_HEIGHT`), `src/lib/layout.test.ts`,
  `src/App.tsx` (passes `null` as `contentH` until task 8), `src-tauri/tauri.conf.json`
  (`minWidth` 735, `minHeight` removed).
- Tests first: the `windowZoom` cases and `the window config matches the base canvas` (section 7.1).
- Interim: with `null`, the zoom is the width term only, so a window shorter than its content
  scrolls at the root until task 8.
- Done when: the four gates pass, and `grep -rn "BASE_HEIGHT" src` matches nothing.

**Task 3: remove the narrow and cards layouts (R7).** Agent `implementer`, tier logic, risk medium:
a wide deletion across components. Depends on: task 1 (`shellVars` and `ASSERTED_VARS` were
touched there).
- Why: R7. The local width is 980 in every state, so `layoutFor` never returns `narrow` or `cards`
  and their code is dead.
- Files: `src/lib/layout.ts` (`layoutFor`, `Layout`, `BREAKPOINTS`, `NARROW_HIDDEN`,
  `autoHiddenColumns`, `SHELL_PADDING`, the ring variables in `shellVars`), `src/lib/layout.test.ts`,
  `src/App.tsx` (cards branch 80-85, Settings `layout` prop), `src/components/AccountCard.tsx`
  (deleted), `src/components/Ring.tsx` (the `md` variant), `src/lib/gauge.ts` and
  `src/lib/gauge.test.ts` (`RING`, `RING_SIZES.md`), `src/components/Header.tsx` (`compact`),
  `src/lib/present.ts` and `src/lib/present.test.ts` (`countPlacement`'s `compact`),
  `src/components/Settings.tsx` (line 6 import, the width hints at 152-182), `src/styles.css`
  (card and ring rules 197-207; `.ring svg, .ring-sm svg` becomes `.ring-sm svg`, R25),
  `src/lib/css-contract.test.ts`.
- Tests first: css-contract `no card-layout rules remain` (the `(?![\w-])` regex, R25) and
  `ASSERTED_VARS` without the ring variables; layout.test `shellVars` without them; present.test
  single-argument `countPlacement`; gauge.test with `sm` only. In the same step (R26, T3-M2):
  - css-contract deletes `.card has no border-radius (flat look)`, `.ring width clamps between the
    ring variables`, `.card-rings is an inline-size container` and `.ring-label may use the full
    ring width`, and rewrites `.ring svg fills its wrapper` as `.ring-sm svg fills its wrapper`
    (selector `.ring-sm svg`); `.ring-sm is a fixed 20px glyph` stays;
  - gauge.test re-points the `ringDash` describe at `RING_SIZES.sm.radius`, keeping its six rows,
    and deletes the `RING` geometry case and the `RING_SIZES.md` lines (7.1).
  So the red run fails only on the new expectations: `no card-layout rules remain` and
  `.ring-sm svg fills its wrapper` (rules not yet edited), and layout.test's `shellVars` `toEqual`
  (ring variables still present). The trimmed `ASSERTED_VARS`, the re-pointed `ringDash` rows and
  the single-argument `countPlacement` rows may already pass at runtime; they guard the deletion,
  and `npm run build`
  (tsc) is what fails if a removed export is still imported.
- Interim: none visible; the deleted paths were unreachable.
- Done when: the four gates pass, and
  `grep -rn "layoutFor\|BREAKPOINTS\|autoHiddenColumns\|AccountCard\|SHELL_PADDING\|NARROW_HIDDEN" src`
  matches nothing.

**Task 4: the Rust pure core.** Agent `implementer`, tier logic, risk medium: the geometry every
later task trusts. Depends on: task 2 (the config values the drift test reads).
- Why: R3, R4, R5, R11, R13. All decisions live in portable, table-tested functions, so the proc
  and the command are thin.
- Files: `src-tauri/src/window_aspect.rs` (new: constants, `Rect`, `Size`, `AspectState`,
  `validate_content_h`, `accept_report`, `decide_fit`, `min_client_px`, `client_bounds`,
  `fit_client`, `WindowFacts`, `plan_fit`, `step`, `Edge`, `fit_rect`, `run_guarded`, `FitOutcome`,
  `outcome_of`), `src-tauri/src/lib.rs` (`pub mod window_aspect;`).
- Tests first: every section 7.2 test except the command-glue tests (`facts_from_getters_*` and
  `handle_report_*`, task 7) and `fit_outcomes_match_fit_ts` (task 8) (R23, T2-M1). That includes
  the r2 rows: `fit_client_table`'s `min_h` row, the two R21 `plan_fit_table` rows and
  `fit_rect_height_cap`.
- Interim: nothing calls the module yet; `pub` items raise no dead-code lint.
- Done when: the four gates pass, and `grep -n "clamp(\|unwrap()\|expect(" src-tauri/src/window_aspect.rs`
  matches only inside `mod tests`.

**Task 5: platform facts and apply.** Agent `implementer`, tier logic, risk medium: first Win32
calls. Depends on: task 4.
- Why: R5, R8, R13 (arch M3). The command and the proc share one apply (`apply_hwnd`) and the
  proc reads facts without tauri getters.
- Files: `src-tauri/src/platform/mod.rs`, `src-tauri/src/platform/windows/mod.rs`,
  `src-tauri/src/platform/windows/aspect.rs` (`hwnd_facts`, `apply_hwnd`, `apply_fit`, and the
  `hwnd_tests` fixture), `src-tauri/src/platform/other.rs` (`apply_fit`), `src-tauri/src/lib.rs`
  (`pub mod platform;`), `src-tauri/Cargo.toml` (the four features and `Win32_System_LibraryLoader`).
- Tests first, in `hwnd_tests` with the section 7.3 fixture (the `CUT_ASPECT_TEST` class with
  `counting_wndproc`, geometry from `SPI_GETWORKAREA` and `AdjustWindowRectExForDpi`, the
  `nc_oracle` and `fitted` oracles; the fixture lands in this task). The oracles are the fixture's
  own numbers, never the getters under test restated (R23, T2-M3):
  - `hwnd_facts_reads_a_hidden_window`: `outer` equals the rect the fixture passed to
    `CreateWindowExW`; `nc` equals `nc_oracle`; `client` equals `outer` less `nc_oracle`
    (800 x 523); `work` equals the `SPI_GETWORKAREA` rect; `dpi > 0`; `maximized` and `minimized`
    false.
  - `apply_hwnd_resizes_without_moving`: a `Resize` to client 900 x `ceil(900 x 640/980)` with the
    top unchanged; afterwards the outer `left` and `top` equal the created ones and the client size
    (`GetClientRect`) is `fitted` with width 900.
  - `apply_hwnd_moves_up_and_resizes_in_one_call`: a `Resize` with `top` = created top - 20 and a
    client 20 px taller; afterwards the outer top is the created top - 20 and the client is the
    planned size, both after one `apply_hwnd` call.
  - `wmsz_literals_match_windows_sys`.
- Interim: nothing calls `apply_fit` yet.
- Done when: the four gates pass.

**Task 6: the subclass proc, driven by the HWND integration test (R9).** Agent `implementer`,
tier logic, risk high: an `extern "system"` callback with a raw pointer. Depends on: task 5 (the
fixture, facts and apply).
- Why: R9. The integration test is the contract for the proc: the tests are written first against
  a real hidden HWND, then the proc is written to pass them. A tests-only task after the proc would
  reverse the TDD order, and one before it would leave the tree red.
- Files: `src-tauri/src/platform/windows/aspect.rs` (`SubclassData`, `SUBCLASS_ID`, `ProcHooks`
  with `ProcHooks::REAL`, `install_hwnd`, `install_hwnd_with`, `install`, `proc` with the
  `WM_DPICHANGED` arm, `PANIC_WARNED`, and the INFO lines "subclass installed" {label, dpi} and
  "size-move end" {.., dpi, ..} whose `dpi` field M10 S5 reads, R28), `src-tauri/src/platform/other.rs`
  (`install`).
- Tests first: every section 7.3 test not landed in task 5, including the r2 rows
  `dpi_changed_applies_when_not_in_a_size_move`, `dpi_changed_in_a_size_move_marks_pending` (R20),
  `apply_failure_in_the_proc_keeps_pending` (R19) and `a_panic_after_the_forward_forwards_once`
  (R22), and the forward-count assertion in `exit_size_move_applies_pending_synchronously`. The red
  run fails on the missing `install_hwnd` and `install_hwnd_with`.
- Interim: nothing installs the subclass on the app window yet.
- Done when: the four gates pass, and the `zoomed_window_keeps_pending` hypothesis is settled: the
  test passes, or it is deleted and M10 H4b is added (section 7.3), with the result written in the
  task's report.

**Task 7: the command and the plugin.** Agent `implementer`, tier logic, risk medium: plugin order
and the main-thread assumption. Depends on: task 6.
- Why: R3, R4, R6, R13 (arch M1). This wires the core to tauri.
- Files: `src-tauri/src/window_aspect.rs` (`facts_from_getters`, `handle_report`,
  `set_content_height`, `window_facts`, `fit_now`, `plugin`), `src-tauri/src/lib.rs` (plugin after
  window-state at 208-212, `generate_handler!` 218-235).
- The command's logic is split so it tests without a tauri runtime (the crate has no `tauri::test`
  feature):
  - `facts_from_getters(inner: (u32, u32), outer_pos: (i32, i32), outer: (u32, u32), scale: f64, work: Option<Rect>, maximized: bool, minimized: bool) -> WindowFacts`;
  - `handle_report(state, content_h, facts: impl FnOnce() -> AppResult<WindowFacts>, apply: impl FnOnce(&FitPlan) -> AppResult<()>) -> AppResult<(FitOutcome, Option<FitPlan>)>`
    (R19).
  The `#[tauri::command]` only gathers getters, calls `handle_report` with
  `|plan| platform::apply_fit(&window, plan)` as `apply`, and logs. `fit_now` shares the same
  re-mark on failure.
- Tests first: `facts_from_getters_rounds_dpi` (1.5 gives 144, 1.25 gives 120);
  `facts_from_getters_without_a_monitor_has_no_work_area`;
  `handle_report_rejects_without_reading_facts` (the closure panics if called; 0 gives
  `OutOfRange`); `handle_report_maximized_skips_and_marks_pending`;
  `handle_report_fitted_window_is_already_fitted` (the `apply` closure panics if called);
  `handle_report_off_shape_applies` (the `apply` closure records the plan it was given, which
  equals the returned `Some(plan)`, and the outcome is `Applied`);
  `handle_report_facts_error_keeps_the_ratio` (the ratio is stored, the error returns);
  `handle_report_apply_failure_errs_and_keeps_pending` (R19, A2-M6): an off-shape window and an
  `apply` closure returning `Err(AppError::Internal(..))`; the call returns `Err`, `is_pending()` is
  true afterwards, and the ratio is stored.
- Interim: the backend command exists; the frontend does not call it yet, so the ready hook fits
  only from a ratio stored earlier in the same process (none on a cold start).
- Done when: the four gates pass, and `grep -n "window_aspect::plugin" src-tauri/src/lib.rs` shows
  it on the line after the window-state plugin.

**Task 8: frontend reporting and the fit reducer.** Agent `implementer`, tier logic, risk medium:
the no-flash ordering. Depends on: tasks 2 and 7.
- Why: R2, R3, R10. The window starts hugging here.
- Files: `src/lib/contentHeight.ts` (`subscribeContentHeight`, `attachContentHeight`; no backend
  or tauri import, R14) and `src/lib/contentHeight.test.ts`; `src/lib/fit.ts` (`FIT_OUTCOMES`,
  `FitOutcome`, `fitReducer`, `zoomContentH`, `shouldResend`, `RESEND_DEBOUNCE_MS`,
  `createFitController`; no tauri import) and `src/lib/fit.test.ts`; `src/hooks/useContentHeight.ts`
  (wiring only, no test file: its logic is in the two tested modules; the controller is created
  inside the `[]` mount effect and disposed by its cleanup, never held in `useRef(create...)`,
  R27); `src/lib/backend.ts`,
  `src/lib/backend.test.ts`, `src/lib/mockBackend.ts` (`console.info` record),
  `src/lib/mockBackend.test.ts`, `src/App.tsx` (`appRef`, the hook, the zoom),
  `src-tauri/src/window_aspect.rs` (`fit_outcomes_match_fit_ts`).
- Tests first: the section 7.1 `contentHeight.test.ts`, `fit.test.ts` (reducer, `zoomContentH`,
  `shouldResend`, no-flash and all nine `createFitController` rows with fake timers),
  `backend.test.ts` and `mockBackend.test.ts` rows, and the Rust drift test.
- Interim: none; this completes the feature.
- Done when: the four gates pass, and the StrictMode dev check (7.5, R27) is recorded in the task's
  report: under `npm run tauri dev` (StrictMode active), maximize the window; the content shows
  whole with side slack and no vertical overflow (the height term applies, which needs a non-null
  `zoomContentH`). A controller reused after its StrictMode dispose still sends, so the window
  still grows, but it publishes nothing, `zoomContentH` stays null, and the maximized content
  overflows the bottom: this check fails on exactly that bug.

**Task 9: M10 harness and run.** Agent `implementer` writes the harness; the orchestrator runs it.
Tier: tests-only. Risk: medium (Win32 timing in PowerShell). Depends on: task 8.
- Why: R12. Drag feel, snap, DPI and the WebView2 zoom numbers are observable only on the real
  window.
- Files (untracked work folder): `.claude-work/window-hug/manual/harness.psm1`, `run-checks.ps1`,
  `selftest.ps1` (copied from `.claude-work/optimize/manual/`, then extended), `M10.md`.
- Harness additions (section 7.4): `GetClientRect` and `ClientToScreen` in `CutWin`;
  `Test-AspectShape -ClientW -ClientH -ContentH` in integer arithmetic
  (`want = [math]::Floor(($ClientW * $ContentH + 979) / 980)`, `0 <= ClientH - want <= 1`, R24);
  `Get-ContentReport` (its `content_h` feeds the judge; the ratio is for humans); `Target` in
  `ConvertFrom-LogLine` and `Get-AspectWarnLines` scoped to the `cut_core::window_aspect` and
  `cut_core::platform` targets (R28); the per-check `Use-Settings -CloseToTray` values (S1
  `$false`; S4, S6 `$true`; R28); S5's widths `round(980 x s)` and `round(1470 x s)` with
  `s = dpi / 96` from the "subclass installed" line (R28); the single-call client sampler with a
  separate window-rect stream; the S1 window-state edit with a backup taken before and restored in
  a `finally` block; the S2 centring and step rule; `M10` added to `run-checks.ps1`'s `-Check`
  `ValidateSet`.
- Tests first, run red before the judge, the parser and the filter exist:
  - the `Test-AspectShape` selftest cases: pass at +0 and +1, fail at +2 and at -1, fail on a zero
    or negative `ContentH`, and the exact-integer rows (R24): `ClientW 1960, ContentH 1282` passes
    at `ClientH 2564` and fails at 2563; `ClientW 1960, ContentH 641` passes at `ClientH 1282` and
    fails at 1281;
  - a `Get-ContentReport` case that parses a sample DEBUG line to `content_h` (integer) and the
    6-decimal ratio string;
  - a `Get-AspectWarnLines` case over three sample lines: a WARN with target
    `cut_core::window_aspect` and a WARN with target `cut_core::platform::windows::aspect` are
    returned, a WARN with target `cut_core::poller` is not (R28).
- Build: `npm run tauri -- build --no-bundle`, then
  `run-checks.ps1 -Exe src-tauri\target\release\claude-usage-tracker.exe -Check M10`.
  UNSURE: the optimize run's exact build flags are not recorded; the exe path is (run-checks.ps1:20).
- Done when: S1-S6 pass (an S2 hit point marked INCONCLUSIVE for lack of room is named in
  `M10.md` and re-run after moving the window to a larger monitor), every H1, H2 and H4-H12 answer
  is recorded yes or no in `M10.md` (H3 is retired, R16), the window-state file is back to its
  backup, and any no is returned to the orchestrator as a finding.

**Task 10: docs.** Agent `implementer`, tier docs, risk low. Depends on: task 9.
- Files: `README.md` (window behaviour: hugging, ratio lock, maximized slack, the 2450 cap, the
  capped scroll corner), `CHANGELOG.md`.
- Done when: `npm run build` passes and the two files describe the behaviour of section 1.

Hypotheses to validate while building, each with its check:
- DefWindowProc re-clamps a rewritten `WM_SIZING` rect (M10 H12).
- Snap layouts and Win+Arrow order against the lock (M10 H5).
- Corner "cover" feel (M10 H6).
- `IsZoomed` reads `WS_MAXIMIZE` on a hidden window (task 6).
- `MIN_CONTENT_H` sits at least 20 below the real minimum (M10 H11).
- A content change during a held drag fits on release (R16, T2-I3): the report returns `deferred`,
  the fit stays pending, and `WM_EXITSIZEMOVE` applies it after the forward. The proc side rests on
  `exit_size_move_applies_pending_synchronously` and `dpi_changed_in_a_size_move_marks_pending`
  (7.3), and the command side on `decide_fit_table` and `step_table` (in a size-move gives `Skip`
  with `is_pending()` true), whose `Skip` maps to `deferred` in `handle_report`. The ordering
  against tao's own `WM_EXITSIZEMOVE` handling rests on reading tao event_loop.rs:983, not on a
  run. No end-to-end check exists: the optimize harness has no channel
  into the running app's data. UNSURE candidate for a later harness: insert an account row into the
  app's SQLite store from a second process while the drag is held (the store is WAL with a 5 s busy
  timeout, so a concurrent writer is accepted), then wait for the dashboard refresh; whether the
  running app notices the external row without its own refresh tick is not researched.
- Reports are `Math.ceil(h - 1/64)` (3.1): if WebView2's border box carries a layout-unit
  fraction at some zooms, the `- 1/64` keeps the report stable across zoom steps; if it carries
  none, the subtraction changes nothing. Check: M10 S5 records the exact `content_h` values at zoom
  1 and 1.5 (UNSURE until that run).
- `window.innerHeight` rounding at fractional scales (3.1): a fitted window may sit a fraction of a
  CSS px short of `holds`. The `w - 1` tolerance absorbs one width px, and a transit that still
  does not settle keeps the smaller C in the minimum (the no-flash side) until the next
  `alreadyFitted`. Check: M10 S2 and H10 at the machine's current scale; 125% and 175% are not run
  (UNSURE), and the failure mode is bounded to a zoom up to one width px low.

## 9. Round-1 revision

One line per finding of `spec-tests-r1.md` (T) and `spec-arch-r1.md` (A), with the ruling that
resolved it. "Refined" marks a place where this text deviates from a ruling's wording, with the
reason.
- T-C1 / A-C1, stretched measured element: R1. `.app` loses `min-height`, the border box is
  measured, width is fixed (3.1). Refined: `var(--base-w)` instead of the literal `980px` (R11).
- T-I1, padding omitted from H: R1. `borderBoxSize` includes both gutters (3.1).
- T-I2 / A-I2, loading render collapses the window: R3. The hook is enabled only with a dashboard;
  the command validates the range (3.1, 3.2; S6).
- T-I3, crossed bounds and `f64::clamp`: R5. `fit_client` with ordered `min`/`max`, min width wins,
  `run_guarded` in the proc (3.2, 3.3, 4; `fit_rect_crossed_bounds`).
- T-I4, stale client size in the ready hook: R6, refuted. The restore is synchronous on the main
  thread; S4 guards it (3.2, 5).
- T-I5 / A-I3, no refit after maximize or minimize: R4. Every skip marks pending; `SIZE_RESTORED`
  and `WM_EXITSIZEMOVE` resume it; the frontend resends on retry (3.1, 3.3, 5). Refined: the
  frontend also retries after `skipped*`, which is the resume path on platforms without the proc.
- T-I6, decision logic not a pure unit: R4. `decide_fit`, `step`, `plan_fit`, `accept_report` and
  `AspectState` are table-tested (3.2, 7.2).
- T-I7, no test for the unsafe glue: R9. Section 7.3; task 6 writes it before the proc.
- T-I8, subscription contract undefined: R10. Four-point contract and test list (3.1, 7.1).
- T-I9, 735 and 980 in several places: R11. One constant per side and drift tests both ways
  (3.2, 7.1, 7.2).
- T-I10, M10 without pass criteria: R12. S1-S6 numeric, H1-H12 yes/no (7.4).
- T-M1 / A-I6, unreachable layouts: R7. Removed in task 3; flagged to Josh as reversible.
- T-M2, ZOOM_MAX and 4K refit loop: R13. `max_w` caps at `980 x ZOOM_MAX x scale`; the layout no
  longer reflows (3.2; `client_bounds_table`, H8).
- T-M3, slack description: R1. Fixed 980 width, `body` slack in both axes (goal 4, 3.1).
- T-M4, consistency items: R13. One work-area owner per context, chart files deleted, the drawer
  and row props removed, test changes listed, `windowZoom` fallback defined (width term only), the
  state managed in plugin `setup`, `min_client_px` pure. Refined: the fallback is the width term,
  not 640, so a restored window is never shrunk by a guessed height.
- T-M5 / A-I4, grow past the work area and two capped definitions: R5. The fit moves up, then caps;
  `fit_client` is the one definition (3.2, 4; `plan_fit_table`). Refined: the height floor is
  `MIN_CONTENT_H x ZOOM_MIN x scale`, since R3's `MIN_CONTENT_H x scale` would break the ratio at
  the minimum width.
- A-I1, zoom flash and oscillation: R2. The fixed layout removes oscillation; `fitReducer` owns the
  zoom height (3.1, 7.1). Refined: `applied` and `capped` pass through `transit`, and
  `zoomContentH` takes the minimum including the last measurement, so there is no flash in either
  arrival order, which R2's wording did not cover.
- A-I5, unguarded deferred apply: R4. `WM_EXITSIZEMOVE` forwards first and runs `step`, which keeps
  the fit pending while zoomed or iconic (3.3; `zoomed_window_keeps_pending`, H5).
- A-M1, where `AspectState` is managed: R13. Plugin `setup`, with a `try_state` guard (3.2; the
  setup-before-ready order was a hypothesis in r1 and is source-verified in r2, section 10).
- A-M2, forwarding reads as dropped: R13. Every handled message forwards except `WM_SIZING` (3.3).
- A-M3, `platform::work_area_height`: R13. Dropped; `Monitor::work_area()` outside the proc (3.4).
- A-M4, r0 task 6 (move the existing `cfg(windows)` code): R8. Dropped; only new code goes in
  `platform/windows/`; flagged to Josh.
- A-M5, rounding direction: R1. Reports and client heights use `ceil` (3.1, 3.2, 4).
- A-M6, Settings is content: R13. Settings grows the window; the modal body's floor is the window
  less the gutters (3.1; H10). Refined: a fixed px floor would push the centred overlay off-screen.
- A-M7, logging cannot explain a wrong shape: R13. One INFO per size-move end with
  `ratio_err_px` and the pending result; WARN on `SetWindowPos` failure (3.3, 6).

## 10. Round-2 revision

One line per finding of `spec-tests-r2.md` (T2) and `spec-arch-r2.md` (A2), with the ruling that
resolved it and where.
- T2-I1, hook glue untested: R14. `createFitController({ send, publish, warn })` in `fit.ts` with
  nine fake-timer rows; `subscribeContentHeight` and `attachContentHeight` move to
  `src/lib/contentHeight.ts` with no backend import; the hook is wiring only (3.1, 7.1, task 8).
- T2-I2, S1/S2 anchored on an absent line: R15. Both anchor on DEBUG "content height report" with a
  6-decimal ratio; S1 forces a fit with an off-shape window-state file (+40 px) and has an
  `alreadyFitted` branch (3.2 step 5, 6, 7.4, task 9).
- T2-I3, H3 not executable: R16. The optimize harness (`settings.py`, `harness.psm1`,
  `m5-hold.ps1`) edits settings before launch and holds memory, but has no channel into the running
  app's data during a drag. H3 is retired and the deferred path is a section 8 hypothesis resting on
  the 7.3 rows and tao event_loop.rs:983.
- T2-M1, `windowZoom` row did not agree: R23. (1470, 960, 640) gives 1.5, plus a width-wins row
  (7.1).
- T2-M2, task 4 pulled in task 7's tests: R23. Task 4 excludes the command-glue tests (section 8).
- T2-M3, Rust table and fixture gaps: R23. `fit_client_table` `min_h` row, `fit_rect_height_cap`,
  `plan_fit_table` numbers, fixture geometry from `SPI_GETWORKAREA` and independent oracles (7.2,
  7.3, task 5).
- T2-M4, M10 false fails: R23. Judge `0 <= client_h - ceil(client_w / ratio) <= 1`, single-call
  sampler, centre before S2 with a room-based step, S3 numbers from the caption height (7.4).
- T2-M5, cold-start shrink: R17. `zoomContentH` is null until the first outcome (3.1, 5, 7.1).
- T2-M6, resend per viewport event: R18. 200 ms debounce, never in flight (3.1, 5, 7.1).
- T2-M7, ceil differs across zooms: R23. `Math.ceil(h - 1/64)`; S5 tolerance +-1; hypothesis in
  section 8 (UNSURE until S5 runs).
- T2-M8, mock recording unobservable: R23. `console.info("mock: setContentHeight", px)` and a spy
  row (3.1, 7.1).
- T2-M9, failed apply loses the pending fit: R19. Command and proc re-mark pending; the command
  returns `Err` (3.2, 3.3, 5, 7.2, 7.3).
- A2-M1, transit settles on the wrong resize: R17. `transit` settles by geometry (`holds`) on
  `viewport` and `applied`; the "two reports in flight" row (3.1, 7.1).
- A2-M2, DPI change never refitted: R20. `WM_DPICHANGED` forwards, marks pending and applies
  outside a size-move; two 7.3 rows; section 5 corrected; H9 extended.
- A2-M3, move-up without growth and the invisible border: R21. Move up by `min(grow, overflow)`
  only; `Unchanged` ignores the top; the invisible border is accepted (3.2, 4, 7.2).
- A2-M4, panic fallback forwards twice: R22. `run_guarded` wraps only the work, three proc shapes,
  `a_panic_after_the_forward_forwards_once` (3.3, 7.3).
- A2-M5, resend storm during a drag: R18. Same fix as T2-M6.
- A2-M6, apply failure unspecified: R19. Same fix as T2-M9; `handle_report` takes `apply` as a
  closure (3.2, task 7).
- A2-M7, 3.1 and 5 disagreed on the cold-start zoom: R17. Both say width term only until the first
  outcome.
- Refined, recorded for the next lens:
  - R14's `dispatch` became `publish`: the controller owns `FitState` and publishes it, so the hook
    holds no reducer and the state has one owner.
  - `capped` settles at once: a capped window never satisfies `holds`, so geometry cannot settle it.
    The cost: on a grow into the cap whose reply beats the resize event, one frame zooms the new C
    by the pre-grow window's height term (3.1, 5). This supersedes r1's "`capped` passes through
    `transit`" (section 9).
  - The section 3 diagram's range was `[120, 10000]`; it is `[80, 10000]`, matching
    `MIN_CONTENT_H`.
  - The r1 hypothesis "plugin `setup` runs before the first `on_window_ready`" is source-verified
    (tauri app.rs:2607, manager/mod.rs:473-478, plugin.rs:907-916) and leaves the hypothesis list;
    the `try_state` guard stays.

## 11. Round-3 revision

- T3-I1, M10 judge false-fails at exact-integer points: R24. `Test-AspectShape -ContentH` with
  `want = floor((ClientW x ContentH + 979) / 980)`; ratio kept for humans; selftest rows C 1282 and
  C 641 at w 1960; 3.2 step 5 reworded (3.2, 7.4, 7.5, task 9).
- T3-M1, `\b` regex matched `.ring-sm`: R25. `(?![\w-])` lookahead; `.ring svg, .ring-sm svg`
  becomes `.ring-sm svg` with the css-contract selector edit in task 3 (7.1, task 3).
- T3-M2, task 3 test lists incomplete: R26. Five css-contract tests named (four deleted, one
  rewritten); `ringDash` describe re-pointed at `RING_SIZES.sm.radius` with its six rows (7.1,
  task 3).
- T3-M3, StrictMode reused a disposed controller: R27. Controller built inside the `[]` mount
  effect, ref only for the other effects, disposed by the cleanup; 7.5 note; task 8 dev check (3.1,
  7.5, task 8).
- T3-M4, M10 settings and WARN scope: R28. Per-check `-CloseToTray` (S1 `$false`; S4, S6 `$true`);
  WARN scoped to the `window_aspect`/`platform` targets via a new `Target` field; S5 in physical px
  `round(980 x s)` to `round(1470 x s)`, `s` from the new `dpi` field of "subclass installed"
  (3.3, 6, 7.4, tasks 6 and 9).
- T3-M5, `holds` on a float boundary: R29. `viewport.h * 980 >= (viewport.w - 1) * c`; row
  (1226, 1124) -> false added (3.1, 7.1, 7.5).
