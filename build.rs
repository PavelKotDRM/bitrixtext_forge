use vergen_gitcl::{Build, Emitter, Gitcl};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let build = Build::builder().build_timestamp(true).build();
    let has_git_metadata = std::path::Path::new(".git").exists();

    let mut emitter = Emitter::default();
    emitter.add_instructions(&build)?;
    if has_git_metadata {
        emitter.add_instructions(&Gitcl::all_git())?;
    }
    emitter.emit()?;

    Ok(())
}
