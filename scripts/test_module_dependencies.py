import unittest

from module_dependencies import overview, parse_modules, render_dot


class ModuleDependenciesTest(unittest.TestCase):
    def test_child_references_cycles_and_isolated_modules(self):
        dot = '''digraph {
"procinsh" [label="crate|procinsh"]; // "crate" node
"procinsh::http_server::process" [label="mod|process"]; // "mod" node
"procinsh::http_server::process::snapshot" [label="mod|snapshot"]; // "mod" node
"procinsh::http_server::api" [label="mod|api"]; // "mod" node
"procinsh::http_server::web" [label="mod|web"]; // "mod" node
"std::fs" [label="external mod|std::fs"]; // "mod" node
"procinsh::http_server::process" -> "procinsh::http_server::process::snapshot" [label="owns"]; // "owns" edge
"procinsh::http_server::process::snapshot" -> "procinsh::http_server::api" [label="uses"]; // "uses" edge
"procinsh::http_server::api" -> "procinsh::http_server::process::snapshot" [label="uses"]; // "uses" edge
"procinsh::http_server::api" -> "procinsh::http_server::api" [label="uses"]; // "uses" edge
"procinsh::http_server::process" -> "std::fs" [label="uses"]; // "uses" edge
}'''
        nodes, edges = parse_modules(dot)
        self.assertEqual(len(nodes), 5)
        self.assertEqual(edges, {('procinsh::http_server::process::snapshot', 'procinsh::http_server::api'),
                                 ('procinsh::http_server::api', 'procinsh::http_server::process::snapshot')})
        summary_nodes, summary_edges = overview(nodes, edges)
        self.assertEqual(summary_nodes, {'procinsh', 'procinsh::http_server::process', 'procinsh::http_server::api', 'procinsh::http_server::web'})
        self.assertEqual(summary_edges, {('procinsh::http_server::process', 'procinsh::http_server::api'),
                                         ('procinsh::http_server::api', 'procinsh::http_server::process')})
        rendered = render_dot(summary_nodes, summary_edges)
        self.assertIn('"procinsh::http_server::web" [label="http_server::web"]', rendered)
        self.assertNotIn('std::', rendered)
        nested = render_dot(nodes, edges, nested=True)
        self.assertIn('cluster_procinsh::http_server::process', nested)
        self.assertIn('label="main"', nested)

    def test_empty_or_changed_format_fails(self):
        for dot in ['', 'digraph {}', 'unrecognized; // "mod" node']:
            with self.subTest(dot=dot), self.assertRaises(ValueError):
                parse_modules(dot)

    def test_implementation_children_fold_into_subsystems(self):
        prefix = 'procinsh::http_server::'
        nodes = {prefix + name for name in (
            'server', 'state', 'router', 'middleware', 'api::router',
            'process::identity', 'process::resources',
            'process::monitoring::service', 'process::snapshot::capture',
            'process::snapshot::symbol::cache', 'process::snapshot::symbol::resolve',
            'system_monitoring::service', 'system_monitoring::status',
        )}
        edges = {(prefix + a, prefix + b) for a, b in (
            ('router', 'api::router'),
            ('api::router', 'process::resources'),
            ('process::resources', 'process::identity'),
            ('process::snapshot::capture', 'process::snapshot::symbol::cache'),
            ('system_monitoring::service', 'process::resources'),
        )}
        summary_nodes, summary_edges = overview(nodes, edges)
        self.assertEqual(summary_nodes, {prefix + name for name in (
            'server', 'state', 'router', 'middleware', 'api', 'process', 'system_monitoring',
        )})
        self.assertEqual(summary_edges, {(prefix + a, prefix + b) for a, b in (
            ('router', 'api'), ('api', 'process'), ('system_monitoring', 'process'),
        )})
        nested = render_dot(nodes, edges, nested=True)
        self.assertIn('cluster_' + prefix + 'process::snapshot::symbol', nested)
        self.assertIn('label="resolve"', nested)


if __name__ == '__main__':
    unittest.main()
