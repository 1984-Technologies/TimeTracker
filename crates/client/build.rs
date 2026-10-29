use protobuf_codegen::CodeGen;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    CodeGen::new()
        .include("../backend/proto")
        .inputs(["metrics.proto"])
        .output_dir("generated")
        .generate_and_compile()?;
    Ok(())
}
