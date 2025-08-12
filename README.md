# jbackup

jbackup is a program that manages compressed directory backups using `tar` and `xdelta3`.

The original use is to compress backups of Minecraft world saves, however, the script should work for most directories.

jbackup is under development, however, the version of jbackup on the master branch should be capable of performing the basic operations.

## Performance

### Minecraft

In a multiplayer world with 3 active players, I made 15 backups over 2 months from the start of the world. The first backup was 163.6MB. The last backup was 1.3GB.

- When compressing all backups with ZIP, Deflate (fastest), the total backup size was 8.4GB.
- jbackup stores all snapshots in 1.8GB (**467%** compression).
  - Each delta only averages **27.6MB** -- meaning that, in this case, each new snapshot takes `change in world size + 27.6mb` more bytes to store on average.

## Installing

### Compatibility

At the moment, the tool is only tested on Ubuntu. The author believes the program should be compatible with all Unix systems.

### Compiling from source

After installing Rust, run `cargo build --release` to produce the executable `target/release/jbackup`. This is the only file required and can be moved anywhere you find convenient.

## Usage

`jbackup` may be placed anywhere. This section assumes `jbackup` is in PATH for convenience.

If you don't plan to use `jbackup` for more directories, you can put `jbackup` under a new directory called `.jbackup` directly under the directory you wish to backup. (i.e, `cd myFiles; mkdir .jbackup; mv /path/to/jbackup .jbackup; .jbackup/jbackup init`)

### Initialization

A repository must be initialized first (like `git`) before the script can track changes.

```
$ cd directoryToBackup
$ jbackup init
```

The `init` subcommand creates a `.jbackup` directory (similar to the `.git` directory.) The `.jbackup` directory will contain all history information about it's parent directory.


#### Transformers

Transformers change files before storing and restoring. Changing files can make the compression better -- meaning smaller file sizes.

Transformers can only be specified during repository initialization.

```
$ cd minecraftWorldToBackup
$ jbackup init --transformer minecraft
```

The `--transformer` option tells `jbackup` there are minecraft files. `jbackup` can compress Minecraft files better with a transformer.

### Config (Not implemented)

You may configure the options to compress: None / Fast / Default / Best

None skips the compression step. This may be ideal for small files or files that have already been compressed. Do not use None compression with transformers! Transformers often decompress input files to achieve smaller deltas (smaller backups).

If not specified, the default compression is Fast.

### Snapshots

We can store 'snapshots' of the parent directory using the `snapshot` command.

```
$ jbackup snapshot
```

The first `snapshot` will simply create a tarball of the parent directory (excluding `.jbackup`) and store the tarball in `.jbackup`.

Following `snapshot`s will:
1. create a tarball of parent directory (excluding `.jbackup`)
2. `xdelta` to create a diff from the current tarball with the previous tarball
3. The patch will be stored in `.jbackup`
4. The previous tarball is deleted, and replaced with the current tarball

You may optionally supply a snapshot message. For example:

```
$ jbackup snapshot -m "Created an iron farm at spawn in Minecraft"
```

### Log

We can view all snapshots by using the `log` command.

```
$ jbackup log
```

*All* snapshots will be listed in chronological order.

If there are multiple branches, the snapshots not part of the current branch will be shown with brackets around them.

### Restore

You can restore a snapshot given the snapshot's ID by using the `restore` command.

```
$ jbackup restore 1749058471-eb03dacbfbc30c61600ca60859fb33f7
```

The working directory will be overwritten with the contents of the snapshot.

### Branches (not implemented)

A branch is special snapshot that stores 'parallel' states alongside other snapshots.

The most common use will be to store "bad" states. For example (in the context of Minecraft saves):

- 01-01 (A): world created
- 01-02 (B): players make progress
- 01-03 (C): someone griefs the world
- 01-03 (D): players make progress
- 01-04 (E): restore to time 01-02 (point B), then make progress
- 01-05 (F): players make more progress

We can represent the relation of worlds in a tree:

```
 A
 |
 B
 | \
 C  E
 |  |
 D  F (main)
```

In this case, we can more optimally store snapshot C and D as a diff applied on snapshot B.

The commands to form this tree would look like the following:

```
$ jbackup snapshot -m "A"
snapshotIdA

(files change)

$ jbackup snapshot -m "B"
snapshotIdB

(files change)

$ jbackup snapshot -m "C"
snapshotIdC

(files change)

$ jbackup snapshot -m "D"
snapshotIdD
$ jbackup branch rename griefed-world
$ jbackup restore snapshotIdB
$ jbackup branch new main

(files change)

$ jbackup snapshot -m "E"
snapshotIdE

(files change)

$ jbackup snapshot -m "F"
snapshotIdF
```

## Internal Structure

This section describes the `.jbackup` directory.

When initialized, the directory looks like:

```
$ ls .jbackup
branches
head
snapshots
$ cat .jbackup/branches
main    NULL
$ cat .jbackup/head
NULL
$ ls .jbackup/snapshots
```

After the first commit, the directory looks like:

```
$ ls .jbackup
branches
head
snapshots
$ cat .jbackup/branches  # we see that main now points to an ID
main    1748490695-d96dbc36c710a6163736f9903b9e5137
$ cat .jbackup/head  # the current "checked out" snapshot ID
1748490695-d96dbc36c710a6163736f9903b9e5137
$ ls .jbackup/snapshots  # list of all snapshots
1748490695-d96dbc36c710a6163736f9903b9e5137-full.tar.gz
1748490695-d96dbc36c710a6163736f9903b9e5137.meta
$ cat .jbackup/snapshots/1748490695-d96dbc36c710a6163736f9903b9e5137.meta
date    1748490695
full    tar.gz
message My message goes here...\nNew lines are escaped. Backslashes are escaped (\\)
```

- In the `{snapshotId}.meta` file, we have a file with a key-value pair separated by the first tab on the line
- the 'full' key specifies a _type_ (ex. tar, tar.gz) that the full contents of the snapshot are stored in, located at `{snapshotId}-full.{type}`
- the 'child' key specifies later snapshots derived from this
- the 'parent' key specifies previous snapshots this snapshot was derived from
- additional 'd' (diff) keys
  - required to store the relationship of diffs (the above keys represent logical / chronological order)
  - idea: the parent/child relationship is only modified explicity by the user using snapshot/branches. The dparent/dchild relationships are modified automatically based on performance/storage optimizations
  - the 'dchild' key specifies the snapshot (_dchild_) such that the snapshot (_snapshotId_) can be recovered by applying the delta list `{snapshotId}-diff-{dchild}` to _dchild_
  - the 'dparent' key is the inverse of 'dchild'. That is: specifies the snapshot (_dparent_) such that the snapshot (_snapshotId_) can be used to recover _dparent_ by applying the delta list `{dparent}-diff-{snapshotId}` to _dparent_

After the second commit, the directory looks like:
```
$ ls .jbackup
branches
head
snapshots
$ cat .jbackup/branches  # we see that main now points to a different ID
main    1748491449-fecdcb27c5bf6e100e42c637feb40394
$ cat .jbackup/head  # the head changed
1748491449-fecdcb27c5bf6e100e42c637feb40394
$ ls .jbackup/snapshots  # a new snapshot was added
1748490695-d96dbc36c710a6163736f9903b9e5137-diff-1748491449-fecdcb27c5bf6e100e42c637feb40394
1748490695-d96dbc36c710a6163736f9903b9e5137.meta
1748491449-fecdcb27c5bf6e100e42c637feb40394-full.tar.gz
1748491449-fecdcb27c5bf6e100e42c637feb40394.meta
$ cat .jbackup/snapshots/1748490695-d96dbc36c710a6163736f9903b9e5137.meta  # the previous snapshot metadata was updated
date    1748490695
child   1748491449-fecdcb27c5bf6e100e42c637feb40394
dchild  1748491449-fecdcb27c5bf6e100e42c637feb40394
message My message goes here...\nNew lines are escaped. Backslashes are escaped (\\)
$ cat .jbackup/snapshots/1748491449-fecdcb27c5bf6e100e42c637feb40394.meta  # the next snapshot is full
date    1748490695
full    tar.gz
parent  1748490695-d96dbc36c710a6163736f9903b9e5137
dparent 1748490695-d96dbc36c710a6163736f9903b9e5137
```

- The `*-diff-*` file is a [delta list](./src/delta_list.rs), which is a list of outputs from xdelta3 for each file. The delta list always lists files with paths in alphabetical order.
