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
        self.assertEqual(
            row.get("onClick"),
            "window.__bramSendMenuAnswer(answerKeys, promptId)",
        )

        number_button = row.find("Button")
        self.assertIsNotNone(number_button)
        self.assertEqual(number_button.get("label"), "{($item.key || '')}")
        self.assertEqual(number_button.get("variant"), "outlined")
        self.assertEqual(number_button.get("size"), "sm")
        self.assertIsNone(number_button.get("onClick"))

        text_values = {node.get("value") for node in row.iter("Text")}
        self.assertIn("{($item.label || '')}", text_values)
        self.assertIn("{$item.description}", text_values)


if __name__ == "__main__":
    unittest.main()
