
//! Features
//! - Check the file list of files
//! - Option to take surface into account
//! - Compare content changes

extern crate core;

pub mod torrent;

use clap::{arg, command, value_parser, Arg, ArgAction, Command};
use std::path::PathBuf;
use crate::torrent::TorrentFile;

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .arg_required_else_help(true)
        .arg(arg!([name] "Optional name to operate on"))
        .arg(arg!(-s --surface "Take other files in the root directory into account")
            .required(false)
            .action(ArgAction::SetTrue))
        .arg(arg!(-d --"no-confirm" "Skip confirmation before deleting files")
            .required(false))
        .arg(Arg::new("file")
            .help("Specify the .torrent file")
            .required(true)
            .value_parser(value_parser!(PathBuf)))
        .subcommand_required(false)
        .subcommand(Command::new("diff")
            .about("Compare directory content changes instead"))
        .get_matches();
    let path: &PathBuf = matches.get_one("file").expect("required");
    let torrent = TorrentFile::new(path)?;
    
    
    Ok(())
}
