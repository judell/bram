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
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// <root>/.opencode/plugins/bram.js. Plugins run inside OpenCode's background
// service, whose cwd and environment belong to whatever started it, so the
// project root comes from this file's own location.
const ROOT = dirname(dirname(dirname(fileURLToPath(import.meta.url))));
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

export default {
  id: "bram.worklist-guard",
  setup(api) {
    api.tool.hook("execute.before", async (evt) => {
      if (READ_ONLY_TOOLS.has(evt.tool)) return;
      const verdict = await askGuard(evt);
      if (!verdict.allow) throw new Error(verdict.message);
    });
  },
};
