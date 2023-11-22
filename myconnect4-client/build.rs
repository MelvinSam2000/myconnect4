fn main() {
    tonic_build::compile_protos("../shared/proto/myconnect4.proto").unwrap();
}
