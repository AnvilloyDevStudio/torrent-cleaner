# Torrent Cleaner
Torrent Cleaner is a small and light utility written in Rust for directory torrenting cleaning. This is useful especially if there are updates available for particularly some torrents.

## Motivation

This project is inspired by and based on a [Java utility](https://torrent-directory-comparison.sourceforge.io/) in [qBittorrent #3842](https://github.com/qbittorrent/qBittorrent/issues/3842).
The corresponding source code for that utility is also availble [on GitHub](https://github.com/nickreserved/Torrent-Directory-Comparison).

## Utilities

```
Torrent Cleaner commandline tool

Usage: torrent-cleaner.exe [OPTIONS] <file> <dir> [COMMAND]

Commands:
  diff  Compare directory content changes instead
  help  Print this message or the help of the given subcommand(s)

Arguments:
  <file>  Specify the .torrent file; must be a multi-file torrent
  <dir>   Specify the directory storing torrent contents

Options:
  -s, --surface     Take other files in the root directory into account
  -f, --no-confirm  Skip confirmation before deleting files
  -d, --empty-dir   Include empty directories
  -h, --help        Print help
  -V, --version     Print version
```
