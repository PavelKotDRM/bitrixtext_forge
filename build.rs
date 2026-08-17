use vergen_gitcl::{Build, Emitter, Gitcl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = Build::builder().build_timestamp(true).build();
    let git = Gitcl::all_git();

    Emitter::default()
        .add_instructions(&build)?
        .add_instructions(&git)?
        .emit()?;

    Ok(())
}