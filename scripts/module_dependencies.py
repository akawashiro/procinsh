#!/usr/bin/env python3
"""Render cargo-modules 0.27.0 DOT output (module nodes, uses edges only)."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess


# cargo-modules' pinned printer emits one node/edge per line, with kind comments.
NODE = re.compile(r'^\s*"([^"]+)" \[.*; // "(\w+)" node$')
EDGE = re.compile(r'^\s*"([^"]+)" -> "([^"]+)" \[.*; // "(\w+)" edge$')


def parse_modules(dot):
    nodes, edges = set(), set()
    for line in dot.splitlines():
        if match := NODE.match(line):
            name, kind = match.groups()
            if (kind == 'mod' and name.startswith('procinsh::')) or (kind == 'crate' and name == 'procinsh'):
                nodes.add(name)
        elif match := EDGE.match(line):
            source, target, kind = match.groups()
            if kind == 'uses':
                edges.add((source, target))
        elif '// "' in line and line.rstrip().endswith((' node', ' edge')):
            raise ValueError(f'Unexpected cargo-modules DOT format: {line}')
    edges = {(a, b) for a, b in edges if a in nodes and b in nodes and a != b}
    if not nodes or not edges:
        raise ValueError('Expected internal module nodes and dependency edges')
    return nodes, edges


def overview(nodes, edges):
    def top(name):
        # Summarize at the HTTP subsystem boundary, below the single root module.
        return '::'.join(name.split('::')[:3])
    return {top(n) for n in nodes}, {(top(a), top(b)) for a, b in edges if top(a) != top(b)}


def render_dot(nodes, edges, *, nested=False):
    lines = ['digraph {', '  graph [rankdir=LR, bgcolor="white", pad=0.3];',
             '  node [shape=box, style="rounded,filled", fillcolor="#e8f2ff", fontname="sans-serif"];',
             '  edge [color="#475569", arrowsize=0.8];']
    if nested:
        # Include ancestors as containers even if the input omits their nodes.
        children = {}
        for node in sorted(nodes):
            parts = node.split('::')
            for depth in range(2, len(parts) + 1):
                parent = '::'.join(parts[:depth - 1])
                child = '::'.join(parts[:depth])
                children.setdefault(parent, set()).add(child)

        def emit(node, indent):
            label = node.rsplit('::', 1)[-1]
            if node in children:
                lines.append(f'{indent}subgraph {json.dumps("cluster_" + node)} {{')
                lines.append(f'{indent}  graph [label={json.dumps(label)}, '
                             'fontname="sans-serif", color="#94a3b8", '
                             'style="rounded", margin=16, labeljust=l];')
                if node in nodes:
                    lines.append(f'{indent}  {json.dumps(node)} [label="(module)", '
                                 f'tooltip={json.dumps(node)}];')
                for child in sorted(children[node]):
                    emit(child, indent + '  ')
                lines.append(f'{indent}}}')
            else:
                lines.append(f'{indent}{json.dumps(node)} [label={json.dumps(label)}, '
                             f'tooltip={json.dumps(node)}];')

        if 'procinsh' in nodes:
            lines.append('  "procinsh" [label="main"];')
        for node in sorted(children.get('procinsh', ())):
            emit(node, '  ')
    else:
        for node in sorted(nodes):
            lines.append(f'  {json.dumps(node)} [label={json.dumps(node.removeprefix("procinsh::"))}];')
    for source, target in sorted(edges):
        lines.append(f'  {json.dumps(source)} -> {json.dumps(target)};')
    return '\n'.join(lines + ['}', ''])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--output', type=Path, default=Path('target/doc/rust-architecture'))
    args = parser.parse_args()
    nodes, edges = parse_modules(args.input.read_text())
    args.output.mkdir(parents=True, exist_ok=True)
    for name, graph in [('module-dependencies', overview(nodes, edges)),
                        ('module-dependencies-detail', (nodes, edges))]:
        dot = args.output / f'{name}.dot'
        dot.write_text(render_dot(*graph, nested=name.endswith("-detail")))
        subprocess.run(['dot', '-Tsvg', str(dot), '-o', str(args.output / f'{name}.svg')], check=True)
    shutil.copyfile(Path(__file__).with_name('architecture.html'), args.output / 'index.html')


if __name__ == '__main__':
    main()
