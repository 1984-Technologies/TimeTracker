use protobuf_codegen::CodeGen;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    CodeGen::new()
        .include("proto")
        .inputs(["upload_client.proto"])
        .output_dir("generated")
        .generate_and_compile()?;
    Ok(())
}
