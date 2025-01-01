
//! Features
//! - Check the file list of files
//! - Option to take surface into account
//! - Compare content changes

extern crate core;

pub mod torrent;

use crate::torrent::parse_torrent;
use anyhow::{anyhow, Context};
use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use indicatif::{BinaryBytes, ProgressBar, ProgressStyle};
use inquire::Confirm;
use path_clean::PathClean;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Display, Path, PathBuf};
use std::time::Duration;
use std::{env, fs, io};
use term_painter::Color::{Blue, Green, NotSet, Red};
use term_painter::{Painted, ToStyle};
use unicode_truncate::UnicodeTruncateStr;
use walkdir::WalkDir;

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .arg_required_else_help(true)
        .arg(arg!(-s --surface "Take other files in the root directory into account")
            .required(false)
            .action(ArgAction::SetTrue))
        .arg(arg!(-f --"no-confirm" "Skip confirmation before deleting files")
            .required(false)
            .action(ArgAction::SetTrue))
        .arg(arg!(-d --"empty-dir" "Include empty directories")
            .required(false)
            .action(ArgAction::SetTrue))
        .arg(Arg::new("file")
            .help("Specify the .torrent file; must be a multi-file torrent")
            .required(true)
            .value_parser(value_parser!(PathBuf)))
        .arg(Arg::new("dir")
            .help("Specify the directory storing torrent contents")
            .required(true)
            .value_parser(value_parser!(PathBuf)))
        .subcommand_required(false)
        .subcommand(Command::new("diff")
            .about("Compare directory content changes instead"))
        .get_matches();

    let path = absolute_path(matches.get_one::<PathBuf>("file").expect("required"))?;
    let dir = absolute_path(matches.get_one::<PathBuf>("dir").expect("required"))?;
    let include_sur = matches.get_flag("surface");
    let no_confirm = matches.get_flag("no-confirm");
    let include_empty_dir = matches.get_flag("empty-dir");

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::default_spinner()
        .tick_chars("|/-\\")
        .template("{spinner:.green} [{elapsed_precise}] {msg}")?);
    spinner.set_message("Parsing...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let result = parse_torrent(&spinner, path);
    spinner.finish_and_clear();
    drop(spinner);
    let torrent = result?;
    println!("Parsing completed.\n");

    let mut files = HashMap::new();
    let mut dirs = HashSet::new();
    let mut surface_files = HashSet::new();
    if let Some(vec) = torrent.info.files {
        for f in vec.iter() {
            let segs = f.path.iter().map(|e| e.to_string()).collect::<Vec<String>>();
            files.insert(PathBuf::from_iter(&segs).into_boxed_path(), f.length);
            surface_files.insert(OsString::from(
                f.path.first().ok_or(anyhow!("Empty path"))?.to_string()));
            dirs.extend(list_recursive_dirs(segs))
        }
    } else {
        return Err(anyhow!("Not a valid multi-file torrent"));
    }

    let mut old_files = Vec::new();
    let mut empty_dirs = Vec::new();
    let mut rm_size: u64 = 0;
    for entry in WalkDir::new(&dir) {
        let entry = entry.context("Failed to read directory contents")?;
        if entry.depth() == 0 { continue; } // skip root
        let path = entry.path().strip_prefix(&dir).with_context(||
            format!("Failed to strip directory contents of {:?}", &dir))?;
        if (include_sur || surface_files.contains(path.components().next().expect("Not empty")
            .as_os_str())) && !files.contains_key(path) {
            let meta = entry.metadata()?;
            if meta.is_file() {
                rm_size += meta.len();
            }

            if meta.is_dir() {
                if include_empty_dir && check_dir_kind_of_empty(entry.path()) {
                    empty_dirs.push(entry.path().to_owned());
                }
            } else {
                old_files.push(entry.path().to_owned());
            }
        }
    }

    fn path_colored(path: &Path) -> Painted<Display> {
        match path.is_dir() {
            true => Blue.paint(path.display()),
            false => NotSet.paint(path.display()),
        }
    }

    // Compare directory
    if matches.subcommand_matches("diff").is_some() {
        let mut new_files = Vec::new();
        let mut new_size: u64 = 0;
        for entry in files.iter() {
            let path = dir.join(entry.0);
            if !path.exists() {
                new_files.push(path);
                new_size += entry.1;
            }
        }

        if new_files.is_empty() && old_files.is_empty() && empty_dirs.is_empty() {
            println!("No matching entries found.");
            return Ok(());
        }

        println!("File changes:");

        for entry in &old_files {
            println!("{}  {}", Red.paint("-f"), path_colored(entry));
        }

        for entry in &empty_dirs {
            println!("{}  {}", Red.paint("-d"), path_colored(entry));
        }

        for entry in new_files.iter() {
            println!("{}   {}", Green.paint("+"), path_colored(entry));
        }

        println!();
        println!("New files: {} ({})", Green.paint(BinaryBytes(new_size)), new_files.len());
        println!("Remove entries: {} ({})", Red.paint(BinaryBytes(rm_size)),
                 old_files.len() + empty_dirs.len());
    } else { // Delete files
        let files = old_files;

        let progress = if files.is_empty() {
            println!("No matching entries found.");
            if !include_empty_dir {
                println!("Aborted.");
                return Ok(())
            }

            let progress = ProgressBar::no_length();
            progress.set_style(ProgressStyle::default_spinner()
                .tick_chars("|/-\\|/-\\ ")
                .template("{prefix} [{elapsed_precise}] {spinner:.green}\n{msg}")?);
            progress.enable_steady_tick(Duration::from_millis(50));
            progress
        } else {
            println!("Existed files found:");
            for entry in &files {
                println!("{}  {}", Red.paint(match entry.is_dir() {
                    true => "-d",
                    false => "-f",
                }), path_colored(entry));
            }

            println!();
            println!("Remove files: {} ({})", Red.paint(BinaryBytes(rm_size)), files.len());

            if !no_confirm {
                match Confirm::new(format!("Delete the above {} files?", files.len()).as_str())
                    .with_default(true).prompt() {
                    Ok(true) => {
                        println!("Confirmed.");
                    }
                    _ => {
                        println!("Aborted.");
                        return Ok(());
                    }
                }
            }

            let progress = ProgressBar::new(files.len() as u64);
            progress.set_style(ProgressStyle::default_bar()
                .template("{prefix} [{wide_bar:.cyan/blue}] {pos}/{len} ({percent}%)\n{msg}")?);
            progress.set_prefix("Processing");

            for entry in &files {
                fs::remove_file(entry)?;
                progress.set_message(truncate_message(
                    format!("Removed file: {}", entry.to_string_lossy())));
                progress.inc(1);
            }

            progress
        };

        let mut count = files.len();
        if include_empty_dir {
            progress.set_prefix("Clearing dirs");
            let vec = find_empty_dirs(dir);
            let mut empty_dirs = vec.iter().filter(|e| !dirs.contains(*e))
                .collect::<Vec<&PathBuf>>();
            empty_dirs.sort();
            empty_dirs.reverse();
            for entry in &empty_dirs {
                fs::remove_dir_all(entry)?;
                progress.set_message(truncate_message(
                    format!("Removed directory: {}", entry.to_string_lossy())));
            }
            count += empty_dirs.len();
        }

        progress.set_prefix("Done");
        progress.set_message(format!("{} entries removed.", count));
        progress.finish();
    }

    println!("Operation completed successfully.");
    Ok(())
}

/// Source: https://stackoverflow.com/a/54817755
pub fn absolute_path(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = path.as_ref();

    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    }.clean();

    Ok(absolute_path)
}

// Credit: Copilot
fn check_dir_kind_of_empty<P: AsRef<Path>>(path: P) -> bool {
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Recursively check the subdirectory
                if !check_dir_kind_of_empty(&path) {
                    return false;
                }
            } else {
                // If there's any file, the directory is not empty
                return false;
            }
        }
    }
    // If we loop through all entries and find only empty directories, return true
    true
}

// Credit: Copilot
fn find_empty_dirs<P: AsRef<Path>>(path: P) -> Vec<PathBuf> {
    let mut empty_dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(&path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if check_dir_kind_of_empty(&path) {
                    empty_dirs.push(path.clone());
                }
                empty_dirs.extend(find_empty_dirs(path));
            }
        }
    }
    empty_dirs
}

fn list_recursive_dirs<I: IntoIterator<Item = impl AsRef<Path>>>(iter: I) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut head = PathBuf::new();
    for path in iter.into_iter() {
        head.push(path);
        paths.push(head.clone());
    }
    paths
}

fn truncate_message(message: String) -> String {
    if let Some((width, _)) = term_size::dimensions() {
        return format!("{}...", message.unicode_truncate(width.saturating_sub(10)).0)
    }
    message.to_string()
}
