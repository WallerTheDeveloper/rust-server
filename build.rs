use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(
        &[
            "proto/common.proto",
            "proto/client.proto",
            "proto/server.proto",
        ],
        &["proto/"],
    )?;
    Ok(())
}