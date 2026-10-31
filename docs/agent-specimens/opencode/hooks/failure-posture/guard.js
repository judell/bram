// Specimen guard plugin for the posture matrix. Mode and target tool come from env
// (GUARD_MODE, GUARD_TOOL) and the log path from GUARD_LOG. Same plugin shape as
// Bram's: api.tool.hook("execute.before", ...) where a thrown Error denies.
import { appendFileSync } from "node:fs";
const LOG = process.env.GUARD_LOG;
const mode = process.env.GUARD_MODE, tgt = process.env.GUARD_TOOL;
const log = (s) => appendFileSync(LOG, new Date().toISOString().slice(11,19) + " " + s + "\n");
if (mode === "load-throw") { log("module loaded, throwing at import"); throw new Error("PLUGIN-LOAD-THROW-MARK"); }
export default {
  id: "specimen.posture-guard",
  setup(api) {
    log("setup mode=" + mode);
    if (mode === "setup-throw") throw new Error("PLUGIN-SETUP-THROW-MARK");
    api.tool.hook("execute.before", async (evt) => {
      log("ran mode=" + mode + " tool=" + evt.tool);
      if (evt.tool !== tgt) return;
      log("APPLY mode=" + mode + " tool=" + evt.tool);
      switch (mode) {
        case "allow": return;
        case "deny": throw new Error("AGENTMSG-DENY-REASON-5521: blocked by guard.");
        case "throwstr": throw "THROWN-STRING-MARK";
        case "throwobj": throw { reason: "THROWN-OBJECT-MARK" };
        case "exit1": console.error("STDERR-EXIT1-MARK"); process.exit(1);
        case "hang": await new Promise((r) => setTimeout(r, Number(process.env.GUARD_HANG_MS||40000))); throw new Error("HANG-LATE-DENY");
        case "empty": return;
        case "emptyobj": return {};
        case "denyobj": return { permission: "deny", reason: "RETURNED-DENY-MARK" };
        case "oldguard": console.error("bram-guard: unknown hook execute.before"); return;
      }
    });
  },
};
