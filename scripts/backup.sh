#!/usr/bin/env bash
#
# Nightly backup: a consistent SQLite snapshot (VACUUM INTO, safe while the
# server runs in WAL mode) plus the attachments, into $BOEK_BACKUP_DIR. Daily
# snapshots are kept 35 days and the one from the 1st of each month for
# eight years (bewaarplicht: seven). Attachments are content-addressed and
# never change, so they are mirrored, not copied per day. With
# BOEK_BACKUP_REMOTE set (an rclone remote:path) the folder goes off-site too.

set -euo pipefail

DATA="${BOEK_DATA:?BOEK_DATA not set}"
DEST="${BOEK_BACKUP_DIR:?BOEK_BACKUP_DIR not set}"
DB="$DATA/boekhouding.db"
DAY=$(date +%F)

mkdir -p "$DEST/db" "$DEST/files"
SNAP="$DEST/db/boekhouding-$DAY.db"
rm -f "$SNAP"
sqlite3 "$DB" "VACUUM INTO '$SNAP'"
if [ "$(sqlite3 "$SNAP" 'PRAGMA integrity_check;')" != "ok" ]; then
  echo "integrity check failed on $SNAP" >&2
  exit 1
fi
gzip -f "$SNAP"

rsync -a "$DATA/files/" "$DEST/files/"

# Keep daily snapshots 35 days, monthly ones (the 1st) for eight years.
find "$DEST/db" -name 'boekhouding-*.db.gz' ! -name 'boekhouding-*-01.db.gz' -mtime +35 -delete
find "$DEST/db" -name 'boekhouding-*-01.db.gz' -mtime +2920 -delete

if [ -n "${BOEK_BACKUP_REMOTE:-}" ]; then
  rclone copy "$DEST" "$BOEK_BACKUP_REMOTE" --transfers 4
fi
echo "backup ok: $SNAP.gz, $(find "$DEST/files" -type f | wc -l) attachments"
