# Boekhouding

The administration of Not Bad Company (eenmanszaak, KvK 42185336): scan
receipts, read PDF invoices, send invoices, import the bank, log hours,
kilometres and assets, and get the quarterly btw-aangifte and the yearly
inkomstenbelasting figures ready to copy into Mijn Belastingdienst
Zakelijk. Reminders before every deadline on the phone and by e-mail.

Design and decisions: [docs/SPEC.md](docs/SPEC.md).

```
server/   Rust (axum + SQLite): all data, all tax maths, PDFs, e-mail, reminders
app/      Flutter: Android (scanning with ML Kit, offline outbox) and Linux desktop
deploy/   systemd units for the Pi
scripts/  backup.sh
```

## Run it locally

```sh
cd server
cp .env.example .env              # BOEK_SECRET can stay empty in debug builds
BOEK_DATA=./data cargo run -- create-user daanhessen@notbadcompany.com
BOEK_DATA=./data cargo run -- serve          # http://127.0.0.1:8090
cargo test

cd ../app
flutter run -d linux             # log in with server http://127.0.0.1:8090
flutter test
```

`create-user` asks for a password (12+ characters), prints a QR code for
the authenticator app and ten recovery codes. Running it again replaces the
account and signs every device out.

## Deploy on the Pi

The server needs `pdftotext` (poppler-utils) and `sqlite3`.

```sh
# on the Pi
cd ~/projects/work && git clone <repo> boekhouding     # or rsync the folder
cd boekhouding/server && cp .env.example .env && $EDITOR .env   # BOEK_SECRET: openssl rand -hex 32
cargo build --release
./target/release/boekhouding create-user daanhessen@notbadcompany.com
sudo cp ../deploy/boekhouding.service ../deploy/boekhouding-backup.* /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now boekhouding boekhouding-backup.timer
```

Expose it through the existing Cloudflare tunnel with an ingress rule such
as `boeken.notbadcompany.com → http://127.0.0.1:8090`. The API refuses
everything without a device token, and a token needs password + TOTP.

## Install the app

- **Android:** `cd app && flutter build apk --release`, then install
  `build/app/outputs/flutter-apk/app-release.apk` (sideload). Release builds
  are signed with the debug key until a keystore is set up.
- **Linux:** `cd app && flutter build linux --release`; the bundle is
  `build/linux/x64/release/bundle/`. Copy it to `~/.local/opt/boekhouding`
  and add a `.desktop` file pointing at its `boekhouding` binary.

## The routine

- **When you pay:** scan the bon (phone: the button on Overzicht or Kosten),
  check the draft, *Bevestigen*. PDF invoices: *Kosten toevoegen* on the
  laptop. Offline scans wait in the app and are sent later.
- **1st of the month** (the e-mail reminds you): import the bank CSV, link
  the suggestions, find the receipts that are missing.
- **Each quarter** (deadline the last day of the next month): *Aangifte →
  Btw*, copy the whole-euro figures into Mijn Belastingdienst Zakelijk, pay,
  then *Ingediend* (that locks the quarter).
- **Before 1 May:** *Aangifte → Inkomstenbelasting* for last year, export
  the zip, file, *Ingediend*.

## Every January

Add the new year to `server/src/tax/mod.rs` (`YEARS`): zelfstandigenaftrek,
startersaftrek, MKB-winstvrijstelling, KIA table, box 1 brackets, km rate.
A year that is missing falls back to the newest one and the report says so.

## Not yet

- The btw-id (`NBC_BTW` in the shared `.env`) is still a placeholder; put it
  in when the Belastingdienst sends it. Invoices must carry it.
- `BOEK_IBAN` is empty until the business account exists.
- Off-site backups: set `BOEK_BACKUP_REMOTE` to an rclone remote.
