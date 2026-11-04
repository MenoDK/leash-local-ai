# Backups

## Schedule and retention

A snapshot of the shared drive is taken every night at 02:00. The seven most recent daily snapshots are kept, then one
snapshot per week for eight weeks. Older snapshots are deleted automatically; nothing is kept by hand.

## Restoring a file

To restore a single file, open the snapshot browser on the file server, pick the day you want, and copy the file back
to its original folder. Never restore over a file somebody is editing: copy it next to the original and compare.

## Restoring a whole machine

A full restore takes about forty minutes. Boot the recovery image from the USB stick in the cabinet, choose the latest
snapshot of that machine, and wait for the checksum verification to finish before rebooting. If the verification
fails, stop and choose the previous snapshot instead.
