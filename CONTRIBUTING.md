# Contributing

Bug reports and ideas are welcome as GitHub issues. Wrong tax figures are the
most useful kind: say which rubriek or line, what you expected, and why
(a link to the Belastingdienst page helps).

## Pull requests

Before you open one, know how the licensing works. The code is published
under the Elastic License 2.0, and the copyright holder (Daan Hessen, trading
as Not Bad Company) may offer future versions under other terms, including
closed-source or commercial ones. To keep that possible, every contribution
comes with this grant:

> By submitting a contribution, I confirm that I wrote it or have the right
> to submit it, and I grant Daan Hessen a perpetual, worldwide,
> non-exclusive, royalty-free, irrevocable licence to use, copy, modify,
> sublicense and distribute it under any terms, including the Elastic
> License 2.0 and proprietary licences. I keep the copyright in my work.

If you don't agree, please open an issue instead of a pull request.

Practical notes:

- `cd server && cargo test` and `cd app && flutter analyze && flutter test`
  must pass.
- Tax rules belong in `server/src/tax/`, never in the app. The app only shows
  what the server computes.
- Money is in integer cents everywhere. No floats for amounts.
- User-facing text is Dutch; code, comments and commits are English.
