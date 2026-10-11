import 'package:intl/intl.dart';

String eur(num? cents, {bool sign = false}) {
  final c = (cents ?? 0).round();
  final neg = c < 0;
  final abs = c.abs();
  final whole = NumberFormat('#,##0', 'en_US').format(abs ~/ 100).replaceAll(',', '.');
  final s = '€ $whole,${(abs % 100).toString().padLeft(2, '0')}';
  if (neg) return '−$s';
  return sign && c > 0 ? '+$s' : s;
}

int? parseEur(String input) {
  var s = input.replaceAll(RegExp(r'[€\s]'), '').replaceAll('−', '-');
  if (s.isEmpty) return null;
  final neg = s.startsWith('-');
  s = s.replaceFirst(RegExp(r'^[-+]'), '');
  final comma = s.lastIndexOf(',');
  final dot = s.lastIndexOf('.');
  String intPart, frac;
  if (comma > dot) {
    intPart = s.substring(0, comma).replaceAll('.', '');
    frac = s.substring(comma + 1);
  } else if (dot > comma && s.length - dot - 1 <= 2) {
    intPart = s.substring(0, dot).replaceAll(',', '');
    frac = s.substring(dot + 1);
  } else {
    intPart = s.replaceAll(RegExp(r'[.,]'), '');
    frac = '';
  }
  if (frac.length > 2 || !RegExp(r'^\d*$').hasMatch(intPart + frac)) return null;
  final v = (int.tryParse(intPart.isEmpty ? '0' : intPart) ?? 0) * 100 + int.parse(frac.padRight(2, '0'));
  return neg ? -v : v;
}

String centsInput(num? cents) {
  final c = (cents ?? 0).round();
  return '${c < 0 ? '-' : ''}${c.abs() ~/ 100},${(c.abs() % 100).toString().padLeft(2, '0')}';
}

const _months = ['jan', 'feb', 'mrt', 'apr', 'mei', 'jun', 'jul', 'aug', 'sep', 'okt', 'nov', 'dec'];
const _monthsLong = ['januari', 'februari', 'maart', 'april', 'mei', 'juni', 'juli', 'augustus', 'september', 'oktober', 'november', 'december'];

DateTime? parseIso(String? s) => s == null || s.isEmpty ? null : DateTime.tryParse(s);

String iso(DateTime d) => '${d.year}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

String today() => iso(DateTime.now());

String shortDate(String? s) {
  final d = parseIso(s);
  return d == null ? '—' : '${d.day} ${_months[d.month - 1]} ${d.year}';
}

String longDate(String? s) {
  final d = parseIso(s);
  return d == null ? '—' : '${d.day} ${_monthsLong[d.month - 1]} ${d.year}';
}

String quarterOf(DateTime d) => '${d.year}-Q${(d.month - 1) ~/ 3 + 1}';

String hoursText(num minutes) {
  final h = minutes ~/ 60;
  final m = (minutes % 60).round();
  return m == 0 ? '$h u' : '$h u ${m.toString().padLeft(2, '0')}';
}

String number(num v, {int decimals = 0}) =>
    NumberFormat.decimalPatternDigits(locale: 'en_US', decimalDigits: decimals).format(v).replaceAll(',', '#').replaceAll('.', ',').replaceAll('#', '.');

const vatRates = <String, String>{
  '21': '21%',
  '9': '9%',
  '0': '0%',
  'verlegd_eu': 'Verlegd (EU)',
  'verlegd_buiten_eu': 'Verlegd (buiten EU)',
  'vrijgesteld': 'Vrijgesteld',
  'geen': 'Geen btw',
};

int vatFromIncl(int incl, String rate) {
  final r = int.tryParse(rate);
  if (r == null || r == 0) return 0;
  final base = (incl * 100 / (100 + r)).round();
  return incl - base;
}

String count(int n, String one, String many) => '$n ${n == 1 ? one : many}';
