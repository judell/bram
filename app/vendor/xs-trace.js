/**
 * App-level tracing for XMLUI inspector traces.
 *
 * Include this script in any XMLUI app:
 *   <script src="trace-tools/xs-trace.js"></script>
 *
 * Timing — wrap expensive calls:
 *   window.xsTrace("filterEvents", () => filterEvents(events, term))
 *   Emits "app:trace" with label + duration.
 *
 * Semantic events — emit app-level facts:
 *   window.xsTraceEvent("search", { term: "jazz", resultCount: 42 })
 *   window.xsTraceEvent("filter", { source: "North Bay Bohemian", action: "hide" })
 *   Emits "app:trace" with label + data payload (no duration).
 *
 * Combined — time a function and attach semantic data:
 *   window.xsTraceWith("filterEvents", () => filterEvents(events, term),
 *     (result) => ({ term: term, resultCount: result.length }))
 *   Emits "app:trace" with label, duration, and data payload.
 *
 * All three emit kind "app:trace". Duration and data are optional fields.
 * "app:" means the event comes from plain JS, not the XMLUI engine.
 *
 * Entries appear in the xs-diff.html inspector timeline alongside
 * engine-generated handler:start/complete and api:start/complete entries.
 */
(function () {
  function getTraceId() {
    var logs = window._xsLogs;
    return logs && logs.length > 0 ? logs[logs.length - 1].traceId : undefined;
  }

  window.xsTrace = function (label, fn) {
    var logs = window._xsLogs;
    if (!logs) return fn();
    var start = performance.now();
    var result = fn();
    var duration = performance.now() - start;
    logs.push({
      kind: "app:trace",
      label: label,
      traceId: getTraceId(),
      perfTs: start,
      duration: duration,
    });
    return result;
  };

  window.xsTraceEvent = function (label, data) {
    var logs = window._xsLogs;
    if (!logs) return;
    logs.push({
      kind: "app:trace",
      label: label,
      data: data,
      traceId: getTraceId(),
      perfTs: performance.now(),
    });
  };

  window.xsTraceWith = function (label, fn, extractData) {
    var logs = window._xsLogs;
    if (!logs) return fn();
    var start = performance.now();
    var result = fn();
    var duration = performance.now() - start;
    var data = typeof extractData === "function" ? extractData(result) : undefined;
    logs.push({
      kind: "app:trace",
      label: label,
      data: data,
      traceId: getTraceId(),
      perfTs: start,
      duration: duration,
    });
    return result;
  };
})();
