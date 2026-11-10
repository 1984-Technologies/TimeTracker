fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/metrics.proto");
    tonic_build::compile_protos("proto/metrics.proto")?;
    Ok(())
}
