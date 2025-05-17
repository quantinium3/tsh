use clap::{value_parser, Arg, ArgAction, Command};
use regex::Regex;
use std::env;
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use which::which;

#[derive(Debug)]
enum TshError {
    IoError(io::Error),
    MissingDependencies(Vec<String>),
    CommandFailed(String),
    NoDirectoriesFound,
    UserCancelled,
}

impl fmt::Display for TshError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TshError::IoError(err) => write!(f, "I/O error: {}", err),
            TshError::MissingDependencies(deps) => write!(f, "MissingDependencies: {:?}", deps),
            TshError::CommandFailed(cmd) => write!(f, "Command failed: {}", cmd),
            TshError::NoDirectoriesFound => write!(f, "No directories found"),
            TshError::UserCancelled => write!(f, "Operation cancelled by user"),
        }
    }
}

impl Error for TshError {}

impl From<io::Error> for TshError {
    fn from(err: io::Error) -> Self {
        TshError::IoError(err)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let matches = Command::new("tsh")
        .version("0.1.0")
        .about("Tmux Session Handler - Select directories with fzf and create tmux sessions")
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

    let selected_dir = find_and_select_directory(&directories, dir_option)?;

    match selected_dir {
        Some(dir) => create_tmux_session(&dir)?,
        None => println!("No directory selected. Exiting."),
    }

    Ok(())
}

fn check_dependencies(deps: &[&str]) -> Result<(), TshError> {
    let missing: Vec<String> = deps
        .iter()
        .filter(|&pkg| which(pkg).is_err())
        .map(|&s| s.to_string())
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(TshError::MissingDependencies(missing))
    }
}

fn find_and_select_directory(
    directories: &[String],
    custom_dir: Option<&PathBuf>,
) -> Result<Option<PathBuf>, TshError> {
    let home_dir = env::var("HOME")
        .map(PathBuf::from)
        .or_else(|_| env::current_dir())
        .map_err(|e| TshError::IoError(e))?;

    if !directories.is_empty() {
        let mut search_paths = Vec::new();

        for search_dir in directories {
            let output = ProcessCommand::new("find")
                .arg(&home_dir)
                .arg("-type")
                .arg("d")
                .arg("-name")
                .arg(search_dir)
                .output()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if !line.is_empty() {
                        search_paths.push(line.to_string());
                    }
                }
            } else {
                return Err(TshError::CommandFailed(format!(
                    "find command for {}",
                    search_dir
                )));
            }
        }

        if search_paths.is_empty() {
            return Err(TshError::NoDirectoriesFound);
        }

        println!("Searching in directories: {:?}", search_paths);

        return run_fd_with_fzf(&search_paths);
    }

    if let Some(dir) = custom_dir {
        println!("Searching in custom directory: {:?}", dir);
        run_fzf_in_directory(dir)
    } else {
        println!("Running default behavior (searching in home directory)...");
        run_fzf_in_directory(&home_dir)
    }
}

fn run_fd_with_fzf(search_paths: &[String]) -> Result<Option<PathBuf>, TshError> {
    let find_cmd = if which("fd").is_ok() { "fd" } else { "find" };

    let mut all_dirs = Vec::new();
    for path in search_paths {
        let args = if find_cmd == "fd" {
            vec![".", path]
        } else {
            vec![path, "-type", "d"]
        };

        let output = ProcessCommand::new(find_cmd).args(&args).output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if !line.is_empty() {
                    all_dirs.push(line.to_string());
                }
            }
        } else {
            return Err(TshError::CommandFailed(format!(
                "{} command for {}",
                find_cmd, path
            )));
        }
    }

    if all_dirs.is_empty() {
        return Err(TshError::NoDirectoriesFound);
    }

    let mut fzf_cmd = ProcessCommand::new("fzf")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let fzf_stdin = fzf_cmd
            .stdin
            .as_mut()
            .ok_or_else(|| TshError::CommandFailed("Failed to open fzf stdin".to_string()))?;
        for dir in all_dirs {
            fzf_stdin.write_all(format!("{}\n", dir).as_bytes())?;
        }
    }

    let output = fzf_cmd.wait_with_output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(selected)))
    }
}

fn run_fzf_in_directory(dir: &Path) -> Result<Option<PathBuf>, TshError> {
    let find_cmd = if which("fd").is_ok() { "fd" } else { "find" };

    let dir_str = dir
        .to_str()
        .ok_or_else(|| TshError::CommandFailed("Invalid directory path".to_string()))?;

    let args = if find_cmd == "fd" {
        vec![".", "--type", "d", dir_str]
    } else {
        vec![dir_str, "-type", "d"]
    };

    let find_output = ProcessCommand::new(find_cmd).args(&args).output()?;

    if !find_output.status.success() {
        return Err(TshError::CommandFailed(format!(
            "Failed to execute {} command",
            find_cmd
        )));
    }

    let exclude_pattern = Regex::new(r"/node_modules/|/\.git/|/\.cache/|/tmp/|/Library/")
        .map_err(|e| TshError::CommandFailed(format!("Failed to compile regex: {}", e)))?;

    let dirs: Vec<String> = String::from_utf8_lossy(&find_output.stdout)
        .lines()
        .filter(|line| !line.is_empty() && !exclude_pattern.is_match(line))
        .map(|s| s.to_string())
        .collect();

    if dirs.is_empty() {
        return Err(TshError::NoDirectoriesFound);
    }

    let mut fzf_cmd = ProcessCommand::new("fzf")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    {
        let fzf_stdin = fzf_cmd
            .stdin
            .as_mut()
            .ok_or_else(|| TshError::CommandFailed("Failed to open fzf stdin".to_string()))?;
        for dir in dirs {
            fzf_stdin.write_all(format!("{}\n", dir).as_bytes())?;
        }
    }

    let output = fzf_cmd.wait_with_output()?;

    if !output.status.success() {
        return Ok(None);
    }

    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(selected)))
    }
}

fn create_tmux_session(dir: &Path) -> Result<(), TshError> {
    let session_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            TshError::CommandFailed("Could not extract session name from directory".to_string())
        })?
        .replace(".", "_");

    let in_tmux = env::var("TMUX").is_ok();

    let output = ProcessCommand::new("tmux")
        .args(&["has-session", "-t", &session_name])
        .output()?;

    let has_session = output.status.success();

    if has_session {
        println!("Session '{}' already exists, attaching...", session_name);

        if in_tmux {
            let status = ProcessCommand::new("tmux")
                .args(&["switch-client", "-t", &session_name])
                .status()?;

            if !status.success() {
                return Err(TshError::CommandFailed(
                    "Failed to switch tmux client".to_string(),
                ));
            }
        } else {
            let status = ProcessCommand::new("tmux")
                .args(&["attach-session", "-t", &session_name])
                .status()?;

            if !status.success() {
                return Err(TshError::CommandFailed(
                    "Failed to attach to tmux session".to_string(),
                ));
            }
        }
    } else {
        println!("Creating new session '{}'...", session_name);

        let dir_str = dir.to_string_lossy();

        if in_tmux {
            let create_status = ProcessCommand::new("tmux")
                .args(&["new-session", "-d", "-s", &session_name, "-c", &dir_str])
                .status()?;

            if !create_status.success() {
                return Err(TshError::CommandFailed(
                    "Failed to create tmux session".to_string(),
                ));
            }

            let switch_status = ProcessCommand::new("tmux")
                .args(&["switch-client", "-t", &session_name])
                .status()?;

            if !switch_status.success() {
                return Err(TshError::CommandFailed(
                    "Failed to switch tmux client".to_string(),
                ));
            }
        } else {
            let status = ProcessCommand::new("tmux")
                .args(&["new-session", "-A", "-s", &session_name, "-c", &dir_str])
                .status()?;

            if !status.success() {
                return Err(TshError::CommandFailed(
                    "Failed to create and attach to tmux session".to_string(),
                ));
            }
        }
    }

    Ok(())
}
