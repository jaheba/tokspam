import ast
import unittest

from tools.calibrate import count_tokens, source_metrics


class SourceMetricsTest(unittest.TestCase):
    def test_collects_widths_without_recursive_traversal(self):
        source = """
def f(a, b, *args, c, **kwargs):
    value = call(a, b, c=1)
    items = [1, 2, 3, 4]
    sliced = value[1:2, 3:4, 5:6]
    rendered = f"{a}{b}{c}"
    function = lambda x, y: x
"""
        metrics = source_metrics(ast.parse(source))
        self.assertEqual(metrics["max_call_arguments"], 3)
        self.assertEqual(metrics["max_parameters"], 5)
        self.assertEqual(metrics["max_display_items"], 4)
        self.assertEqual(metrics["max_slice_items"], 3)
        self.assertEqual(metrics["max_interpolation_fields"], 3)
        self.assertGreaterEqual(metrics["identifier_count"], 10)
        self.assertGreater(metrics["identifier_occurrences"], metrics["identifier_count"])
        self.assertGreater(metrics["ast_nodes"], metrics["statement_count"])
        self.assertGreater(count_tokens(source), 20)


if __name__ == "__main__":
    unittest.main()
