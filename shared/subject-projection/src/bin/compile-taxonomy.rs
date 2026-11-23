use std::path::PathBuf;

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(3..=4).contains(&args.len()) {
        return Err("usage: compile-taxonomy SOURCE OUTPUT RELEASE [PREVIOUS_RUNTIME]".into());
    }
    let release = args[2].to_str().ok_or("invalid release")?.parse().map_err(|e: std::num::ParseIntError| e.to_string())?;
    subject_projection::compiler::compile(&PathBuf::from(&args[0]), &PathBuf::from(&args[1]), release, args.get(3).map(PathBuf::from).as_deref())
}
