// bram-managed: Bram Setup seeds this file as .opencode/plugins/bram.js and
// refreshes it on every Setup, so edits to the installed copy don't last.
// Source: app/__shell/opencode/bram-plugin.js in the Bram repo.
//
// opencode-worklist-guard: OpenCode's counterpart of the PreToolUse hook Bram
// registers for Claude and Codex. Before each tool call that can change the
// project, it asks Bram's guard (~/.bram/bram-guard guard opencode-worklist)
// and throws when the answer is not an allow; a throwing execute.before
// refuses the call and the model reads the message. It fails closed: no
// guard, no answer, or an unreadable answer is a deny.
//
// opencode-turn-lifecycle: it also forwards this project's turn boundaries
// from OpenCode's event bus to Bram (POST /__opencode/event), since OpenCode
// writes no transcript file Bram can watch for them.
import { spawn } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, sep } from "node:path";
import { fileURLToPath } from "node:url";

// <root>/.opencode/plugins/bram.js. Plugins run inside OpenCode's background
// service, whose cwd and environment belong to whatever started it, so the
// project root comes from this file's own location (symlinks resolved, as
// OpenCode reports session directories).
const ROOT = (() => {
  const root = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
  try { return realpathSync(root); } catch { return root; }
})();
const GUARD = join(
  homedir(),
  ".bram",
  process.platform === "win32" ? "bram-guard.exe" : "bram-guard",
);
const GUARD_TIMEOUT_MS = 15000;
// Built-in tools that can't change the project. Everything else, MCP tools
// included, goes to the guard, which allows what it has no rule for.
const READ_ONLY_TOOLS = new Set([
  "read",
  "glob",
  "grep",
  "list",
  "webfetch",
  "websearch",
  "todowrite",
  "todoread",
  "question",
  "skill",
  "lsp",
]);

function deny(message) {
  return { allow: false, message };
}

// A committed copy of this file reaches clones where Bram was never set up.
// There, with no guard and no Bram marker, it stays out of the way, as the
// guard itself does in an unmanaged repo. With the marker present a missing
// guard is a broken install, and that denies.
function bramManagedHere() {
  return existsSync(join(ROOT, "resources", ".worklist-authorization.json"));
}

function askGuard(evt) {
  if (!existsSync(GUARD) && !bramManagedHere()) {
    return Promise.resolve({ allow: true, message: "" });
  }
  const payload = JSON.stringify({
    tool: evt.tool,
    input: evt.input,
    sessionID: evt.sessionID,
    agent: evt.agent,
    cwd: ROOT,
  });
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(GUARD, ["guard", "opencode-worklist"], {
        cwd: ROOT,
        stdio: ["pipe", "pipe", "pipe"],
        timeout: GUARD_TIMEOUT_MS,
      });
    } catch (e) {
      resolve(deny(`Blocked: Bram's worklist guard could not start (${e.message}). Run Setup in Bram, then retry.`));
      return;
    }
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => { out += d; });
    child.stderr.on("data", (d) => { err += d; });
    child.on("error", (e) => {
      resolve(deny(`Blocked: Bram's worklist guard could not run (${e.message}). Run Setup in Bram, then retry.`));
    });
    child.on("close", () => {
      const line = out.trim().split("\n").pop() || "";
      let answer = null;
      try { answer = JSON.parse(line); } catch {}
      if (!answer || (answer.decision !== "allow" && answer.decision !== "deny")) {
        const detail = err.trim().slice(0, 300);
        resolve(deny(
          "Blocked: Bram's worklist guard gave no decision, so the call is denied by default. " +
          "This is a guard or install problem, not a policy decision; run Setup in Bram." +
          (detail ? ` Guard said: ${detail}` : ""),
        ));
        return;
      }
      resolve({ allow: answer.decision === "allow", message: answer.message || "Blocked by Bram's worklist guard." });
    });
    child.stdin.on("error", () => {});
    child.stdin.end(payload);
  });
}

// --- turn boundaries ---------------------------------------------------------

const FORWARDED = new Set([
  "session.execution.started",
  "session.inbox.delivered",
  // opencode-sessions-browser: each completed step adds to the transcript
  // Bram shows, so it refreshes as the turn goes rather than only at its end.
  "session.step.ended",
  "session.execution.succeeded",
  "session.execution.failed",
  "session.execution.interrupted",
]);

// Read per event: Bram writes a new port each time it starts.
function bramPort() {
  try {
    return Number(readFileSync(join(ROOT, "resources", ".bram-port"), "utf8").trim()) || 0;
  } catch {
    return 0;
  }
}

async function postEvent(body) {
  const port = bramPort();
  if (!port) return;
  try {
    await fetch(`http://127.0.0.1:${port}/__opencode/event`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(2000),
    });
  } catch {
    // Bram is down or restarting; a missed boundary is recovered by the next.
  }
}

function inProject(dir) {
  return typeof dir === "string" && (dir === ROOT || dir.startsWith(ROOT + sep));
}

// Only this project's top-level sessions count. A subagent runs as a child
// session (session.created with a parentID) with executions of its own
// inside the parent's turn, and the bus may carry other projects' sessions.
// Execution events carry no directory; a session's created and enqueued
// events do, and come first.
async function forwardTurns(api) {
  const ours = new Set();
  const children = new Set();
  let chain = Promise.resolve();
  for await (const evt of api.event.subscribe()) {
    const data = (evt && evt.data) || {};
    const sid = data.sessionID;
    if (!sid) continue;
    if (evt.type === "session.created" && data.parentID) children.add(sid);
    const dir = (evt.location && evt.location.directory) || (data.location && data.location.directory);
    if (inProject(dir)) ours.add(sid);
    if (!FORWARDED.has(evt.type) || !ours.has(sid) || children.has(sid)) continue;
    // Chained so the host sees boundaries in order; never awaited here, so
    // a slow or absent Bram can't hold up OpenCode's event stream.
    const body = { type: evt.type, sessionID: sid, at: evt.created };
    chain = chain.then(() => postEvent(body));
  }
}

export default {
  id: "bram.worklist-guard",
  setup(api) {
    api.tool.hook("execute.before", async (evt) => {
      if (READ_ONLY_TOOLS.has(evt.tool)) return;
      const verdict = await askGuard(evt);
      if (!verdict.allow) throw new Error(verdict.message);
    });
    forwardTurns(api).catch(() => {});
  },
};
