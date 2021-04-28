fn main() {
    tonic_build::compile_protos("src/at2.proto").expect("failed to compile protobufs");
}
