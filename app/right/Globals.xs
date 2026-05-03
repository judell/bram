var componentCatalog = [
  {
    id: "agent-echo",
    label: "AgentEcho",
    description: "Show the full agent transcript."
  },
  {
    id: "xmlui-repos",
    label: "XmluiRepos",
    description: "Browse the raw xmlui-org repository snapshot with latest commit metadata."
  }
];

var selectedComponentId = "agent-echo";
var selectedSessionFile = "live/AgentEcho.md";
var selectedSessionTitle = "Current session";
var selectedSessionCreatedAt = "In progress";
var selectedSessionNote = "Live AgentEcho transcript for the active session.";

function getSelectedComponent() {
  return componentCatalog.find(component => component.id === selectedComponentId) || componentCatalog[0];
}

function selectComponent(id) {
  selectedComponentId = id;
}

function setSelectedComponent(items, tabs) {
  if (items && items.length > 0) {
    if (items[0].id === selectedComponentId) {
      return;
    }
    selectedComponentId = items[0].id;
    if (tabs) {
      tabs.setActiveTabById("selected-component");
    }
  }
}

function buildSessionRows(catalog) {
  const rows = [
    {
      id: "live-agent-echo",
      filename: "Current session",
      path: "live/AgentEcho.md",
      createdAt: "In progress",
      note: "Live AgentEcho transcript for the active session."
    }
  ];

  if (!Array.isArray(catalog)) {
    return rows;
  }

  return rows.concat(
    catalog
      .slice()
      .sort((a, b) => String(b.createdAt || "").localeCompare(String(a.createdAt || "")))
      .map((item, index) => ({
        id: item.id || item.path || `session-${index}`,
        ...item
      }))
  );
}

function openSessionArchive(item) {
  if (!item) {
    return;
  }
  selectedSessionFile = item.path;
  selectedSessionTitle = item.filename;
  selectedSessionCreatedAt = item.createdAt || "";
  selectedSessionNote = item.note || "";
}

function scrollSessionTranscriptToBottom() {
  if (selectedSessionFile !== "live/AgentEcho.md") {
    return;
  }

  delay(0);

  const root = document.getElementById("session-transcript-scroller");
  if (!root) {
    return;
  }

  const viewport =
    root.querySelector("[data-overlayscrollbars-viewport]") ||
    root.querySelector(".os-viewport") ||
    root;

  try {
    viewport.scrollTo({ top: viewport.scrollHeight, behavior: "smooth" });
  } catch (e) {
    viewport.scrollTop = viewport.scrollHeight;
  }
}

function maybeAutoScrollSessionTranscript(data, isRefetch) {
  if (selectedSessionFile !== "live/AgentEcho.md" || !isRefetch) {
    return;
  }
  scrollSessionTranscriptToBottom();
}
