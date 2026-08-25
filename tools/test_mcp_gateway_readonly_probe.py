from __future__ import annotations

import unittest

from tools import mcp_gateway_readonly_probe as probe


class GatewayReadonlyProbeTests(unittest.TestCase):
    def test_lifecycle_calls_are_model_free_and_preserve_presence_pairing(self) -> None:
        calls = probe.model_free_calls("release-operator")
        names = [name for name, _ in calls]

        self.assertEqual(
            names,
            ["agent_heartbeat", "list_memories", "agent_farewell"],
        )
        self.assertNotIn("search_memory", names)
        self.assertEqual(calls[0][1]["agent_id"], calls[2][1]["agent_id"])
        self.assertEqual(calls[1][1]["actor_id"], "release-operator")
        self.assertEqual(calls[1][1]["user_id"], "release-operator")
        self.assertEqual(calls[1][1]["limit"], 1)


if __name__ == "__main__":
    unittest.main()
