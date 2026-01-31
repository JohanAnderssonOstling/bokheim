import numpy as np
import plotly.graph_objects as go
from math import cos, sin, pi
import pandas as pd

from shared_visualization import load_clusters


CLUSTER_META_CSV = "clustering_outputs/cluster_hierarchy.csv"
OUTPUT_HTML = "radial_cluster_hierarchy.html"
RADIUS_STEP = 200


df = pd.read_csv(CLUSTER_META_CSV)

# build tree
children = {}
for _, r in df.iterrows():
    pid = r["parent_cluster_id"]
    if pd.notna(pid):
        children.setdefault(pid, []).append(r["cluster_id"])


def radial_positions(root_id):
    pos = {}

    def recurse(cid, level, angle_start, angle_end):
        angle = (angle_start + angle_end) / 2
        r = level * RADIUS_STEP
        pos[cid] = (r * cos(angle), r * sin(angle))

        kids = children.get(cid, [])
        if not kids:
            return

        step = (angle_end - angle_start) / len(kids)
        for i, k in enumerate(kids):
            recurse(
                k,
                level + 1,
                angle_start + i * step,
                angle_start + (i + 1) * step,
            )

    recurse(root_id, 0, 0, 2 * pi)
    return pos
root_id = df.loc[df["level"] == 0, "cluster_id"].iloc[0]
positions = radial_positions(root_id)

fig = go.Figure()

# edges
for _, r in df.iterrows():
    if pd.isna(r["parent_cluster_id"]):
        continue
    x0, y0 = positions[r["parent_cluster_id"]]
    x1, y1 = positions[r["cluster_id"]]
    fig.add_trace(
        go.Scatter(
            x=[x0, x1],
            y=[y0, y1],
            mode="lines",
            line=dict(width=1),
            hoverinfo="skip",
            showlegend=False,
        )
    )

# nodes
for _, r in df.iterrows():
    cid = r["cluster_id"]
    x, y = positions[cid]

    fig.add_trace(
        go.Scatter(
            x=[x],
            y=[y],
            mode="markers",
            marker=dict(
                size=18 if r["is_leaf"] else 10,
                symbol="circle",
            ),
            hovertemplate=(
                f"<b>Cluster {cid}</b><br>"
                f"Level: {r['level']}<br>"
                f"{'Leaf (contains books)' if r['is_leaf'] else 'Internal node'}"
                "<extra></extra>"
            ),
            showlegend=False,
        )
    )

fig.update_layout(
    title="Radial Cluster Hierarchy (Books at Leaves Only)",
    xaxis=dict(visible=False),
    yaxis=dict(visible=False),
    width=1200,
    height=1200,
)

fig.write_html(OUTPUT_HTML)
