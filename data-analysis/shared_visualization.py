from typing import Optional, Dict, List, Tuple, Any, Counter as CounterType
import numpy as np
import pandas as pd
from pathlib import Path
from collections import Counter, defaultdict
from sklearn.preprocessing import normalize
from sklearn.decomposition import PCA
import plotly.graph_objects as go
from plotly.colors import qualitative

FIGURE_WIDTH = 2000
FIGURE_HEIGHT = 1400
HOVER_LABEL_STYLE = {
    "bgcolor": "white",
    "bordercolor": "black",
    "font": {"size": 14, "family": "Arial, sans-serif"},
    "align": "left",
}
DEFAULT_PARENT_DIR_LIMIT = 20
DEFAULT_TITLE_LIMIT = 20


NON_EMBEDDING_COLS = {"relative_path", "title", "author"}


def load_embeddings(embedding_csv: str) -> np.ndarray:
    df = pd.read_csv(embedding_csv)
    embedding_cols = [c for c in df.columns if c not in NON_EMBEDDING_COLS]
    embeddings = df[embedding_cols].values.astype("float32")
    return normalize(embeddings)


def load_clusters(cluster_csv: str) -> pd.DataFrame:
    df = pd.read_csv(cluster_csv)
    if not {"relative_path", "cluster_id"}.issubset(df.columns):
        raise ValueError("clusters.csv missing required columns")
    return df


def get_parent_dirs(paths: List[str]) -> List[str]:
    out = []
    for fp in paths:
        parent = Path(fp).parent
        out.append(str(parent) if str(parent) not in ("", ".") else "root")
    return out


def compute_pca(embeddings: np.ndarray, n_components: int = 2) -> np.ndarray:
    return PCA(n_components=n_components, random_state=42).fit_transform(embeddings)


def compute_centroids(X: np.ndarray, cluster_ids: np.ndarray) -> Tuple[Dict[int, np.ndarray], Dict[int, int]]:
    centroids = {}
    sizes = {}
    for cid in np.unique(cluster_ids):
        idx = np.where(cluster_ids == cid)[0]
        centroids[cid] = X[idx].mean(axis=0)
        sizes[cid] = len(idx)
    return centroids, sizes


def apply_default_layout(fig: go.Figure, title: str, xaxis_title: Optional[str] = None, yaxis_title: Optional[str] = None, hide_axes: bool = False) -> go.Figure:
    layout = dict(
        title=title,
        width=FIGURE_WIDTH,
        height=FIGURE_HEIGHT,
    )
    if hide_axes:
        layout["xaxis"] = dict(visible=False)
        layout["yaxis"] = dict(visible=False)
    else:
        if xaxis_title:
            layout["xaxis_title"] = xaxis_title
        if yaxis_title:
            layout["yaxis_title"] = yaxis_title
    fig.update_layout(**layout)
    return fig


def apply_hover_label_style(fig: go.Figure) -> None:
    fig.update_layout(hovermode="closest")
    fig.update_traces(hoverlabel=HOVER_LABEL_STYLE)


def parent_directory(path: str) -> str:
    parent = Path(path).parent
    parent_str = str(parent)
    return parent_str if parent_str not in ("", ".") else "root"


def summarize_parent_dirs(relative_paths: List[str], cluster_ids: np.ndarray, hierarchy_df: Optional[pd.DataFrame] = None, limit: int = DEFAULT_PARENT_DIR_LIMIT) -> Dict[int, List[Tuple[str, int]]]:
    counts = defaultdict(Counter)
    for cid, path in zip(cluster_ids, relative_paths):
        counts[cid][parent_directory(path)] += 1

    if hierarchy_df is None:
        return {
            cid: counter.most_common(limit)
            for cid, counter in counts.items()
        }

    node_children = defaultdict(list)
    leaf_cluster_map = {}
    for _, row in hierarchy_df.iterrows():
        node_id = int(row["cluster_id"])
        parent = row["parent_cluster_id"]
        if pd.notna(parent):
            node_children[int(parent)].append(node_id)
        leaf_cluster_id = row.get("leaf_cluster_id")
        if pd.notna(leaf_cluster_id):
            leaf_cluster_map[node_id] = int(leaf_cluster_id)

    summary = {}

    def aggregate(node):
        counter = Counter()
        if node in leaf_cluster_map:
            counter = counts.get(leaf_cluster_map[node], Counter()).copy()
        else:
            for child in node_children.get(node, []):
                counter += aggregate(child)
        summary[node] = counter.most_common(limit)
        return counter

    roots = [
        int(row["cluster_id"])
        for _, row in hierarchy_df.iterrows()
        if pd.isna(row["parent_cluster_id"])
    ]
    for root in roots:
        aggregate(root)

    return summary


def collect_cluster_titles(meta_df: pd.DataFrame, cluster_ids: np.ndarray, limit: int = DEFAULT_TITLE_LIMIT) -> Dict[int, List[str]]:
    titles = defaultdict(list)
    for cid, title in zip(cluster_ids, meta_df["title"].astype(str)):
        if limit is not None and len(titles[cid]) >= limit:
            continue
        titles[cid].append(title)
    return titles


def build_hover_text(lines: List[str]) -> str:
    filtered = [line for line in lines if line]
    return "<br>".join(filtered)


def build_level_colors(levels: List[Any], palette: Optional[List[str]] = None) -> Dict[Any, str]:
    palette = palette or qualitative.Plotly
    unique_levels = sorted(set(levels))
    return {
        level: palette[i % len(palette)]
        for i, level in enumerate(unique_levels)
    }


def add_marker_trace(fig: go.Figure, xs: np.ndarray, ys: np.ndarray, hover_text: Optional[List[str]] = None, marker: Optional[Dict[str, Any]] = None, **kwargs: Any) -> None:
    trace = dict(
        x=xs,
        y=ys,
        mode=kwargs.pop("mode", "markers"),
        showlegend=kwargs.pop("showlegend", False),
        marker=marker or {},
    )
    if hover_text is not None:
        # Ensure hover_text is a list matching the number of points
        if isinstance(hover_text, str):
            hover_text = [hover_text]
        trace["hovertext"] = hover_text
        trace["hovertemplate"] = "%{hovertext}<br><extra></extra>"
    trace.update(kwargs)
    fig.add_trace(go.Scatter(**trace))


def add_line_trace(fig: go.Figure, xs: np.ndarray, ys: np.ndarray, line: Optional[Dict[str, Any]] = None, **kwargs: Any) -> None:
    trace = dict(
        x=xs,
        y=ys,
        mode=kwargs.pop("mode", "lines"),
        showlegend=kwargs.pop("showlegend", False),
        line=line or {},
    )
    trace.update(kwargs)
    fig.add_trace(go.Scatter(**trace))
