
//! Features
//! - Check the file list of files
//! - Option to take surface into account
//! - Compare content changes

extern crate core;

pub mod torrent;

use crate::torrent::parse_torrent;
use anyhow::Context;
use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use humansize::{format_size, BINARY};
use indicatif::ProgressStyle;
use inquire::Confirm;
use path_clean::PathClean;
use std::collections::HashMap;
use std::path::{Display, Path, PathBuf};
use std::{env, fmt, fs, io};
use term_painter::Color::{Blue, Green, NotSet, Red};
use term_painter::{Painted, ToStyle};
use tracing::{info, info_span, Event, Subscriber};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use walkdir::WalkDir;

struct OnlyMessageFormatter;

impl<S, N> FormatEvent<S, N> for OnlyMessageFormatter where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .arg_required_else_help(true)
        .arg(arg!(-s --surface "Take other files in the root directory into account")
            .required(false)
            .action(ArgAction::SetTrue))
        .arg(arg!(-d --"no-confirm" "Skip confirmation before deleting files")
            .required(false))
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

    let indicatif_layer = IndicatifLayer::new();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer()
            .with_writer(indicatif_layer.get_stdout_writer())
            .event_format(OnlyMessageFormatter))
        .with(indicatif_layer)
        .init();

    let path = absolute_path(matches.get_one::<PathBuf>("file").expect("required"))?;
    let dir = absolute_path(matches.get_one::<PathBuf>("dir").expect("required"))?;
    let torrent = parse_torrent(path)?;
    let mut files = HashMap::new();
    if let Some(vec) = torrent.info.files {
        for f in vec.iter() {
            files.insert(PathBuf::from_iter(f.path.iter().map(|e| e.to_string()))
                .into_boxed_path(), f.length);
        }
    } else {
        return Err(anyhow::anyhow!("Not a valid multi-file torrent"));
    }

    let mut old_files = Vec::new();
    let mut rm_size: u64 = 0;
    for entry in WalkDir::new(&dir) {
        let entry = entry.context("Failed to read directory contents")?;
        let path = entry.path().strip_prefix(&dir).with_context(||
            format!("Failed to strip directory contents of {:?}", &dir))?;
        if !files.contains_key(path) {
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

        info!("File changes:");

        for entry in old_files {
            info!("{}  {}", Red.paint(match entry.is_dir() {
                true => "-d",
                false => "-f",
            }), path_colored(&entry));
        }

        for entry in new_files.iter() {
            info!("{}   {}", Green.paint("+"), path_colored(entry));
        }

        println!();
        info!("New files: {}", Green.paint(format_size(new_size, BINARY)));
        info!("Remove files: {}", Red.paint(format_size(rm_size, BINARY)));
    } else { // Delete files
        let files = old_files;
        info!("Existed files found:");
        for entry in &files {
            info!("{}  {}", Red.paint(match entry.is_dir() {
                true => "-d",
                false => "-f",
            }), path_colored(entry));
        }

        println!();
        info!("Remove files: {}", Red.paint(format_size(rm_size, BINARY)));

        match Confirm::new(format!("Delete the above {} files?", files.len()).as_str())
            .with_default(true)
            .prompt() {
            Ok(true) => {
                info!("Confirmed.");
            }
            _ => {
                info!("Aborted.");
                return Ok(());
            }
        }

        let header_span = info_span!("deletion");
        header_span.pb_set_style(&ProgressStyle::default_bar());
        header_span.pb_set_length(files.len() as u64);
        header_span.pb_start();

        let header_span_enter = header_span.enter();

        let mut dirs = Vec::new();
        for entry in &files {
            if entry.is_dir() {
                dirs.push(entry);
            } else {
                // fs::remove_file(&entry)?;
                info!("Removed file: {}", entry.to_string_lossy());
                header_span.pb_inc(1);
            }
        }

        dirs.sort();
        dirs.reverse(); // Subdirectories go first
        for entry in dirs {
            // fs::remove_dir(entry)?;
            info!("Removed directory: {}", entry.to_string_lossy());
            header_span.pb_inc(1);
        }

        drop(header_span_enter);
        drop(header_span);
        info!("{} entries removed.", files.len());
    }

    info!("Operation completed successfully.");
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
