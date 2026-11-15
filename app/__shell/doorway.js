// Bram's staged-turn doorway for project pages (doorway-stage-a-repo-prompt).
// A page served by Bram loads this file from /__shell/doorway.js and calls
//
//   BramDoorway.stage("get-me-going")
//
// to hand the user to the agent: Bram's window comes forward with the named
// prompt in its message box, unsent; the user reads it and presses Enter.
//
// The name must be declared in the project's committed .bram.json:
//
//   "prompts": { "get-me-going": { "file": "docs/get-me-going-prompt.md" } }
//
// A page can only name a prompt, never supply its words, and nothing is sent
// for the user. Only a page Bram serves on its own port may call the route
// (its loopback gate refuses every other origin).
//
// stage(name) resolves to { ok: true, prompt } and rejects with an Error whose
// `reason` is the route's: bad-name, unknown-prompt, refused-path,
// missing-file, too-large, not-utf8, empty, no-project, or "unreachable"
// when Bram didn't answer (not running, or the page isn't served by Bram).
(function (root) {
  // The route is on the server that served this script: a same-origin
  // request, so the page's own origin is what the gate checks.
  const script = typeof document !== "undefined" ? document.currentScript : null;
  const base = script && script.src ? new URL(script.src).origin : "";

  async function stage(name) {
    let res;
    try {
      res = await fetch(base + "/__doorway/stage?prompt=" + encodeURIComponent(String(name || "")), {
        method: "POST",
      });
    } catch (e) {
      const err = new Error("Bram didn't answer");
      err.reason = "unreachable";
      throw err;
    }
    let body = {};
    try {
      body = await res.json();
    } catch (e) {}
    if (!res.ok || !body.ok) {
      const err = new Error("Bram refused the prompt: " + (body.reason || res.status));
      err.reason = body.reason || (res.status === 403 ? "foreign-origin" : String(res.status));
      throw err;
    }
    return body;
  }

  root.BramDoorway = { stage };
})(typeof window !== "undefined" ? window : globalThis);
