
//! This includes all the functionalities parsing Bittorrent metafile
//! and storing them in a type.

use bendy::decoding::{Decoder, DictDecoder, FromBencode, Object};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use anyhow::{anyhow, Context};
use getset::Getters;

macro_rules! throw_bencode_redundant_key {
    () => {
        // return Err(anyhow!("Redundant key in dictionary"));
    };
}

macro_rules! check_bencode_key_missing {
    // ($v:expr, $str:tt, $($rest:tt)*) => {
    //     $v.ok_or(anyhow!("Key {} is missing in {}", stringify!($str), stringify!($($rest)*)))?
    // };
    ($v:expr, $str:ident) => {
        $v.ok_or(anyhow!("Key \"{}\" is missing in dictionary", stringify!($str)))?
    };
}

#[derive(Debug, Getters)]
pub struct TorrentFile {
    announce: Box<str>,
    info: MetaFileInfo,
}

impl TorrentFile {
    pub fn new(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file = file.as_ref();
        let mut buf = Vec::new();
        File::open(&file)?.read_to_end(&mut buf)?;
        Self::parse_torrent(buf)
            .with_context(|| format!("Error while parsing {:?}", file))?.validate_torrent()
    }

    fn parse_torrent(buf: Vec<u8>) -> anyhow::Result<Self> {
        let mut decoder = Decoder::new(&buf).with_max_depth(<usize>::MAX);
        let mut announce: Option<Box<str>> = None;
        let mut info: Option<MetaFileInfo> = None;
        {
            let obj = decoder.next_object().map_err(BendyError)?;
            match obj {
                None => return Err(anyhow!("Metafile is empty")),
                Some(Object::Dict(mut dict)) => {
                    while let Some((k, v)) = dict.next_pair().map_err(BendyError)? {
                        let key = String::from_bencode(k).map_err(BendyError)?;
                        if key == "announce" {
                            if announce.is_some() { panic!("Duplicate key in dictionary") }
                            match v {
                                Object::Bytes(s) => {
                                    announce = Some(String::from_bencode(s).map_err(BendyError)?
                                        .into_boxed_str());
                                }
                                _ => return Err(anyhow!("Invalid \"announce\" data type")),
                            }
                        } else if key == "info" {
                            if info.is_some() { panic!("Duplicate key in dictionary") }
                            match v {
                                Object::Dict(dict) => {
                                    info = Some(MetaFileInfo::parse_torrent(dict)
                                        .context("Error while parsing \"info\" in dictionary")?);
                                }
                                _ => return Err(anyhow!("Invalid \"info\" data type")),
                            }
                        } else {
                            throw_bencode_redundant_key!();
                        }
                    }
                },
                Some(_) => return Err(anyhow!("Metafile is malformed")),
            }
        }

        // No more data should be left.
        if decoder.next_object().map_err(BendyError)?.is_some() {
            Err(anyhow!("Metafile is malformed"))
        } else {
            Ok(Self {
                announce: check_bencode_key_missing!(announce, announce),
                info: check_bencode_key_missing!(info, info),
            })
        }
    }

    fn validate_torrent(self) -> anyhow::Result<Self> {
        if self.info.pieces.len() == 0 {
            return Err(anyhow!("Value \"info\".\"pieces\" is empty"));
        }

        if self.info.piece_length == 0 {
            return Err(anyhow!("Value \"info\".\"piece_length\" is zero"));
        }
        
        match &self.info.file_list {
            FileListUnion::Single(length) => {
                if length == &0 {
                    return Err(anyhow!("Value \"info\".\"length\" is zero"));
                }
            },
            FileListUnion::Multiple(list) => {
                if list.len() == 0 {
                    return Err(anyhow!("Value \"info\".\"files\" is empty"));
                }
                
                for entry in list.iter() {
                    if entry.path.len() == 0 {
                        return Err(anyhow!("Value \"info\".\"files\" element \"path\" is empty"));
                    }
                    
                    if entry.length == 0 {
                        return Err(anyhow!("Value \"info\".\"files\" element \"length\" is zero"));
                    }
                }
            }
        }

        Ok(self)
    }
}

#[derive(Debug, Getters)]
#[get = "pub"]
pub struct MetaFileInfo {
    name: Box<str>,
    piece_length: u64,
    pieces: Box<[[u8; 20]]>,
    file_list: FileListUnion,
}

impl MetaFileInfo {
    fn parse_torrent(mut dict: DictDecoder) -> anyhow::Result<Self> {
        let mut name: Option<Box<str>> = None;
        let mut piece_length: Option<u64> = None;
        let mut pieces: Option<Box<[[u8; 20]]>> = None;
        let mut file_list: Option<FileListUnion> = None;
        while let Some((k, v)) = dict.next_pair().map_err(BendyError)? {
            let key = String::from_bencode(k).map_err(BendyError)?;
            if key == "name" {
                if name.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::Bytes(s) => {
                        name = Some(String::from_bencode(s).map_err(BendyError)?.into_boxed_str())
                    }
                    _ => return Err(anyhow!("Invalid \"name\" data type")),
                }
            } else if key == "piece_length" {
                if piece_length.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::Integer(s) => {
                        piece_length = Some(<u64>::from_bencode(s.as_ref()).map_err(BendyError)?)
                    }
                    _ => return Err(anyhow!("Invalid \"piece_length\" data type")),
                }
            } else if key == "pieces" {
                if pieces.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::Bytes(mut s) => {
                        let length = s.len();
                        if length % 20 != 0 {
                            return Err(anyhow!("Invalid \"pieces\" data length"));
                        }

                        let mut pieces_vec = Vec::new();
                        for _i in 0..(length / 20) {
                            let (a, b) = s.split_first_chunk()
                                .expect("pieces in length of multiple of 20");
                            pieces_vec.push(*a);
                            s = b;
                        }
                        pieces = Some(pieces_vec.into_boxed_slice());
                    }
                    _ => return Err(anyhow!("Invalid \"pieces\" data type")),
                }
            } else if key == "length" {
                if file_list.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::Integer(s) => {
                        file_list = Some(FileListUnion::Single(
                            <u64>::from_bencode(s.as_ref()).map_err(BendyError)?))
                    }
                    _ => return Err(anyhow!("Invalid \"length\" data type")),
                }
            } else if key == "files" {
                if file_list.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::List(mut list) => {
                        let mut files = Vec::new();
                        while let Some(o) = list.next_object().map_err(BendyError)? {
                            match o {
                                Object::Dict(mut dict) => {
                                    files.push(FileListItem::parse_torrent(dict)
                                        .context("Error while parsing \"files\" element")?);
                                }
                                _ => return Err(anyhow!("Invalid \"files\" element data type")),
                            }
                        }
                        file_list = Some(FileListUnion::Multiple(files.into_boxed_slice()))
                    }
                    _ => return Err(anyhow!("Invalid \"files\" data type")),
                }
            } else {
                throw_bencode_redundant_key!();
            }
        }
        
        Ok(Self {
            name: name.ok_or(anyhow!("Key \"name\" is missing"))?,
            piece_length: piece_length.ok_or(anyhow!("Key \"piece_length\" is missing"))?,
            pieces: pieces.ok_or(anyhow!("Key \"pieces\" is missing"))?,
            file_list: file_list.ok_or(anyhow!("Key \"length\" or \"files\" is missing"))?,
        })
    }
}

#[derive(Debug)]
pub enum FileListUnion {
    Single(u64),
    Multiple(Box<[FileListItem]>),
}

#[derive(Debug, Getters)]
#[get = "pub"]
pub struct FileListItem {
    length: u64,
    path: Box<[Box<str>]>,
}

impl FileListItem {
    fn parse_torrent(mut dict: DictDecoder) -> anyhow::Result<Self> {
        let mut length = None;
        let mut path = None;
        while let Some((k, v)) = dict.next_pair().map_err(BendyError)? {
            let key = String::from_bencode(k).map_err(BendyError)?;
            if key == "length" {
                if length.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::Integer(s) => {
                        length = Some(<u64>::from_bencode(s.as_ref()).map_err(BendyError)?)
                    }
                    _ => return Err(anyhow!("Invalid \"length\" data type")),
                }
            } else if key == "path" {
                if path.is_some() { panic!("Duplicate key in dictionary") }
                match v {
                    Object::List(mut list) => {
                        let mut path_vec = Vec::new();
                        while let Some(o) = list.next_object().map_err(BendyError)? {
                            match o {
                                Object::Bytes(s) => {
                                    path_vec.push(String::from_bencode(s).map_err(BendyError)?
                                        .into_boxed_str());
                                }
                                _ => return Err(anyhow!("Invalid \"path\" element data type")),
                            }
                        }
                        path = Some(path_vec.into_boxed_slice());
                    }
                    _ => return Err(anyhow!("Invalid \"path\" data type")),
                }
            }
        }
        Ok(Self {
            length: check_bencode_key_missing!(length, length),
            path: check_bencode_key_missing!(path, path),
        })
    }
}

#[derive(Debug)]
struct BendyError(bendy::decoding::Error);

impl Display for BendyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for BendyError {}
