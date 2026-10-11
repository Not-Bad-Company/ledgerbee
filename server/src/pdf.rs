//! The invoice as a PDF, with every field the Belastingdienst requires on a
//! factuur (art. 35a Wet OB): seller and buyer, KvK, btw-id, a unique
//! number, dates, description, amounts per rate, the btw, and the reverse
//! charge wording when that applies.

use crate::config::Company;
use crate::invoices::{self, Totals};
use printpdf::{BuiltinFont, IndirectFontRef, Line, Mm, PdfDocument, PdfLayerReference, Point, Rect};
use serde_json::Value;

/// `123456` → `€ 1.234,56`.
pub fn eur(cents: i64) -> String {
    let neg = cents < 0;
    let c = cents.abs();
    let int = (c / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    format!("{}€ {},{:02}", if neg { "-" } else { "" }, grouped, c % 100)
}

pub fn nl_date(iso: &str) -> String {
    chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d")
        .map(|d| {
            const M: [&str; 12] =
                ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"];
            format!("{} {} {}", chrono::Datelike::day(&d), M[chrono::Datelike::month0(&d) as usize], chrono::Datelike::year(&d))
        })
        .unwrap_or_else(|_| iso.to_string())
}

fn qty(milli: i64) -> String {
    if milli % 1000 == 0 {
        (milli / 1000).to_string()
    } else {
        format!("{:.2}", milli as f64 / 1000.0).replace('.', ",")
    }
}

/// The EPC QR code payload ("girocode", EPC069-12 version 002): banking apps
/// that read it fill in the transfer by themselves. None when there is no
/// IBAN, nothing to pay, or the text would not fit the 331-byte limit.
pub fn epc_payload(name: &str, iban: &str, cents: i64, reference: &str) -> Option<String> {
    let iban: String = iban.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase();
    if iban.len() < 15 || cents <= 0 || cents > 99_999_999_999 {
        return None;
    }
    let name: String = name.chars().take(70).collect();
    let reference: String = reference.chars().take(140).collect();
    let text = format!("BCD\n002\n1\nSCT\n\n{name}\n{iban}\nEUR{}.{:02}\n\n\n{reference}", cents / 100, cents % 100);
    (text.len() <= 331).then_some(text)
}

/// Splits text into lines of at most `max` characters, at spaces.
fn wrap(text: &str, max: usize) -> Vec<String> {
    let mut out = vec![];
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > max {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    out.push(line);
    out
}

struct Page<'a> {
    layer: PdfLayerReference,
    regular: &'a IndirectFontRef,
    bold: &'a IndirectFontRef,
}

impl Page<'_> {
    fn text(&self, s: &str, size: f32, x: f32, y: f32, bold: bool) {
        self.layer.use_text(s, size, Mm(x), Mm(y), if bold { self.bold } else { self.regular });
    }
    /// Right-aligned at `x`, by Helvetica's average width; good enough for
    /// digits, which all have the same width.
    fn right(&self, s: &str, size: f32, x: f32, y: f32, bold: bool) {
        let w = s.chars().count() as f32 * size * 0.556 * 0.3528;
        self.text(s, size, x - w, y, bold);
    }
    fn rule(&self, y: f32) {
        self.layer.set_outline_thickness(0.4);
        self.layer.add_line(Line { points: vec![(Point::new(Mm(20.0), Mm(y)), false), (Point::new(Mm(190.0), Mm(y)), false)], is_closed: false });
    }
}

pub fn render(company: &Company, invoice: &Value, client: &Value) -> anyhow::Result<Vec<u8>> {
    let number = invoice["number"].as_str().unwrap_or("CONCEPT");
    let (doc, page, layer) = PdfDocument::new(format!("Factuur {number}"), Mm(210.0), Mm(297.0), "factuur");
    let regular = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;
    let p = Page { layer: doc.get_page(page).get_layer(layer), regular: &regular, bold: &bold };

    // Seller.
    p.text(&company.name, 18.0, 20.0, 270.0, true);
    let mut y = 270.0;
    for line in [
        company.street.clone(),
        format!("{} {}", company.postcode, company.city),
        company.email.clone(),
        if company.kvk.is_empty() { String::new() } else { format!("KvK {}", company.kvk) },
        if company.btw.is_empty() { String::new() } else { format!("Btw-id {}", company.btw) },
        if company.iban.is_empty() { String::new() } else { format!("IBAN {}", company.iban) },
    ]
    .into_iter()
    .filter(|l| !l.trim().is_empty())
    {
        y -= 5.0;
        p.right(&line, 9.0, 190.0, y, false);
    }

    // Buyer.
    let mut by = 240.0;
    p.text("Factuur aan", 8.0, 20.0, by, false);
    for line in [
        client["name"].as_str().unwrap_or("").to_string(),
        client["contact"].as_str().map(|c| if c.is_empty() { String::new() } else { format!("t.a.v. {c}") }).unwrap_or_default(),
        client["street"].as_str().unwrap_or("").to_string(),
        format!("{} {}", client["postcode"].as_str().unwrap_or(""), client["city"].as_str().unwrap_or("")),
        match client["country"].as_str() {
            Some("NL") | None | Some("") => String::new(),
            Some(c) => c.to_string(),
        },
        client["vat_id"].as_str().map(|v| if v.is_empty() { String::new() } else { format!("Btw-id {v}") }).unwrap_or_default(),
    ]
    .into_iter()
    .filter(|l| !l.trim().is_empty())
    {
        by -= 5.0;
        p.text(&line, 10.0, 20.0, by, false);
    }

    // Invoice facts.
    let is_credit = !invoice["credit_of"].as_str().unwrap_or("").is_empty();
    p.text(if is_credit { "Creditfactuur" } else { "Factuur" }, 22.0, 20.0, 195.0, true);
    let facts = [
        ("Factuurnummer", number.to_string()),
        ("Factuurdatum", nl_date(invoice["date"].as_str().unwrap_or(""))),
        ("Vervaldatum", nl_date(invoice["due_date"].as_str().unwrap_or(""))),
    ];
    let mut fy = 185.0;
    for (k, v) in facts {
        p.text(k, 9.0, 20.0, fy, false);
        p.text(&v, 9.0, 60.0, fy, true);
        fy -= 5.0;
    }

    // Lines.
    let mut ly = 160.0;
    p.text("Omschrijving", 9.0, 20.0, ly, true);
    p.right("Aantal", 9.0, 120.0, ly, true);
    p.right("Prijs", 9.0, 145.0, ly, true);
    p.right("Btw", 9.0, 160.0, ly, true);
    p.right("Bedrag", 9.0, 190.0, ly, true);
    p.rule(ly - 2.0);
    ly -= 8.0;
    let scheme = invoice["vat_scheme"].as_str().unwrap_or("nl");
    for l in invoice["lines"].as_array().into_iter().flatten() {
        let q = l["quantity_milli"].as_i64().unwrap_or(1000);
        let u = l["unit_cents"].as_i64().unwrap_or(0);
        let desc = l["description"].as_str().unwrap_or("");
        let mut first = true;
        for text in wrap(desc, 55) {
            p.text(&text, 10.0, 20.0, ly, false);
            if first {
                p.right(&qty(q), 10.0, 120.0, ly, false);
                p.right(&eur(u), 10.0, 145.0, ly, false);
                p.right(&if scheme == "nl" { format!("{}%", l["vat_rate"].as_str().unwrap_or("21")) } else { "-".into() }, 10.0, 160.0, ly, false);
                p.right(&eur(invoices::line_amount(q, u)), 10.0, 190.0, ly, false);
                first = false;
            }
            ly -= 6.0;
        }
    }
    p.rule(ly + 2.0);
    ly -= 4.0;

    let t: Totals = invoices::totals_of(invoice);
    p.right("Subtotaal", 10.0, 160.0, ly, false);
    p.right(&eur(t.excl_cents), 10.0, 190.0, ly, false);
    for r in &t.per_rate {
        if scheme != "nl" {
            break;
        }
        ly -= 6.0;
        p.right(&format!("Btw {}% over {}", r.rate, eur(r.base_cents)), 10.0, 160.0, ly, false);
        p.right(&eur(r.vat_cents), 10.0, 190.0, ly, false);
    }
    ly -= 8.0;
    p.right("Totaal", 12.0, 160.0, ly, true);
    p.right(&eur(t.incl_cents), 12.0, 190.0, ly, true);

    ly -= 14.0;
    let notice = match scheme {
        "verlegd_eu" => Some("Btw verlegd naar de afnemer (artikel 196 Btw-richtlijn)."),
        "export" => Some("Niet in Nederland belast; de afnemer is buiten de EU gevestigd."),
        _ => None,
    };
    if let Some(n) = notice {
        p.text(n, 9.0, 20.0, ly, true);
        ly -= 6.0;
    }
    let pay = if t.incl_cents < 0 {
        format!("Dit bedrag van {} wordt verrekend met een openstaande factuur of aan je teruggestort.", eur(-t.incl_cents))
    } else if company.iban.is_empty() {
        format!("Graag het bedrag van {} vóór {} overmaken, onder vermelding van {}.", eur(t.incl_cents), nl_date(invoice["due_date"].as_str().unwrap_or("")), number)
    } else {
        format!(
            "Graag het bedrag van {} vóór {} overmaken naar {} t.n.v. {}, onder vermelding van {}.",
            eur(t.incl_cents),
            nl_date(invoice["due_date"].as_str().unwrap_or("")),
            company.iban,
            if company.owner.is_empty() { &company.name } else { &company.owner },
            number
        )
    };
    for line in wrap(&pay, 95) {
        p.text(&line, 9.0, 20.0, ly, false);
        ly -= 4.5;
    }
    if let Some(note) = invoice["note"].as_str().filter(|n| !n.is_empty()) {
        ly -= 2.0;
        for line in wrap(note, 95) {
            p.text(&line, 9.0, 20.0, ly, false);
            ly -= 4.5;
        }
    }
    // Scan to pay, bottom right.
    let payee = if company.owner.is_empty() { &company.name } else { &company.owner };
    if let Some(payload) = epc_payload(payee, &company.iban, t.incl_cents, &format!("Factuur {number}")) {
        if let Ok(code) = qrcode::QrCode::with_error_correction_level(payload.as_bytes(), qrcode::EcLevel::M) {
            let n = code.width();
            let size = 30.0_f32;
            let cell = size / n as f32;
            let (x0, y0) = (190.0 - size, 22.0);
            for (i, c) in code.to_colors().iter().enumerate() {
                if *c == qrcode::Color::Dark {
                    let (col, row) = ((i % n) as f32, (i / n) as f32);
                    let x = x0 + col * cell;
                    let y = y0 + size - (row + 1.0) * cell;
                    p.layer.add_rect(Rect::new(Mm(x), Mm(y), Mm(x + cell), Mm(y + cell)));
                }
            }
            p.right("Scan om te betalen", 7.5, 190.0, y0 + size + 3.0, false);
        }
    }
    p.text(&format!("{} · KvK {} · {}", company.name, company.kvk, company.email), 7.5, 20.0, 12.0, false);

    let mut out = std::io::BufWriter::new(Vec::new());
    doc.save(&mut out)?;
    Ok(out.into_inner()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_epc_code_follows_the_standard() {
        let t = epc_payload("Daan Hessen", "NL91 ABNA 0417 1643 00", 246840, "Factuur 2026-001").unwrap();
        assert_eq!(t, "BCD\n002\n1\nSCT\n\nDaan Hessen\nNL91ABNA0417164300\nEUR2468.40\n\n\nFactuur 2026-001");
        assert!(epc_payload("x", "", 100, "r").is_none(), "no IBAN, no code");
        assert!(epc_payload("x", "NL91ABNA0417164300", 0, "r").is_none(), "nothing to pay");
    }

    #[test]
    fn money_and_dates_read_dutch() {
        assert_eq!(eur(123456), "€ 1.234,56");
        assert_eq!(eur(5), "€ 0,05");
        assert_eq!(eur(-100000), "-€ 1.000,00");
        assert_eq!(nl_date("2026-10-06"), "6 oktober 2026");
    }

    #[test]
    fn an_invoice_renders() {
        let c = Company { name: "Not Bad Company".into(), kvk: "42185336".into(), ..Default::default() };
        let inv = json!({"number": "2026-001", "date": "2026-10-06", "due_date": "2026-10-20", "vat_scheme": "nl",
            "lines": [{"description": "Website ontwerp en bouw", "quantity_milli": 1000, "unit_cents": 125000, "vat_rate": "21"}]});
        let pdf = render(&c, &inv, &json!({"name": "Klant BV", "city": "Hilversum"})).unwrap();
        assert!(pdf.starts_with(b"%PDF"));
    }
}
