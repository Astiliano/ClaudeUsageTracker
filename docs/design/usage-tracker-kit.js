/* ============================================================================
   Usage Tracker Kit — the data/config layer for "Usage Tracker.dc.html".
   Plain classic script; attaches window.UsageTrackerKit. No build step.

   WHAT LIVES HERE (change these, not the component):
     THEME       data colors (meter/line thresholds)
     THRESHOLDS  when a value counts as warn / critical
     FONTS,SIZES the Settings > Typeface / Text size options
     COLUMNS     the table's column registry — label, width, kind, accessors
     DEFAULT_ORDER  initial left-to-right column order
     series/format  pure helpers (line segments, per-day dots, stats, labels)
     SAMPLE_ACCOUNTS  demo data; swap for real polling output

   HOW TO EXTEND
     · New column of an existing kind → add one COLUMNS entry + its key to
       DEFAULT_ORDER. No template edit.
         COLUMNS.cost = { label:'Cost', width:'minmax(86px,1.2fr)', kind:'meter',
                          value:a=>a.costPct, note:a=>a.costLabel }
     · New column kind → add a `case` in buildCell below plus one <sc-if>
       branch in the component template (keyed on the flag you set).
     · New typeface / text size → one FONTS or SIZES entry.
     · Different thresholds or palette → THEME / THRESHOLDS.
   ========================================================================== */
(function (root) {
  'use strict';

  var THEME = {
    ok: '#7aa2f7',
    warn: '#d8a94f',
    crit: '#e0705f',
    dot: '#e9eef3',
    dotRing: '#0f1416',
    live: '#5fb98a',
    idle: '#4b5359',
    meta: '#8b949e',
    metaWarn: '#d08c80'
  };

  var THRESHOLDS = { warn: 70, crit: 95 };

  var FONTS = {
    system: { label: 'System', ui: "ui-sans-serif, 'Segoe UI', Helvetica, Arial, sans-serif", mono: "ui-monospace, 'Cascadia Mono', Consolas, 'SF Mono', monospace" },
    plex: { label: 'IBM Plex', ui: "'IBM Plex Sans', Helvetica, sans-serif", mono: "'IBM Plex Mono', monospace" },
    jetbrains: { label: 'JetBrains', ui: "ui-sans-serif, 'Segoe UI', Helvetica, sans-serif", mono: "'JetBrains Mono', monospace" }
  };

  var SIZES = {
    sm: { label: 'small', zoom: 0.92 },
    md: { label: 'default', zoom: 1 },
    lg: { label: 'large', zoom: 1.1 },
    xl: { label: 'largest', zoom: 1.22 }
  };

  function clamp(v, lo, hi) { return Math.max(lo, Math.min(hi, v)); }
  function isNum(v) { return v !== null && v !== undefined && !isNaN(v); }

  /* value → color, shared by meters, sparklines and the 30-day line */
  function metricColor(v) {
    if (v >= THRESHOLDS.crit) return THEME.crit;
    if (v >= THRESHOLDS.warn) return THEME.warn;
    return THEME.ok;
  }

  var format = {
    pct: function (v) { return (isNum(v) ? Math.round(v) : 0) + '%'; },
    /* label for point `i` of a series whose last point is today */
    dayLabel: function (i, total, now) {
      var d = now ? new Date(now) : new Date();
      d.setDate(d.getDate() - (total - 1 - i));
      return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
    },
    nextReset: function (a) {
      var day = a.resetDay || 'Monday', time = a.resetTime || '00:00', tz = a.resetTz || 'local';
      return day.slice(0, 3) + ' ' + time + (tz === 'UTC' ? ' UTC' : '');
    }
  };

  var series = {
    /* polyline point runs; nulls are skipped so the line stays connected */
    segments: function (vals, w, h) {
      var pts = [], n = vals.length;
      vals.forEach(function (v, i) {
        if (!isNum(v)) return;
        var x = n === 1 ? 0 : (i / (n - 1)) * w;
        var y = h - (clamp(v, 0, 100) / 100) * h;
        pts.push(x.toFixed(2) + ',' + y.toFixed(2));
      });
      return pts.length > 1 ? [{ line: pts.join(' ') }] : [];
    },
    /* one marker per day WITH data — a missing dot means no sample that day */
    dots: function (vals) {
      var n = vals.length, out = [];
      vals.forEach(function (v, i) {
        if (!isNum(v)) return;
        out.push({ index: i, value: v, leftPct: n === 1 ? 0 : (i / (n - 1)) * 100 });
      });
      return out;
    },
    stats: function (vals) {
      var known = vals.filter(isNum);
      return {
        peak: known.length ? Math.max.apply(null, known) : 0,
        avg: known.length ? Math.round(known.reduce(function (x, y) { return x + y; }, 0) / known.length) : 0,
        missing: vals.length - known.length
      };
    },
    /* evenly spaced x-axis tick labels */
    axis: function (total, count) {
      var ticks = [], c = count || 4;
      for (var i = 0; i < c; i++) ticks.push(format.dayLabel(Math.round(i * (total - 1) / (c - 1)), total));
      return ticks;
    }
  };

  /* ---- column registry -------------------------------------------------- */
  var COLUMNS = {
    account: { label: 'Account', width: 'minmax(150px,1fr)', kind: 'account' },
    session: {
      label: 'Session', width: 'minmax(86px,1.5fr)', kind: 'meter',
      value: function (a) { return a.session; },
      note: function (a) { return a.resets || 'idle'; }
    },
    week: {
      label: 'Week (all)', width: 'minmax(86px,1.5fr)', kind: 'meter',
      value: function (a) { return a.week; },
      note: function (a) { return a.week >= THRESHOLDS.crit ? 'at limit' : 'all models'; },
      noteColor: function (a) { return a.week >= THRESHOLDS.crit ? THEME.metaWarn : THEME.meta; }
    },
    model: {
      label: 'Per model', width: 'minmax(86px,1.5fr)', kind: 'meter',
      value: function (a) { return parseInt(String(a.model || '').replace(/[^0-9]/g, ''), 10) || 0; },
      note: function (a) { return String(a.model || '').split(' ')[0] || 'model'; }
    },
    spark: { label: '7 days', width: '76px', kind: 'spark', points: 7 },
    updated: {
      label: 'Updated', width: '82px', kind: 'text',
      value: function (a) { return a.updated; }
    }
  };

  var DEFAULT_ORDER = ['account', 'session', 'week', 'model', 'spark', 'updated'];

  /* account + column def + row context → a cell object the template renders.
     Flags (isAccount/isMeter/isSpark/isUpdated) pair with template <sc-if>s. */
  function buildCell(key, a, ctx) {
    var def = COLUMNS[key] || {};
    var base = { key: key, isAccount: false, isMeter: false, isSpark: false, isUpdated: false, bg: ctx.bg };
    switch (def.kind) {
      case 'account':
        return Object.assign(base, {
          isAccount: true,
          name: a.name,
          isDefault: !!a.isDefault,
          nameColor: a.enabled ? '#eef1f3' : '#79828a',
          dotColor: a.enabled ? (a.week >= THRESHOLDS.crit ? THEME.crit : THEME.live) : THEME.idle
        });
      case 'meter': {
        var v = clamp(def.value(a) || 0, 0, 100);
        return Object.assign(base, {
          isMeter: true,
          barColor: (def.color || metricColor)(v, a),
          barW: v + '%',
          pct: format.pct(v),
          note: def.note ? def.note(a) : '',
          noteColor: def.noteColor ? def.noteColor(a) : THEME.meta
        });
      }
      case 'spark':
        return Object.assign(base, {
          isSpark: true,
          segments: series.segments(ctx.days.slice(-(def.points || 7)), 100, 24),
          stroke: ctx.stroke,
          sparkBg: ctx.chartOpen ? '#161b1f' : 'transparent',
          sparkBorder: ctx.chartOpen ? '#33404b' : '#1c2125',
          onChart: ctx.onChart
        });
      default:
        return Object.assign(base, { isUpdated: true, updated: def.value ? def.value(a) : '' });
    }
  }

  var SAMPLE_ACCOUNTS = [
    { id: 'a1', name: 'claude3', isDefault: true, enabled: true, session: 40, resets: '2h 3m left', week: 46, model: 'Fable 47%', updated: '59 s ago', resetDay: 'Wednesday', resetTime: '09:00', resetTz: 'local',
      days: [12, 30, 18, 25, 44, 60, 38, 22, null, null, 55, 62, 48, 33, 27, 19, 36, 52, 64, 58, 40, 24, 16, 28, 22, 41, 18, 60, 35, 46] },
    { id: 'a2', name: 'claude', isDefault: false, enabled: true, session: 0, resets: 'idle', week: 100, model: 'Fable 68%', updated: '56 s ago', resetDay: 'Monday', resetTime: '00:00', resetTz: 'UTC',
      days: [48, 62, 71, 66, 80, 92, 88, 74, 69, 83, 95, 100, 97, 85, 78, 90, 99, 100, 94, 88, 76, 82, 91, 100, 80, 95, 70, 100, 100, 100] },
    { id: 'a3', name: 'claude2', isDefault: false, enabled: true, session: 0, resets: 'idle', week: 98, model: 'Fable 99%', updated: '53 s ago', resetDay: 'Friday', resetTime: '18:30', resetTz: 'local',
      days: [30, 44, 52, 61, 58, 66, 74, 70, 63, 77, 85, 80, 72, null, 79, 88, 92, 86, 75, 81, 90, 96, 93, 87, 55, 72, 88, 96, 90, 98] }
  ];

  var WEEKDAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
  var TIMEZONES = ['local', 'UTC'];

  /* generic list reorder used by both row drag and column drag */
  function reorder(list, from, to) {
    var out = list.slice(), moved = out.splice(from, 1)[0];
    out.splice(to, 0, moved);
    return out;
  }

  root.UsageTrackerKit = {
    THEME: THEME, THRESHOLDS: THRESHOLDS, FONTS: FONTS, SIZES: SIZES,
    COLUMNS: COLUMNS, DEFAULT_ORDER: DEFAULT_ORDER,
    SAMPLE_ACCOUNTS: SAMPLE_ACCOUNTS, WEEKDAYS: WEEKDAYS, TIMEZONES: TIMEZONES,
    metricColor: metricColor, buildCell: buildCell,
    series: series, format: format, reorder: reorder, clamp: clamp,
    registerColumn: function (key, def) { COLUMNS[key] = def; return this; },
    gridTemplate: function (order, lead, trail) {
      return (lead || '26px') + ' ' + order.map(function (k) { return (COLUMNS[k] || {}).width || '1fr'; }).join(' ') + ' ' + (trail || '62px');
    }
  };
})(window);
