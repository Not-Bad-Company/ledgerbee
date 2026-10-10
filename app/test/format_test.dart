import 'package:boekhouding/format.dart';
import 'package:boekhouding/screens/hours.dart';
import 'package:boekhouding/screens/invoices.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('amounts read back the way they are written', () {
    expect(eur(123456), '€ 1.234,56');
    expect(eur(-5), '−€ 0,05');
    expect(eur(100, sign: true), '+€ 1,00');
    expect(parseEur('12,34'), 1234);
    expect(parseEur('1.234,56'), 123456);
    expect(parseEur('12.34'), 1234);
    expect(parseEur('€ 5'), 500);
    expect(parseEur('-1,5'), -150);
    expect(parseEur('abc'), isNull);
    expect(parseEur(centsInput(98765)), 98765);
  });

  test('btw inside an amount', () {
    expect(vatFromIncl(12100, '21'), 2100);
    expect(vatFromIncl(1090, '9'), 90);
    expect(vatFromIncl(1000, 'verlegd_eu'), 0);
  });

  test('invoice totals match the server: btw per rate, not per line', () {
    final t = invoiceTotals('nl', [
      for (var i = 0; i < 3; i++) {'quantity_milli': 1000, 'unit_cents': 33, 'vat_rate': '21'},
      {'quantity_milli': 2000, 'unit_cents': 500, 'vat_rate': '9'},
    ]);
    expect(t.perRate['21'], (99, 21));
    expect(t.perRate['9'], (1000, 90));
    expect(t.incl, 99 + 21 + 1000 + 90);
    expect(invoiceTotals('verlegd_eu', [{'quantity_milli': 1000, 'unit_cents': 10000, 'vat_rate': '21'}]).vat, 0);
  });

  test('durations in every way people type them', () {
    expect(parseDuration('2,5'), 150);
    expect(parseDuration('2:30'), 150);
    expect(parseDuration('45m'), 45);
    expect(parseDuration('3 u'), 180);
    expect(parseDuration('x'), isNull);
  });

  test('dates', () {
    expect(shortDate('2026-10-06'), '6 okt 2026');
    expect(longDate('2026-10-06'), '6 oktober 2026');
    expect(quarterOf(DateTime(2026, 10, 6)), '2026-Q4');
    expect(count(1, 'bon', 'bonnen'), '1 bon');
    expect(count(2, 'bon', 'bonnen'), '2 bonnen');
  });
}
