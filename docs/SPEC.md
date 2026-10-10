# Boekhouding — design

Bookkeeping for Not Bad Company (eenmanszaak, KvK 42185336, owner Daan
Hessen). Written 2026-10-06 from a brainstorm with the owner; he then asked
for autonomous build.

## Goal

One place that holds everything an eenmanszaak needs, so the quarterly
btw-aangifte and the yearly inkomstenbelasting are a matter of copying
numbers:

- scan a pinbonnetje with the phone and have it filled in;
- read PDF invoices (hosting, domains, software) on the laptop;
- make and send sales invoices with every legal field;
- import bank statements and match them, flag costs without a bonnetje;
- log hours (urencriterium), kilometres, business assets;
- compute the btw-aangifte per quarter (Belastingdienst rubrieken) and a
  winst / aftrekposten overview per year;
- remind before every deadline, on the phone and by e-mail.

Non-goals: payroll, multiple users, filing directly with the
Belastingdienst (no Digipoort; numbers are copied into Mijn Belastingdienst
Zakelijk), double-entry ledger, iOS, web.

## Decisions (owner)

| Topic | Decision |
|---|---|
| Home | Own app for Not Bad Company, separate from Krentenweger |
| Storage | Own server (Pi, later Hetzner VPS), synced; 7-year bewaarplicht |
| Btw | Normal regime, quarterly aangifte, no KOR |
| Scanning | On-device OCR (Google ML Kit), deterministic parser, user confirms; no LLM |
| Sales | Full invoicing: clients, numbered invoice PDF, e-mail, paid/overdue |
| Bank | CSV / CAMT.053 import with auto-match. No PSD2 |
| IB | Hours, winst + aftrekposten, assets/depreciation, kilometres |
| Platforms | Flutter, native Android + native Linux desktop |
| Stack | Rust server holds all tax logic; Flutter client with offline outbox |

## Architecture

```
boekhouding/
  server/   Rust: axum + rusqlite (r2d2), one binary `boekhouding`
  app/      Flutter: Android + Linux
  docs/
  deploy/   systemd unit, env example
```

**Server.** Single user. JSON API under `/api`. SQLite at
`$BOEK_DATA/boekhouding.db`; attachments content-addressed at
`$BOEK_DATA/files/<sha256>` and never deleted (bewaarplicht). A background
task sends reminder e-mails (Brevo HTTP API, the same provider Krentenweger
uses) once a day. Same Rust toolchain and crate choices as krentenweger-web
so one person maintains both.

**Auth.** One account. Password (argon2) + TOTP (RFC 6238). Login returns a
device token (random 32 bytes, stored hashed) that the app keeps in secure
storage; tokens are listed and revocable. First run: `boekhouding
create-user` on the server prints the TOTP enrolment URI.

**Client.** Online-first with an offline outbox: reads are cached in a local
JSON cache so the app opens offline; writes made offline (a scanned
bonnetje mostly) go to an outbox with a client-generated UUID and are
replayed on reconnect. The server treats a repeated UUID as the same
record (idempotent create). Edits to existing records require being online.
This is simpler than full two-way sync and covers the real offline case:
scanning in a shop.

## Data model

All money in integer cents. All dates ISO `YYYY-MM-DD`. Every table has
`id` (UUID text), `created_at`, `updated_at`.

- `documents` — incoming cost: kind (`bonnetje`, `factuur`), date, supplier
  name, supplier_id, total_cents, currency (EUR only), file sha256, ocr_text,
  status (`concept`, `bevestigd`), note, paid_private (paid from private
  account).
- `document_lines` — document_id, description, amount_excl_cents,
  vat_rate (`21`, `9`, `0`, `vrijgesteld`, `verlegd_eu`, `verlegd_buiten_eu`,
  `geen`), vat_cents, category_id, business_pct (default 100).
- `categories` — name, ib_group (IB aangifte cost group: `auto`,
  `huisvesting`, `kantoor`, `verkoop`, `overig`, `afschrijving`), deductible_pct
  (e.g. representatie 80), vat_deductible (bool). Seeded.
- `suppliers` — name, kvk, vat_id, default_category_id, match_terms
  (lowercase tokens seen on receipts/bank lines, learned from corrections).
- `clients` — name, contact, email, address, kvk, vat_id, country,
  is_business, payment_days (default 14).
- `invoices` — number (`YYYY-NNN`, gap-free, assigned when finalised),
  client_id, date, due_date, status (`concept`, `verzonden`, `betaald`),
  vat_scheme (`nl`, `verlegd_eu`, `export`), note, sent_at, paid_at.
- `invoice_lines` — invoice_id, description, quantity_milli, unit_cents,
  vat_rate.
- `bank_transactions` — date, amount_cents (+in / −out), counterparty,
  iban, description, import_hash (dedupe), matched_kind/matched_id,
  ignored (private).
- `assets` — name, purchase_date, cost_cents (excl. btw), useful_years,
  residual_cents, business_pct, document_id, disposed_date.
- `hours` — date, minutes, client_id, description.
- `trips` — date, from, to, km_tenths, purpose, client_id, return_trip.
- `periods` — kind (`btw_q`, `ib_year`), key (`2026-Q4`, `2026`), status
  (`open`, `ingediend`), filed_at, filed_numbers (JSON snapshot).
- `audit_log` — table, row id, action, before JSON, after JSON, at.
- `settings` — key/value: company details (from the NBC env), start date
  (for startersaftrek), starter years used, reminder e-mail.

A filed period locks every record dated inside it: the server refuses
changes and the app says to correct it in the next period, which is how the
Belastingdienst wants small corrections handled (suppletie above €1.000 is
flagged in the reminder).

## Btw-aangifte (quarterly)

Rubrieken computed for a quarter from confirmed documents and finalised
invoices (factuurstelsel: by invoice date):

- 1a leveringen/diensten 21% — base + btw
- 1b 9% — base + btw
- 1e 0% / niet bij u belast — base
- 2a verlegd naar u binnenland — not used yet (0)
- 3b leveringen naar/diensten in EU (B2B verlegd) — base
- 3a buiten EU — base
- 4a diensten uit landen buiten EU (reverse charge on costs, e.g. US SaaS) — base + 21% btw
- 4b uit EU-landen (e.g. Hetzner DE, Google IE) — base + 21% btw
- 5a verschuldigde btw = 1a+1b+4a+4b
- 5b voorbelasting = deductible btw on costs + 4a + 4b btw
- 5g totaal te betalen / terug

Deadline: last day of the month after the quarter. Reminders 14, 7 and 1
day(s) before, and on the day if not marked `ingediend`.

## Inkomstenbelasting (yearly)

Year overview, figures from a versioned tax table (`server/src/tax/2026.rs`;
one file per year so updating is a data change):

- omzet (excl. btw), kosten per ib_group (applying business_pct and
  deductible_pct), afschrijvingen (linear, pro rata by month, business_pct),
  km deduction (€0,23/km, 2026) for trips with own car → winst.
- urencriterium: sum of hours vs 1.225, progress through the year.
- ondernemersaftrek if urencriterium met: zelfstandigenaftrek €1.200
  (2026), startersaftrek €2.123 (max 3× in first 5 years, setting).
- MKB-winstvrijstelling 12,70% of winst after ondernemersaftrek.
- KIA: total investments ≥ €450 each; 28% of total when total between
  €2.901 and €71.683 (2026 table, full table encoded).
- belastbare winst, and an indication of box-1 tax at 35,70% / 37,56% /
  49,50% (2026 brackets, €38.883 / €79.137) clearly labelled "indicatie,
  zonder heffingskortingen en andere inkomsten".
- year-end balance for the aangifte: assets book value, open invoices
  (debiteuren), unpaid documents (crediteuren), btw still to pay.

Deadline 1 May (reminders from 1 March), voorlopige aanslag hint.

## Scanning

Phone: camera (or a file) → ML Kit text recognition (Latin script, on the
device, offline) → the text goes to the server's `parse.rs`, one rule-based
parser for every source (unit-tested):

- total: lines with `totaal|te betalen|...` beat `pin|betaald|...`; the
  largest such amount wins; `1.234,56` / `12,34` / `12.34` / `5,-`;
- date: `dd-mm-yyyy`, `dd/mm/yy`, `yyyy-mm-dd`, Dutch and English month
  names; the first one not in the future and not older than two years;
- btw: lines naming `21%`/`9%`/`hoog`/`laag` with base and btw (or incl and
  btw, or only btw); missing → 21% assumed and said so; reverse-charge
  wording or a foreign btw-id → `verlegd_eu`;
- supplier: KvK/btw-id/match terms of known suppliers, else the first line.

The draft opens as a filled form; the user confirms. Confirming teaches the
supplier its category and a match term. Offline, the photo and OCR text wait
in the app's outbox; the server parses them on arrival and the document stays
a concept for review.

Laptop (Linux, no ML Kit): PDF invoices are uploaded and the server reads the
text layer with `pdftotext -layout` into the same parser. Images on Linux are
attached without OCR (manual entry).

## Invoicing

Client list, invoice editor (lines, vat per line, scheme), preview. The
server renders the PDF (Typst-free: `printpdf`-style simple layout written
with `genpdf`) with: seller name, address, KvK, btw-id (once set), IBAN,
invoice number and date, client name/address (and client btw-id for EU
reverse charge with the text "Btw verlegd"), lines, subtotal per rate, btw,
total, payment term. Sending e-mails the PDF to the client via Brevo with a
copy to the owner. Overdue invoices (due date passed, not betaald) appear on
the dashboard and in the daily reminder.

Numbering: `YYYY-NNN`, assigned at finalising, never reused; concepts have
no number.

## Bank import

Upload a CSV (ING, Rabobank, ABN AMRO, Knab, bunq formats detected by
header) or CAMT.053 XML. Rows deduped by hash of (date, amount, iban,
description). Matching, deterministic:

- incoming: invoice total and number in description → invoice paid;
- outgoing: same amount ±€0,01 and date within ±10 days of a document, or
  supplier match_terms in counterparty → suggested match, user confirms;
- outgoing with no match after 14 days → "bonnetje ontbreekt" list.

## Reminders

- Phone: `flutter_local_notifications` scheduled for every btw and IB
  deadline of the next 12 months (deadlines are known in advance), plus a
  weekly "X bonnetjes nog bevestigen / Y zonder bonnetje" nudge when the app
  last synced it.
- Server: daily 08:00 Europe/Amsterdam job e-mails the owner on deadline
  days (14/7/1/0 before), overdue invoices, and on the first of each month a
  short "maandafsluiting" checklist (import bank, confirm bonnetjes).

## App screens

Overzicht (deadlines, quarter btw so far, year winst, todo counts) · Scannen
· Kosten (documents list + detail) · Facturen (+ klanten) · Bank · Uren ·
Ritten · Activa · Aangifte (btw quarter + IB year, "Markeer als ingediend")
· Instellingen (server, account, devices, company, categories, export).

Export: a zip of CSVs + all attachments per year, for an accountant or the
Belastingdienst.

## Testing

- Server: unit tests for btw rubrieken, IB calc, depreciation, KIA, parser,
  bank CSV detection and matching, invoice numbering, period locking; API
  tests through the router (`tower::ServiceExt::oneshot`).
- App: unit tests for money and duration parsing and invoice totals (parity
  with the server); Linux build and Android debug build must succeed.

## Build order

1. Server core: schema, auth, CRUD, attachments, audit, locking.
2. Tax: btw quarter, IB year, assets, KIA, tax tables.
3. Invoices + PDF + e-mail; bank import + matching; reminders job; PDF parse.
4. App: login, overzicht, kosten + scanner, facturen, bank, uren/ritten/activa,
   aangifte, instellingen, notifications, outbox.
5. Deploy files + README. Deploying to the Pi waits for the owner.
