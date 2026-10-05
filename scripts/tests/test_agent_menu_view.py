import unittest
import xml.etree.ElementTree as ET
from pathlib import Path


VIEW = (
    Path(__file__).resolve().parents[2]
    / "app"
    / "tools"
    / "components"
    / "AgentMenuView.xmlui"
)


class AgentMenuViewTests(unittest.TestCase):
    def test_option_row_is_clickable_without_changing_its_layout(self):
        root = ET.parse(VIEW).getroot()
        items = next(
            node
            for node in root.iter("Items")
            if node.get("data") == "{$props.menu.options}"
        )

        self.assertEqual([child.tag for child in items], ["HStack"])
        row = items[0]
        self.assertEqual(row.get("width"), "100%")
        # fe84674: every row but Claude's "Type something." row still
        # answers through __bramSendMenuAnswer (inside __bramMenuRowClick);
        # that row opens a text box instead.
        self.assertEqual(
            row.get("onClick"),
            "typeSomethingFor = window.__bramMenuRowClick($props.menu, $item, answerKeys, promptId, typeSomethingFor)",
        )

        number_button = row.find("Button")
        self.assertIsNotNone(number_button)
        self.assertEqual(number_button.get("label"), "{($item.key || '')}")
        # outlined, or solid for the chosen "Type something." row while its
        # box is open (fe84674).
        self.assertEqual(
            number_button.get("variant"),
            "{window.__bramMenuRowVariant($props.menu, $item, typeSomethingFor, promptId)}",
        )
        self.assertEqual(number_button.get("size"), "sm")
        self.assertIsNone(number_button.get("onClick"))

        text_values = {node.get("value") for node in row.iter("Text")}
        self.assertIn("{($item.label || '')}", text_values)
        self.assertIn("{$item.description}", text_values)


if __name__ == "__main__":
    unittest.main()
