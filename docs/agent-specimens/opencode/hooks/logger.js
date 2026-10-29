// Specimen logger: writes the events and tool hooks Bram's plugin uses, one JSON per type (first of each).
// Also saves session.tool.failed / session.step.failed, which Bram does not forward but which carry the blocked-tool errors.
// Set SPECIMEN_EVENTS_DIR; the default below is the redacted path.
import { mkdirSync, writeFileSync, appendFileSync } from "node:fs";
const OUT = process.env.SPECIMEN_EVENTS_DIR || "<workspace>/events";
const FORWARDED = new Set(["session.created","session.execution.started","session.inbox.delivered","session.step.ended","session.execution.succeeded","session.execution.failed","session.execution.interrupted","permission.asked","permission.replied","form.created","form.replied","form.cancelled","session.tool.failed","session.step.failed"]);
const seen = new Set();
function dump(name, obj) {
  mkdirSync(OUT, { recursive: true });
  appendFileSync(OUT + "/all.log", name + " " + JSON.stringify(obj).slice(0, 200) + "\n");
  if (seen.has(name)) return; seen.add(name);
  writeFileSync(OUT + "/" + name + ".json", JSON.stringify(obj, null, 2));
}
export default {
  id: "specimen.logger",
  setup(api) {
    api.tool.hook("execute.before", async (evt) => { dump("tool.execute.before", evt); });
    api.tool.hook("execute.after", async (evt) => { dump("tool.execute.after", evt); });
    (async () => { for await (const evt of api.event.subscribe()) { if (FORWARDED.has(evt.type)) dump(evt.type, evt); else appendFileSync(OUT + "/all.log", "other " + evt.type + "\n"); } })().catch(() => {});
  },
};
