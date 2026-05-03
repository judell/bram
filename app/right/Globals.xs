var componentCatalog = [
  {
    id: "xmlui-repos",
    label: "Repos",
    description: "Browse the raw xmlui-org repository snapshot with latest commit metadata."
  },
  {
    id: "agent-interaction",
    label: "AgentInteraction",
    description: "Placeholder for future agent interaction UI."
  }
];

var selectedComponentId = "xmlui-repos";

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
