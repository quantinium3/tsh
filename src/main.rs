use clap::{value_parser, Arg, ArgAction, Command};
use std::path::PathBuf;
use which::which;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = Command::new("tsh")
        .version("0.1.0")
        .about("tmux session handler")
        .arg(Arg::new("directory").action(ArgAction::Append))
        .arg(
            Arg::new("dir")
                .short('d')
                .long("dir")
                .value_name("PATH")
                .num_args(1)
                .help("Set custom directory to search in")
                .value_parser(value_parser!(PathBuf)),
        )
        .get_matches();

    check_dependencies(&["fzf", "tmux"])?;

    let directories: Vec<String> = matches
        .get_many::<String>("directory")
        .unwrap_or_default()
        .cloned()
        .collect();

    let dir_option = matches.get_one::<PathBuf>("dir");

    handle_input(&directories, dir_option);

    Ok(())
}

fn handle_input(directories: &[String], dir_option: Option<&PathBuf>) {
    if !directories.is_empty() {
        println!("directory: {:?}", directories);
    }

    if let Some(dir) = dir_option {
        println!("value for dir: {}", dir.display());
    }

    if directories.is_empty() && dir_option.is_none() {
        println!("Running default behavior...");
    }
}

fn check_dependencies(deps: &[&str]) -> Result<(), String> {
    let missing: Vec<&str> = deps
        .iter()
        .copied()
        .filter(|pkg| which(pkg).is_err())
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("Missing dependencies: {:?}", missing))
    }
}
