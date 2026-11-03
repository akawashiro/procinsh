#!/usr/bin/env python3
"""Check module-boundary projection and Graphviz rendering of dependency edges."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / 'scripts'))
from module_dependencies import module_boundaries, overview, parse_modules, render_dot


class ModuleBoundaryTests(unittest.TestCase):
    def test_external_consumer_stops_at_first_module_boundary(self):
        self.assertEqual(module_boundaries({
            ('app::snapshot::unwind', 'app::snapshot::symbol::cache'),
            ('app::server', 'app::analysis::snapshot::symbol::cache'),
        }), {
            ('app::snapshot::unwind', 'app::snapshot::symbol'),
            ('app::server', 'app::analysis'),
        })

    def test_internal_dependencies_remain_within_their_parent(self):
        self.assertEqual(module_boundaries({
            ('app::symbol::cache', 'app::symbol::image'),
            ('app::symbol::cache', 'app::symbol::resolve::inner'),
        }), {
            ('app::symbol::cache', 'app::symbol::image'),
            ('app::symbol::cache', 'app::symbol::resolve'),
        })

    def test_parent_to_descendant_stops_at_immediate_child(self):
        self.assertEqual(module_boundaries({
            ('app::symbol', 'app::symbol::cache::reader'),
            ('app', 'app::server::router'),
        }), {
            ('app::symbol', 'app::symbol::cache'),
            ('app', 'app::server'),
        })

    def test_ancestors_are_preserved_and_self_edges_removed(self):
        self.assertEqual(module_boundaries({
            ('app::symbol::cache', 'app::symbol'),
            ('app::symbol::cache', 'app'),
            ('app::symbol', 'app::symbol'),
        }), {
            ('app::symbol::cache', 'app::symbol'),
            ('app::symbol::cache', 'app'),
        })

    def test_projection_deduplicates_edges_and_does_not_mutate_raw_input(self):
        raw = {
            ('app::user', 'app::api::one'),
            ('app::user', 'app::api::two'),
            ('app::user', 'app::api'),
        }
        self.assertEqual(module_boundaries(raw), {('app::user', 'app::api')})
        self.assertEqual(len(raw), 3)

    def test_pinned_dot_parser_keeps_only_internal_module_uses(self):
        raw = '''digraph {
            "app" [label="app"]; // "crate" node
            "app::user" [label="user"]; // "mod" node
            "app::api" [label="api"]; // "mod" node
            "app::api::inner" [label="inner"]; // "mod" node
            "app::api::inner::Value" [label="Value"]; // "struct" node
            "std::path" [label="path"]; // "mod" node
            "app::user" -> "app::api::inner" []; // "uses" edge
            "app::api" -> "app::api::inner" []; // "owns" edge
            "app::user" -> "std::path" []; // "uses" edge
        }'''
        nodes, edges = parse_modules(raw)
        self.assertEqual(nodes, {'app', 'app::user', 'app::api', 'app::api::inner'})
        self.assertEqual(edges, {('app::user', 'app::api::inner')})
        self.assertEqual(module_boundaries(edges), {('app::user', 'app::api')})

    def test_overview_aggregates_the_projected_graph(self):
        nodes = {'app', 'app::server', 'app::server::user',
                 'app::ipc', 'app::ipc::schema', 'app::ipc::schema::inner'}
        edges = module_boundaries({('app::server::user', 'app::ipc::schema::inner')})
        _, edges = overview(nodes, edges)
        self.assertEqual(edges, {('app::server::user', 'app::ipc')})

    def test_nested_svg_targets_container_outline_without_clipping_ancestor_edges(self):
        nodes = {'app', 'app::user', 'app::api', 'app::api::inner'}
        edges = {('app::user', 'app::api'), ('app::api::inner', 'app::api')}
        projected = render_dot(nodes, edges, nested=True, cluster_targets=True)
        self.assertIn('[lhead="cluster_app::api"]', projected)
        self.assertIn('"app::api::inner" -> "app::api";', projected)
        raw = render_dot(nodes, {('app::user', 'app::api::inner')}, nested=True)
        self.assertNotIn('lhead=', raw)
        with tempfile.TemporaryDirectory() as directory:
            for name, dot in [('boundary', projected), ('raw', raw)]:
                path = Path(directory) / f'{name}.svg'
                result = subprocess.run(['dot', '-Tsvg', '-o', str(path)],
                                        input=dot, text=True, capture_output=True, check=True)
                self.assertEqual(result.stderr, '')
                svg = ET.parse(path)
                titles = {element.text for element in svg.iter('{http://www.w3.org/2000/svg}title')}
                self.assertIn('app::user->app::api' if name == 'boundary'
                              else 'app::user->app::api::inner', titles)


if __name__ == '__main__':
    unittest.main()
