#!/usr/bin/env python3
"""Render source-path Rust module dependencies, preserving re-export boundaries."""
import argparse
import json
from pathlib import Path
import tempfile
import shutil
import subprocess


def extract_modules(root, target=None):
    """Read source paths under the host (or requested target) rustc configuration."""
    command = ['rustc', '--print', 'cfg']
    if target:
        command.extend(['--target', target])
    config = subprocess.check_output(command, cwd=root, text=True)
    with tempfile.TemporaryDirectory(prefix='procinsh-dependencies-') as directory:
        cfg_file = Path(directory) / 'rustc.cfg'
        cfg_file.write_text(config)
        command = [
            'cargo', 'run', '--quiet', '--locked', '--manifest-path',
            str(root / 'scripts/rust-dependencies/Cargo.toml'),
            '--target-dir', str(root / 'target/rust-dependencies'),
            '--', '--cfg-file', str(cfg_file),
            str(root), 'procinsh', 'src/http_server/entry.rs',
            str(root), 'procinsh_privileged_helper', 'src/privileged_helper/entry.rs',
        ]
        graph = json.loads(subprocess.check_output(command, cwd=root, text=True))
    graph['configuration'] = config.splitlines()
    return graph


def overview(nodes, edges):
    def top(name):
        # Summarize each executable at its subsystem boundary.
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
                    label = '(module)' if '::' in node else 'main'
                    lines.append(f'{indent}  {json.dumps(node)} [label={json.dumps(label)}, '
                                 f'tooltip={json.dumps(node)}];')
                for child in sorted(children[node]):
                    emit(child, indent + '  ')
                lines.append(f'{indent}}}')
            else:
                lines.append(f'{indent}{json.dumps(node)} [label={json.dumps(label)}, '
                             f'tooltip={json.dumps(node)}];')

        for node in sorted({n.split('::', 1)[0] for n in nodes}):
            emit(node, '  ')
    else:
        for node in sorted(nodes):
            lines.append(f'  {json.dumps(node)} [label={json.dumps(node)}];')
    for source, target in sorted(edges):
        lines.append(f'  {json.dumps(source)} -> {json.dumps(target)};')
    return '\n'.join(lines + ['}', ''])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', help='rustc target configuration to analyze')
    parser.add_argument('--output', type=Path, default=Path('target/doc/rust-architecture'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    graph = extract_modules(root, args.target)
    nodes = set(graph['nodes'])
    edges = {tuple(edge) for edge in graph['edges']}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'source-dependencies.json').write_text(json.dumps(graph, indent=2) + '\n')
    for name, graph in [('module-dependencies', overview(nodes, edges)),
                        ('module-dependencies-detail', (nodes, edges))]:
        dot = args.output / f'{name}.dot'
        dot.write_text(render_dot(*graph, nested=name.endswith("-detail")))
        subprocess.run(['dot', '-Tsvg', str(dot), '-o', str(args.output / f'{name}.svg')], check=True)
    shutil.copyfile(Path(__file__).with_name('architecture.html'), args.output / 'index.html')


if __name__ == '__main__':
    main()
