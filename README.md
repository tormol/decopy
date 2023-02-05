# Decopy - a program to find identical files

## WIP

The scanning / reading part works, but using the data has barely been started on.

## Goals

* Skip files with IO errors instead of aborting the program.
* Write file information from scanning to a sqlite database.
  (One disk I want to use this on is failing, so I want to minimize usages of it.)
* Use python scripts to analyze that data and decide what to do.

## Features implemented so far

* Writes file information from scanning to a sqlite database.
* Uses different threads for reading and hashing, to keep the disk busy.
* Number of threads for reading and hashing can be set independently.
  Note that hasher threads will stick to one file until it has been completely read and hashed.
* Bounded memory usage: reader threads will wait if hasher thread(s) can't keep up.
* Hasher threads set minimum CPU priority.
* On Linux, the program set lowest IO priority.
* Logs throughput.

## Comparison with [fclones](https://github.com/pkolaczk/fclones)

It's much more advanced than what I plan to:

* Automatically adapts to whether it's reading from a HDD or SSD. (using sysinfo crate.)
* Orders reads based on a files position on disk. (using [fiemap](https://docs.rs/fiemap/latest/fiemap/) crate.)
* Allows choosing between many hashing algorithms, while this program only supports SHA-2.
* Uses the sled database, and I think io_uring?

But It doesn't appear to support saving the scan results, only the file hashes.

## Comparison with [czkawka](https://github.com/qarmin/czkawka)

It has a GUI and advanced ways to compare not quite identical files.
But when I tried it (quite some time ago) it didn't seem to be designed for quite what I want.

## License

Copyright 2023 Torbjørn Birch Moltu

This program is free software: you can redistribute it and/or modify it under the
terms of the GNU General Public License as published by the Free Software Foundation,
either version 3 of the License, or (at your option) any later version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with this program.
If not, see https://www.gnu.org/licenses/

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, shall be licensed as above,
without any additional terms or conditions.
