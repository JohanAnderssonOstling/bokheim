fn main() {
    println!("cargo:rerun-if-changed=proto/bokheim_wire.proto");
    prost_build::compile_protos(&["proto/bokheim_wire.proto"], &["proto"]).expect("compile Bokheim protobuf schema");
}
