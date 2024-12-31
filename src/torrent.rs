
//! This includes all the functionalities parsing Bittorrent metafile
//! and storing them in a type.

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub struct TorrentFile {
    announce: Box<str>,
}

impl TorrentFile {
    pub fn new(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let mut buf = Vec::new();
        File::open(file)?.read_to_end(&mut buf)?;
        
        todo!();
        // TorrentFile {
        //     
        // }
    }
}
