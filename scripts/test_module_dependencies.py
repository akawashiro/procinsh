import unittest

from module_dependencies import overview, parse_modules, render_dot


class ModuleDependenciesTest(unittest.TestCase):
    def test_child_references_cycles_and_isolated_modules(self):
        dot = '''digraph {
"procinsh" [label="crate|procinsh"]; // "crate" node
"procinsh::snapshot" [label="pub mod|snapshot"]; // "mod" node
"procinsh::snapshot::capture" [label="pub mod|capture"]; // "mod" node
"procinsh::symbol" [label="pub mod|symbol"]; // "mod" node
"procinsh::state" [label="pub mod|state"]; // "mod" node
"std::fs" [label="external mod|std::fs"]; // "mod" node
"procinsh::snapshot" -> "procinsh::snapshot::capture" [label="owns"]; // "owns" edge
"procinsh::snapshot::capture" -> "procinsh::symbol" [label="uses"]; // "uses" edge
"procinsh::symbol" -> "procinsh::snapshot::capture" [label="uses"]; // "uses" edge
"procinsh::symbol" -> "procinsh::symbol" [label="uses"]; // "uses" edge
"procinsh::snapshot" -> "std::fs" [label="uses"]; // "uses" edge
}'''
        nodes, edges = parse_modules(dot)
        self.assertEqual(len(nodes), 4)
        self.assertEqual(edges, {('procinsh::snapshot::capture', 'procinsh::symbol'),
                                 ('procinsh::symbol', 'procinsh::snapshot::capture')})
        summary_nodes, summary_edges = overview(nodes, edges)
        self.assertEqual(summary_nodes, {'procinsh::snapshot', 'procinsh::symbol', 'procinsh::state'})
        self.assertEqual(summary_edges, {('procinsh::snapshot', 'procinsh::symbol'),
                                         ('procinsh::symbol', 'procinsh::snapshot')})
        rendered = render_dot(summary_nodes, summary_edges)
        self.assertIn('"procinsh::state" [label="state"]', rendered)
        self.assertNotIn('std::', rendered)

    def test_empty_or_changed_format_fails(self):
        for dot in ['', 'digraph {}', 'unrecognized; // "mod" node']:
            with self.subTest(dot=dot), self.assertRaises(ValueError):
                parse_modules(dot)


if __name__ == '__main__':
    unittest.main()
