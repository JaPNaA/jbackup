# xdelta backups

This repository contains a Rust script that manages compressed directory backups using `tar` and `xdelta`.

The original use is to compress backups of Minecraft world saves, however, the script should work for all `tar`-able directories.

## Usage

The script may be placed anywhere. This section assumes `jbackup` is in your path for convience.

If you don't plan to use `jbackup` for more directories, you can put `jbackup` under a new directory called `.jbackup` directly under the directory you wish to backup.

### Initialization

A repository must be initalized first (like `git`) before the script can track changes.

```
$ cd directoryToBackup
$ jbackup init
```

The `init` subcommand creates a `.jbackup` directory (similar to the `.git` directory.) The `.jbackup` directory will contain all history information about it's parent directory.

### Commits

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

You may optionally supply a commit message. For example:

```
$ jbackup commit -m "Created an iron farm at spawn in Minecraft"
```

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

When initalized, the directory looks like:

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
1748490695-d96dbc36c710a6163736f9903b9e5137-full.tar
1748490695-d96dbc36c710a6163736f9903b9e5137.meta
$ cat .jbackup/snapshots/1748490695-d96dbc36c710a6163736f9903b9e5137.meta
date    1748490695
full    tar
message My message goes here...\nNew lines are escaped. Backslashes are escaped (\\)
```

- In the `{snapshotId}.meta` file, we have a file with a key-value pair separated by the first tab on the line
- the 'full' key specifies a _type_ (ex. tar, tar.gz) that the full contents of the snapshot are stored in, located at `{snapshotId}-full.{type}`
- the 'child' key specifies later snapshots derived from this
- the 'parent' key specifies previous snapshots this snapshot was derived from
- additional 'd' (diff) keys
  - required to store the relationship of diffs (the above keys represent logical / chronological order)
  - idea: the parent/child relationship is only modified explicity by the user using snapshot/branches. The dparent/dchild relationships are modified automatically based on performance/storage optimizations
  - the 'dchild' key specifies the snapshot (_dchild_) such that the snapshot (_snapshotId_) can be recovered by applying the delta file `{snapshotId}-diff-{dchild}` to _dchild_
  - the 'dparent' key is the inverse of 'dchild'. That is: specifies the snapshot (_dparent_) such that the snapshot (_snapshotId_) can be used to recover _dparent_ by applying the delta file `{dparent}-diff-{snapshotId}` to _dparent_


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
1748491449-fecdcb27c5bf6e100e42c637feb40394
1748491449-fecdcb27c5bf6e100e42c637feb40394.meta
$ cat .jbackup/snapshots/1748490695-d96dbc36c710a6163736f9903b9e5137.meta  # the previous snapshot metadata was updated
child   1748491449-fecdcb27c5bf6e100e42c637feb40394
dchild  1748491449-fecdcb27c5bf6e100e42c637feb40394
date    1748490695
message My message goes here...\nNew lines are escaped. Backslashes are escaped (\\)
$ cat .jbackup/snapshots/1748491449-fecdcb27c5bf6e100e42c637feb40394.meta  # the next snapshot is full
date    1748490695
parent  1748490695-d96dbc36c710a6163736f9903b9e5137
dparent 1748490695-d96dbc36c710a6163736f9903b9e5137
full    true
```
