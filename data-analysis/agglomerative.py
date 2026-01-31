import os
import re
import numpy as np
import pandas as pd
from collections import Counter, defaultdict

from sklearn.preprocessing import normalize
from sklearn.decomposition import PCA
from sklearn.metrics import silhouette_score

from scipy.cluster.hierarchy import linkage, fcluster

import plotly.figure_factory as ff
import plotly.express as px


OUTPUT_DIR = "clustering_outputs"
os.makedirs(OUTPUT_DIR, exist_ok=True)

def next_version(prefix_dir):
    versions = []
    for f in os.listdir(prefix_dir):
        m = re.match(r"(\d{2})_", f)
        if m:
            versions.append(int(m.group(1)))
    return f"{max(versions) + 1:02d}" if versions else "01"

VERSION = next_version(OUTPUT_DIR)

def out(name):
    return os.path.join(OUTPUT_DIR, f"{VERSION}_{name}")

df = pd.read_csv("embeddings.csv")
required_cols = {"relative_path", "title", "author"}
missing = required_cols - set(df.columns)
if missing:
    raise ValueError(f"Missing required columns: {missing}")

relative_paths = df["relative_path"].astype(str).tolist()
titles = df["title"].astype(str).tolist()
authors = df["author"].astype(str).tolist()

embedding_cols = [
    c for c in df.columns
    if c not in required_cols
]

embeddings = df[embedding_cols].values.astype("float32")

n_books, dim = embeddings.shape
print(f"Loaded {n_books} embeddings with dim={dim}")
embeddings = normalize(embeddings)
pca = PCA(n_components=40, random_state=42)
X = pca.fit_transform(embeddings)
Z = linkage(X,method="average",metric="cosine")
k_range = range(2, min(40, n_books))
sil_scores = []

for k in k_range:
    tmp_ids = fcluster(Z, t=k, criterion="maxclust")
    sil_scores.append(
        silhouette_score(X, tmp_ids, metric="cosine")
    )

best_k = k_range[int(np.argmax(sil_scores))]
print(f"Selected k = {best_k}")

# ------------------------------------------------------------
# 6. Final cluster assignment
# ------------------------------------------------------------
cluster_ids = fcluster(Z, t=best_k, criterion="maxclust")

# ------------------------------------------------------------
# 7. Build cluster → taxonomy summaries
# ------------------------------------------------------------
cluster_path_counts = defaultdict(Counter)

for cid, path in zip(cluster_ids, relative_paths):
    parts = path.strip("/").split("/")
    top_path = "/" + "/".join(parts[:2]) if len(parts) >= 2 else path
    cluster_path_counts[cid][top_path] += 1

def format_cluster_summary(counter, max_items=10):
    return ", ".join(
        f"{p} ({c})" for p, c in counter.most_common(max_items)
    )

cluster_summaries = {
    cid: format_cluster_summary(counter)
    for cid, counter in cluster_path_counts.items()
}

# ------------------------------------------------------------
# 8. Save cluster assignments
# ------------------------------------------------------------
df_out = pd.DataFrame({
    "row_id": np.arange(n_books),
    "relative_path": relative_paths,
    "title": titles,
    "author": authors,
    "cluster_id": cluster_ids,
    "taxonomy_summary": [cluster_summaries[cid] for cid in cluster_ids]
})

df_out.to_csv(out("clusters.csv"), index=False)

# ------------------------------------------------------------
# 9. Save taxonomy summary
# ------------------------------------------------------------
with open(out("cluster_taxonomy_summary.txt"), "w") as f:
    for cid in sorted(cluster_summaries):
        f.write(f"Cluster {cid}\n")
        f.write(cluster_summaries[cid] + "\n\n")

# ------------------------------------------------------------
# 10. Dendrogram
# ------------------------------------------------------------
leaf_labels = [
    f"{cluster_summaries[cid]}"
    for cid in cluster_ids
]

fig_dendrogram = ff.create_dendrogram(
    X,
    linkagefun=lambda _: Z,
    labels=leaf_labels,
    orientation="left"
)

fig_dendrogram.update_layout(
    title=f"Hierarchical Clustering (version {VERSION})",
    xaxis_title="Cosine Distance",
    height=1100
)

fig_dendrogram.write_html(out("dendrogram.html"))

# ------------------------------------------------------------
# 11. Silhouette vs k
# ------------------------------------------------------------
fig_sil = px.line(
    x=list(k_range),
    y=sil_scores,
    markers=True,
    title="Silhouette Score vs Number of Clusters",
    labels={"x": "k", "y": "Silhouette"}
)

fig_sil.write_html(out("silhouette_vs_k.html"))


pca_2d = PCA(n_components=2, random_state=42)
X_2d = pca_2d.fit_transform(embeddings)

fig_scatter = px.scatter(
    x=X_2d[:, 0],
    y=X_2d[:, 1],
    color=cluster_ids.astype(str),
    hover_data={
        "relative_path": relative_paths,
        "title": titles,
        "author": authors
    },
    title="2D PCA View of Clusters"
)

fig_scatter.write_html(out("scatter_2d.html"))

print("Outputs written with prefix:", VERSION)
