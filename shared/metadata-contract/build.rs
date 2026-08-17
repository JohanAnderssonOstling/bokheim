fn main() {
    println!("cargo:rerun-if-changed=proto/metadata_v2.proto");
    prost_build::Config::new().compile_protos(&["proto/metadata_v2.proto"], &["proto"]).expect("metadata v2 protobuf schema compiles");
}
