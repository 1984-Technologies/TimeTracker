fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../backend/proto/metrics.proto");
    tonic_build::compile_protos("../backend/proto/metrics.proto")?;
    Ok(())
}
