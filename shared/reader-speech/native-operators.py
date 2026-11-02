#!/usr/bin/env python3
"""Convert pinned voices to portable ORT graphs and generate minimal-runtime requirements."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import tempfile


def generate(models, destination):
    import onnx
    import onnxruntime as ort
    from onnxruntime.tools.ort_format_model import OrtFormatModelProcessor, OperatorTypeUsageManager

    pins = json.loads(Path(__file__).with_name("native-source.json").read_text())
    for name, version in pins["operator_tools"].items():
        actual = onnx.__version__ if name == "onnx" else ort.__version__
        if actual != version:
            raise RuntimeError(f"Operator analysis requires {name}=={version}, found {actual}")
    models, destination = Path(models), Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    required = defaultdict(set)
    type_usage = OperatorTypeUsageManager()
    element_types = set()
    value_types = set()
    evidence = {"tools": pins["operator_tools"], "models": {}, "converted_models": {}}

    def collect_tensor(tensor):
        if tensor.data_location == onnx.TensorProto.EXTERNAL or tensor.external_data:
            raise RuntimeError("Bundled voices must embed every tensor; external weights are disabled")
        element_types.add(onnx.TensorProto.DataType.Name(tensor.data_type))

    def collect_type(value):
        kind = value.WhichOneof("value")
        value_types.add(kind)
        if kind == "tensor_type":
            element_types.add(onnx.TensorProto.DataType.Name(value.tensor_type.elem_type))
        elif kind == "sequence_type":
            collect_type(value.sequence_type.elem_type)
        elif kind == "optional_type":
            collect_type(value.optional_type.elem_type)
        elif kind == "sparse_tensor_type":
            element_types.add(onnx.TensorProto.DataType.Name(value.sparse_tensor_type.elem_type))

    def collect(graph):
        for value in (*graph.input, *graph.output, *graph.value_info):
            collect_type(value.type)
        for tensor in graph.initializer:
            collect_tensor(tensor)
        if graph.sparse_initializer:
            value_types.add("sparse_tensor_type")
        for node in graph.node:
            for attribute in node.attribute:
                if node.op_type == "Cast" and attribute.name == "to":
                    element_types.add(onnx.TensorProto.DataType.Name(attribute.i))
                if attribute.type == onnx.AttributeProto.GRAPH:
                    collect(attribute.g)
                elif attribute.type == onnx.AttributeProto.GRAPHS:
                    for child in attribute.graphs:
                        collect(child)
                elif attribute.type == onnx.AttributeProto.TENSOR:
                    collect_tensor(attribute.t)
                elif attribute.type == onnx.AttributeProto.TENSORS:
                    for tensor in attribute.tensors:
                        collect_tensor(tensor)
                elif attribute.type == onnx.AttributeProto.TYPE_PROTO:
                    collect_type(attribute.tp)
                elif attribute.type == onnx.AttributeProto.TYPE_PROTOS:
                    for value in attribute.type_protos:
                        collect_type(value)
                elif attribute.type in (onnx.AttributeProto.SPARSE_TENSOR, onnx.AttributeProto.SPARSE_TENSORS):
                    value_types.add("sparse_tensor_type")

    def inspect(path):
        model = onnx.load(str(path), load_external_data=False)
        collect(model.graph)
        return ({entry.key: entry.value for entry in model.metadata_props},
                [value.name for value in model.graph.input], [value.name for value in model.graph.output])

    # Basic minimal builds load pre-optimized ORT graphs. NCHWc depends on
    # the conversion host's CPU layout, so exclude it from portable assets.
    for name, expected in pins["models"].items():
        path = models / name
        with path.open("rb") as source:
            actual = hashlib.file_digest(source, "sha256").hexdigest()
        if actual != expected:
            raise RuntimeError(f"Voice model checksum mismatch: {path}")
        metadata, inputs, outputs = inspect(path)
        converted = Path(name).with_suffix(".ort")
        optimized = destination / "models" / converted
        optimized.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=destination) as temporary:
            staged = Path(temporary) / "optimized.ort"
            options = ort.SessionOptions()
            options.intra_op_num_threads = 1
            options.inter_op_num_threads = 1
            options.enable_cpu_mem_arena = False
            options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
            options.optimized_model_filepath = str(staged)
            options.add_session_config_entry("session.save_model_format", "ORT")
            session = ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"],
                                           disabled_optimizers=["NchwcTransformer"])
            if (session.get_modelmeta().custom_metadata_map != metadata
                    or [value.name for value in session.get_inputs()] != inputs
                    or [value.name for value in session.get_outputs()] != outputs):
                raise RuntimeError(f"Voice conversion changed metadata or inputs/outputs: {path}")
            del session
            staged.replace(optimized)
        # Serialized graphs retain actual node versions and inferred types,
        # including optimizer-created operators and nested subgraphs.
        optimized_ops = {}
        processor = OrtFormatModelProcessor(str(optimized), optimized_ops, type_usage)
        processor.process()
        del processor
        for domain, versions in optimized_ops.items():
            for version, operators in versions.items():
                required[domain, version].update(operators)
        evidence["models"][name] = actual
        with optimized.open("rb") as source:
            checksum = hashlib.file_digest(source, "sha256").hexdigest()
        evidence["converted_models"][str(converted)] = {
            "sha256": checksum, "source": name, "source_bytes": path.stat().st_size,
            "bytes": optimized.stat().st_size, "metadata": metadata, "inputs": inputs, "outputs": outputs}
    # These runtime-wide switches are enabled by build_native.py. Refuse new
    # pinned models that need them instead of silently producing a broken build.
    unsupported = {name for name in element_types
                   if name == "STRING" or name.startswith(("FLOAT8", "FLOAT4"))}
    unsupported.update(value_types & {"optional_type", "sparse_tensor_type"})
    if unsupported:
        raise RuntimeError(f"Voice models require disabled tensor types: {sorted(unsupported)}")
    config = destination / "required_operators.config"
    config.write_text("# Packaged, pre-optimized ORT graphs, including subgraphs.\n"
                      + "".join(f"{domain};{version};"
                                + ",".join(operator + (type_usage.get_config_entry(domain, operator) or "")
                                           for operator in sorted(operators)) + "\n"
                                for (domain, version), operators in sorted(required.items())))
    evidence["operators"] = {f"{domain};{version}": sorted(operators)
                             for (domain, version), operators in sorted(required.items())}
    evidence["note"] = ("Analysis uses the pinned 1.28.0 wheel; native runtime is 1.28.2. "
                        "Operator types cover the packaged ORT graphs. "
                        "Validate both voices against the built runtime before rollout.")
    evidence["element_types"] = sorted(element_types)
    evidence["value_types"] = sorted(kind for kind in value_types if kind)
    evidence["operator_type_reduction"] = True
    evidence["conversion"] = {"format": "ORT", "optimization": "all", "disabled_optimizers": ["NchwcTransformer"],
                              "external_initializers": False}
    (destination / "operators.json").write_text(json.dumps(evidence, indent=2) + "\n")
    return config


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    print(generate(args.models, args.output))
