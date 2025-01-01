
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
        .arg(arg!(-d --"no-confirm" "Skip confirmation before deleting files")
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
    let mut surface_files = HashSet::new();
    if let Some(vec) = torrent.info.files {
        for f in vec.iter() {
            files.insert(PathBuf::from_iter(f.path.iter().map(|e| e.to_string()))
                .into_boxed_path(), f.length);
            surface_files.insert(OsString::from(
                f.path.first().ok_or(anyhow!("Empty path"))?.to_string()));
        }
    } else {
        return Err(anyhow!("Not a valid multi-file torrent"));
    }

    let mut old_files = Vec::new();
    let mut rm_size: u64 = 0;
    for entry in WalkDir::new(&dir) {
        let entry = entry.context("Failed to read directory contents")?;
        if entry.depth() == 0 { continue; }
        let path = entry.path().strip_prefix(&dir).with_context(||
            format!("Failed to strip directory contents of {:?}", &dir))?;
        if (include_sur || surface_files.contains(path.components().next().expect("Not empty")
            .as_os_str())) && !files.contains_key(path) {
            let meta = entry.metadata()?;
            if meta.is_file() {
                rm_size += meta.len();
            }
            old_files.push(entry.path().to_owned());
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

        println!("File changes:");

        for entry in &old_files {
            println!("{}  {}", Red.paint(match entry.is_dir() {
                true => "-d",
                false => "-f",
            }), path_colored(entry));
        }

        for entry in new_files.iter() {
            println!("{}   {}", Green.paint("+"), path_colored(entry));
        }

        println!();
        println!("New files: {} ({})", Green.paint(BinaryBytes(new_size)), new_files.len());
        println!("Remove entries: {} ({})", Red.paint(BinaryBytes(rm_size)), old_files.len());
    } else { // Delete files
        let files = old_files;
        println!("Existed files found:");
        for entry in &files {
            println!("{}  {}", Red.paint(match entry.is_dir() {
                true => "-d",
                false => "-f",
            }), path_colored(entry));
        }

        println!();
        println!("Remove entries: {} ({})", Red.paint(BinaryBytes(rm_size)), files.len());

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

        let mut dirs = Vec::new();
        for entry in &files {
            if entry.is_dir() {
                dirs.push(entry);
            } else {
                fs::remove_file(entry)?;
                progress.set_message(truncate_message(
                    format!("Removed file: {}", entry.to_string_lossy())));
                progress.inc(1);
            }
        }

        dirs.sort();
        dirs.reverse(); // Subdirectories go first
        for entry in dirs {
            fs::remove_dir(entry)?;
            progress.set_message(truncate_message(
                format!("Removed directory: {}", entry.to_string_lossy())));
            progress.inc(1);
        }

        progress.set_prefix("Done");
        progress.set_message(format!("{} entries removed.", files.len()));
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

fn truncate_message(message: String) -> String {
    if let Some((width, _)) = term_size::dimensions() {
        return format!("{}...", message.unicode_truncate(width.saturating_sub(10)).0)
    }
    message.to_string()
}
