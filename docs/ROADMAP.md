# Goudgulden roadmap

What it takes to be a free, self-hosted alternative to Moneybird, e-Boekhouden
and Jortt for one eenmanszaak. Ordered by what matters most; sources in
[research/market-2026-10.md](research/market-2026-10.md).

## Now

- [x] Share PDFs and photos to the app from any other app
- [x] Company details editable in the app instead of the server's `.env`
- [x] **Btw check before filing:** tap a rubriek to see the invoices and
      costs behind it
- [x] **First run:** ask for the company details and IBAN before anything else
- [ ] **Quarter checklist:** what's still missing for this quarter (unconfirmed
      costs, payments without a bon, concept invoices), in one place
- [x] **Invoices people pay faster:** payment reference, an EPC QR code
      ("scan to pay") on the PDF, and a reminder e-mail for overdue invoices

## Next

- [ ] Quotes (offertes) that become an invoice in one tap
- [ ] Recurring invoices (monthly retainer, hosting)
- [ ] Receipts by e-mail: an IMAP mailbox the server reads, so forwarding a
      receipt is enough
- [ ] Credit invoices (creditfacturen) for corrections
- [ ] UBL 2.1 / NLCIUS export of sales invoices (Peppol-ready, no transmission)
- [ ] Web version of the app, served by the server itself

## Later

- [ ] Bank feed through a PSD2 provider, for whoever has an account
- [ ] KOR (kleineondernemersregeling) as a setting
- [ ] Multi-currency invoices at ECB rates

## Not planned

- Filing directly with the Belastingdienst (Digipoort/SBR needs certification)
- Hosting other people's administration (see the license)
