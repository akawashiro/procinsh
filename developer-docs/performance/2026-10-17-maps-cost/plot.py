#!/usr/bin/env python3
"""Render whole-backend ablation and the directly timed maps-reader scope."""
import json
from pathlib import Path
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

root=Path(__file__).resolve().parent
s=json.loads((root/'summary.json').read_text())
normal, absent=s['normal'],s['no_maps']
maps=normal['maps_scope']
fig,axes=plt.subplots(1,2,figsize=(12,6),layout='constrained')
left,right=axes
left.bar([0,1],[normal['system_percent'],absent['system_percent']],color='#4472c4',label='Kernel CPU',width=.6)
left.bar([0,1],[normal['user_percent'],absent['user_percent']],bottom=[normal['system_percent'],absent['system_percent']],color='#79b9a6',label='User CPU',width=.6)
for i,r in enumerate((normal,absent)):
    left.scatter([i-.07,i+.07],r['cpu_runs'],c='#172334',s=28,zorder=3)
    left.annotate(f"{r['cpu_percent']:.2f}%",(i,max(r['cpu_runs'])),xytext=(0,12),textcoords='offset points',ha='center',weight='bold')
left.set_xticks([0,1],['Normal','maps disabled'])
left.set_title('Whole backend: removal experiment')
left.legend(loc='upper right')
read,parse=maps['read_cpu_percent'],maps['parse_cpu_percent']
rest=normal['cpu_percent']-read-parse
right.bar([0],[read],color='#4472c4',label=f'Read: {read:.2f}% CPU',width=.5)
right.bar([0],[parse],bottom=[read],color='#e7a34f',label=f'Parse: {parse:.2f}% CPU',width=.5)
right.bar([0],[rest],bottom=[read+parse],color='#dadfe5',label='Outside read_maps scope',width=.5)
right.set_xticks([0],['Normal backend'])
right.set_title(f"Direct reader scope: {maps['share_of_backend_cpu_percent']:.1f}% of backend CPU")
right.legend(loc='upper right',fontsize=9)
for ax in axes:
    ax.set_ylim(0,max(normal['cpu_runs']+absent['cpu_runs'])*1.3)
    ax.set_ylabel('CPU usage (one logical CPU = 100%)')
    ax.grid(axis='y',alpha=.2)
    ax.set_axisbelow(True)
    ax.spines[['top','right']].set_visible(False)
fig.suptitle('SPACE maps: measured CPU cost',fontsize=17)
fig.text(.5,-.035,'Release; 20 Chrome tabs; two 60 s runs per case. Reader scope excludes output handling/encoding; removal includes those effects.',ha='center',fontsize=9)
fig.savefig(root/'maps-cpu.png',dpi=180,bbox_inches='tight')
