pub mod btw;
pub mod ib;

pub struct TaxYear {
    pub year: i32,
    pub zelfstandigenaftrek: i64,
    pub startersaftrek: i64,
    pub mkb_bp: i64,
    pub uren: i64,
    pub km_cents: i64,
    pub kia_min: i64,
    pub kia_pct_until: i64,
    pub kia_pct: i64,
    pub kia_flat_until: i64,
    pub kia_flat: i64,
    pub kia_max: i64,
    pub kia_taper_bp: i64,
    pub kia_item_min: i64,
    pub box1: &'static [(i64, i64)],
    pub source: &'static str,
}

pub const YEARS: &[TaxYear] = &[TaxYear {
    year: 2026,
    zelfstandigenaftrek: 1_200_00,
    startersaftrek: 2_123_00,
    mkb_bp: 1270,
    uren: 1225,
    km_cents: 23,
    kia_min: 2_900_00,
    kia_pct_until: 71_683_00,
    kia_pct: 28,
    kia_flat_until: 132_746_00,
    kia_flat: 20_072_00,
    kia_max: 398_236_00,
    kia_taper_bp: 756,
    kia_item_min: 450_00,
    box1: &[(38_883_00, 3570), (79_137_00, 3756), (i64::MAX, 4950)],
    source: "Belastingplan 2026: zelfstandigenaftrek €1.200, startersaftrek €2.123, MKB-winstvrijstelling 12,70%, KIA-tabel 2026, box 1 35,70/37,56/49,50%",
}];

pub fn year(year: i32) -> (&'static TaxYear, bool) {
    match YEARS.iter().find(|t| t.year == year) {
        Some(t) => (t, true),
        None => (YEARS.iter().max_by_key(|t| t.year).expect("at least one tax year"), false),
    }
}

pub fn bp(amount: i64, bp: i64) -> i64 {
    let n = amount as i128 * bp as i128;
    let q = (n.abs() + 5000) / 10000;
    (if n < 0 { -q } else { q }) as i64
}

pub fn pct(amount: i64, pct: i64) -> i64 {
    bp(amount, pct * 100)
}

impl TaxYear {
    pub fn kia(&self, total: i64) -> i64 {
        if total <= self.kia_min || total > self.kia_max {
            0
        } else if total <= self.kia_pct_until {
            pct(total, self.kia_pct)
        } else if total <= self.kia_flat_until {
            self.kia_flat
        } else {
            self.kia_flat - bp(total - self.kia_flat_until, self.kia_taper_bp)
        }
    }

    pub fn box1_tax(&self, income: i64) -> i64 {
        let mut tax = 0;
        let mut lower = 0;
        for &(upper, rate) in self.box1 {
            if income <= lower {
                break;
            }
            tax += bp(income.min(upper) - lower, rate);
            lower = upper;
        }
        tax
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kia_follows_the_2026_table() {
        let (t, exact) = year(2026);
        assert!(exact);
        assert_eq!(t.kia(2_900_00), 0);
        assert_eq!(t.kia(2_901_00), 812_28);
        assert_eq!(t.kia(10_000_00), 2_800_00);
        assert_eq!(t.kia(100_000_00), 20_072_00);
        assert_eq!(t.kia(200_000_00), 20_072_00 - bp(200_000_00 - 132_746_00, 756));
        assert_eq!(t.kia(400_000_00), 0);
    }

    #[test]
    fn box1_is_progressive() {
        let (t, _) = year(2026);
        assert_eq!(t.box1_tax(10_000_00), 3_570_00);
        assert_eq!(t.box1_tax(50_000_00), bp(38_883_00, 3570) + bp(50_000_00 - 38_883_00, 3756));
        assert_eq!(t.box1_tax(0), 0);
    }

    #[test]
    fn rounding_is_half_away_from_zero() {
        assert_eq!(pct(5, 50), 3);
        assert_eq!(pct(-5, 50), -3);
        assert_eq!(pct(1000, 21), 210);
    }
}
