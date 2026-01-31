"""
Pure Plotly visualization file.
- No clustering
- No evaluation
- No versioning logic
- Only visualization of existing clustering outputs
"""

import numpy as np
import pandas as pd

from shared_visualization import summarize_parent_dirs

from sklearn.preprocessing import normalize
from sklearn.decomposition import PCA

import plotly.graph_objects as go

# ============================================================
# CONFIG (only things you change)
# ============================================================
VERSION = "01"  # clustering version to visualize
EMBEDDING_CSV = "embeddings.csv"
CLUSTERS_CSV = f"clustering_outputs/{VERSION}_clusters.csv"
OUTPUT_HTML = f"{VERSION}_plotly_cluster_centroids.html"

MAX_PATHS_ON_HOVER = 10
PCA_COMPONENTS = 2   # visualization only


df_embed = pd.read_csv(EMBEDDING_CSV)
df_clusters = pd.read_csv(CLUSTERS_CSV)

# required columns from clustering output
required = {"relative_path", "cluster_id"}
if not required.issubset(df_clusters.columns):
    raise ValueError("clusters.csv missing required columns")

# embeddings = all columns except metadata
non_embedding_cols = {"relative_path", "title", "author"}
embedding_cols = [c for c in df_embed.columns if c not in non_embedding_cols]

embeddings = df_embed[embedding_cols].values.astype("float32")
rel_paths = df_clusters["relative_path"].astype(str).values
cluster_ids = df_clusters["cluster_id"].values

embeddings = normalize(embeddings)

dir_summary = summarize_parent_dirs(rel_paths, cluster_ids, MAX_PATHS_ON_HOVER)
pca = PCA(n_components=PCA_COMPONENTS, random_state=42)
X_2d = pca.fit_transform(embeddings)

centroids = {}
cluster_sizes = {}
cluster_hover_text = {}
for cid in np.unique(cluster_ids):
    idx = np.where(cluster_ids == cid)[0]

    # centroid in 2D space
    centroids[cid] = X_2d[idx].mean(axis=0)

    # cluster size
    cluster_sizes[cid] = len(idx)

    # build hover text from shared summary
    entries = dir_summary.get(cid, [])
    cluster_hover_text[cid] = "<br>".join(f"{path} ({count})" for path, count in entries)


# ============================================================
# 5. Build Plotly figure
# ============================================================
fig = go.Figure()

# scale marker circumference by cluster size
sizes = np.array(list(cluster_sizes.values()))
size_scale = 40 / np.sqrt(sizes.max())

for cid, (x, y) in centroids.items():
    fig.add_trace(
        go.Scatter(
            x=[x],
            y=[y],
            mode="markers",
            marker=dict(
                size=cluster_sizes[cid] * size_scale,
                sizemode="diameter",
                symbol="circle-open",
                line=dict(width=2)
            ),
            hovertemplate=(
                f"<b>Cluster {cid}</b><br>"
                f"Size: {cluster_sizes[cid]}<br><br>"
                f"{cluster_hover_text[cid]}"
                "<extra></extra>"
            ),
            showlegend=False
        )
    )

fig.update_traces(hoverlabel=dict(bgcolor="white"))

# ============================================================
# 6. Layout + save
# ============================================================
fig.update_layout(
    title=f"Cluster Centroids (version {VERSION})",
    xaxis_title="PCA-1 (visual only)",
    yaxis_title="PCA-2 (visual only)",
    width=1200,
    height=1200
)

fig.write_html(OUTPUT_HTML)
print(f"Plotly visualization written to {OUTPUT_HTML}")
