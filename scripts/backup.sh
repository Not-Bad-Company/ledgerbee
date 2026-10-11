#!/usr/bin/env bash

set -euo pipefail

DATA="${BOEK_DATA:?BOEK_DATA not set}"
DEST="${BOEK_BACKUP_DIR:?BOEK_BACKUP_DIR not set}"
DB="$DATA/boekhouding.db"
DAY=$(date +%F)

mkdir -p "$DEST/db" "$DEST/files" "$DATA/files"
SNAP="$DEST/db/boekhouding-$DAY.db"
rm -f "$SNAP"
sqlite3 "$DB" "VACUUM INTO '$SNAP'"
if [ "$(sqlite3 "$SNAP" 'PRAGMA integrity_check;')" != "ok" ]; then
  echo "integrity check failed on $SNAP" >&2
  exit 1
fi
gzip -f "$SNAP"

rsync -a "$DATA/files/" "$DEST/files/"

find "$DEST/db" -name 'boekhouding-*.db.gz' ! -name 'boekhouding-*-01.db.gz' -mtime +35 -delete
find "$DEST/db" -name 'boekhouding-*-01.db.gz' -mtime +2920 -delete

if [ -n "${BOEK_BACKUP_REMOTE:-}" ]; then
  rclone copy "$DEST" "$BOEK_BACKUP_REMOTE" --transfers 4
fi
echo "backup ok: $SNAP.gz, $(find "$DEST/files" -type f | wc -l) attachments"
