# Nemo Files — audit, 2026-11-18

Read: `ops.rs` (copy, move, delete, trash, restore, link, extract, compress), `trash.rs`, `fsx.rs` naming helpers.
Not yet read: panes, drawing, mouse and keyboard handling, thumbnails, devices, portal, remote (about 8,000 of 12,000 lines).
Tests were 6; they are now 10.

## What is solid

- **Copying.** The kernel copies in 16 MB pieces (reflink on btrfs/xfs), falls back to read/write for FUSE, old kernels and `/proc`
  files, reports progress and honours cancel; a cancelled copy removes its half-written file. Permissions and times are kept.
- **Special files.** Symlinks stay symlinks, FIFOs are recreated rather than read (a read would block forever), devices and
  sockets are refused with a message. Directory permissions are applied after the contents, so read-only folders copy.
- **Move.** `rename` first, copy-and-delete only across disks, and the source is deleted only when the copy had no errors.
- **Trash.** Freedesktop layout shared with Dolphin and Nautilus; a foreign `.trashinfo` cannot restore a file outside its own disk
  (tested).
- **Archives.** `bsdtar` with `--` so a file named like an option is not read as one; a single top-level folder is lifted.

## Fixed in this pass

1. `free_name` turned `b.tar.gz` into `b.tar (2).gz`. It now uses the same extension rule as the rename code: `b (2).tar.gz`.
2. The "folder into itself" guard compared raw paths, so copying `/a` into `/link-to-a/sub` was not caught and would recurse until
   the path got too long. It now compares real paths.

3. **Replace is now safe for files.** The copy goes to a hidden temporary name beside the target and only a complete copy is
   renamed over the old file (one atomic `rename`, same disk); a failed or cancelled copy removes the temporary and leaves the old
   file untouched. Folders still merge as before.

## Known, not changed (decisions for later)

- A symlink to a folder counts as a folder in the "into itself" check (harmless: it only blocks a copy that makes no sense).
- `Compress` with sources outside the destination folder passes absolute paths to `bsdtar`.

## Gaps against the plan

No batch rename, no breadcrumb path bar, no search, no storage view, no archive dialog, no right-to-left names. These are the
planned upgrades (`~/nemo/06`), and each needs its own design and tests.

## Next audit steps

Read `pane.rs`, `mouse.rs`, `thumbs.rs` and `devices.rs`; measure memory at idle and with a large folder open; add tests for
undo of every operation kind.
