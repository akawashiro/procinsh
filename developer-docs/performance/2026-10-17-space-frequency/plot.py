#!/usr/bin/env python3
"""Render CPU averages and the two observed runs as a standalone PNG."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

root = Path(__file__).resolve().parent
data = json.loads((root / 'summary.json').read_text())['cases']
order = ['s1', 's2', 's5', 'p100', 'idle']
labels = ['1 s scan\n10 ms poll', '2 s scan\n10 ms poll', '5 s scan\n10 ms poll',
          '1 s scan\n100 ms poll', 'SPACE\nclosed']
fig, ax = plt.subplots(figsize=(11, 6), layout='constrained')
x = list(range(len(order)))
user = [data[k]['user_percent'] for k in order]
system = [data[k]['system_percent'] for k in order]
ax.bar(x, system, label='Kernel CPU', color='#4472c4', width=.65)
ax.bar(x, user, bottom=system, label='User CPU', color='#79b9a6', width=.65)
for i, key in enumerate(order):
    runs = data[key]['cpu_runs']
    ax.scatter([i + (j-(len(runs)-1)/2)*.12 for j in range(len(runs))], runs,
               color='#172334', s=28, zorder=3, label='Individual runs' if i == 0 else None)
    cpu = data[key]['cpu_percent']
    ax.annotate(f'{cpu:.2f}%', (i, max(runs)), xytext=(0, 12),
                textcoords='offset points', ha='center', fontsize=12, weight='bold')
ax.set_xticks(x, labels, fontsize=11)
ax.set_ylabel('Backend CPU usage (one logical CPU = 100%)')
ax.set_title('SPACE backend: effect of collection frequency', fontsize=16, pad=16)
ax.set_ylim(0, max(data[k]['cpu_percent'] for k in order)*1.3)
ax.grid(axis='y', alpha=.2)
ax.set_axisbelow(True)
ax.spines[['top', 'right']].set_visible(False)
ax.legend(loc='upper right')
fig.text(.5, -.035, 'Release build; 20 Chrome tabs reloading every 10 s; 2 runs × 60 s per case; 30 s warmup.',
         ha='center', fontsize=10)
fig.savefig(root / 'cpu-frequency.png', dpi=180, bbox_inches='tight')
