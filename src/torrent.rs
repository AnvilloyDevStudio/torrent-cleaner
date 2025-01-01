use librqbit_buffers::ByteBufOwned;
use librqbit_core::torrent_metainfo::{torrent_from_bytes_ext, TorrentMetaV1};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub fn parse_torrent(file: impl AsRef<Path>) -> anyhow::Result<TorrentMetaV1<ByteBufOwned>> {
    let file = file.as_ref();
    let mut buf = Vec::new();
    File::open(&file)?.read_to_end(&mut buf)?;
    let buf = ByteBufOwned::from(buf);
    Ok(torrent_from_bytes_ext(buf.as_ref())?.meta)
}
