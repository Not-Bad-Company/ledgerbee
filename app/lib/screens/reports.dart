// The aangiftes: the btw quarter in the form's own rubrieken, and the
// year's figures for the inkomstenbelasting.

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../api.dart';
import '../format.dart';
import '../theme.dart';
import '../widgets.dart';

class ReportsScreen extends StatelessWidget {
  const ReportsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 2,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Aangifte'),
          bottom: const TabBar(tabs: [Tab(text: 'Btw per kwartaal'), Tab(text: 'Inkomstenbelasting')]),
        ),
        body: const TabBarView(children: [_Btw(), _Ib()]),
      ),
    );
  }
}

List<String> _quarters() {
  final now = DateTime.now();
  final out = <String>[];
  var y = now.year, q = (now.month - 1) ~/ 3 + 1;
  while (out.length < 8 && (y > 2026 || (y == 2026 && q >= 4))) {
    out.add('$y-Q$q');
    q--;
    if (q == 0) {
      q = 4;
      y--;
    }
  }
  return out.isEmpty ? [quarterOf(now)] : out;
}

class _Btw extends StatefulWidget {
  const _Btw();

  @override
  State<_Btw> createState() => _BtwState();
}

class _BtwState extends State<_Btw> {
  late final quarters = _quarters();
  // The quarter to file is usually the one that just ended.
  late String key = quarters.length > 1 && DateTime.now().month % 3 == 1 ? quarters[1] : quarters.first;

  @override
  Widget build(BuildContext context) {
    return Loaded<Map>(
      key: ValueKey(key),
      load: () async => await Api.instance.get('/api/reports/btw/$key') as Map,
      builder: (context, r, reload) {
        final rubrieken = (r['rubrieken'] as List).cast<Map>();
        final filed = r['status'] == 'ingediend';
        final total = r['total_eur'] as int;
        return ListView(padding: const EdgeInsets.all(16), children: [
          Readable(
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              Row(children: [
                Expanded(
                  child: DropdownButtonFormField<String>(
                    initialValue: key,
                    decoration: const InputDecoration(labelText: 'Kwartaal'),
                    items: [for (final q in quarters) DropdownMenuItem(value: q, child: Text(q.replaceAll('-', ' ')))],
                    onChanged: (v) => setState(() => key = v!),
                  ),
                ),
                const SizedBox(width: 12),
                Text('Uiterlijk ${longDate(r['deadline'] as String)}'),
              ]),
              const SizedBox(height: 16),
              if (filed) Notice('Ingediend op ${shortDate((r['filed_at'] as String?)?.substring(0, 10))}. Dit kwartaal staat vast.', color: green, icon: Icons.check_circle_outline),
              for (final w in (r['warnings'] as List)) Notice(w as String),
              Section(
                title: 'Invullen in Mijn Belastingdienst Zakelijk (hele euro\'s)',
                child: Column(children: [
                  const _Head(),
                  for (final x in rubrieken)
                    if (!['1c', '1d', '2a', '3c'].contains(x['code']) || x['base_cents'] != 0)
                      _Rubriek(x, highlight: (x['code'] as String).startsWith('5')),
                ]),
              ),
              Card(
                color: (total > 0 ? red : green).withValues(alpha: 0.08),
                child: AmountRow(total >= 0 ? 'Te betalen' : 'Terug te vragen', total.abs() * 100, strong: true, hint: 'Exact: ${eur((r['total_cents'] as int).abs())}'),
              ),
              const SizedBox(height: 16),
              Wrap(spacing: 12, runSpacing: 12, children: [
                OutlinedButton.icon(
                  onPressed: () => launchUrl(Uri.parse('https://mijn.belastingdienst.nl/mbd-pmb/')),
                  icon: const Icon(Icons.open_in_new),
                  label: const Text('Mijn Belastingdienst Zakelijk'),
                ),
                if (!filed)
                  FilledButton.icon(
                    onPressed: () async {
                      if (!await confirm(context, 'Markeren als ingediend? De cijfers worden bewaard zoals ze nu zijn en het kwartaal gaat op slot.', ok: 'Ingediend')) return;
                      if (!context.mounted) return;
                      await guard(context, () => Api.instance.post('/api/periods/btw_q/$key/file'));
                      reload();
                    },
                    icon: const Icon(Icons.lock_outline),
                    label: const Text('Ingediend'),
                  )
                else
                  TextButton(
                    onPressed: () async {
                      if (!await confirm(context, 'Kwartaal heropenen? Alleen doen als je een suppletie indient.', ok: 'Heropenen')) return;
                      if (!context.mounted) return;
                      await guard(context, () => Api.instance.post('/api/periods/btw_q/$key/reopen'));
                      reload();
                    },
                    child: const Text('Heropenen'),
                  ),
              ]),
              const SizedBox(height: 16),
              Text(
                'Omzet en verschuldigde btw zijn naar beneden afgerond, voorbelasting naar boven: dat mag, in je eigen voordeel. '
                'Verlegde btw van buitenlandse leveranciers (rubriek 4) geef je aan én trek je in 5b weer af. Heb je achteraf een fout '
                'van meer dan € 1.000 btw, dien dan een suppletie in; kleinere fouten corrigeer je in de volgende aangifte.',
                style: Theme.of(context).textTheme.bodySmall,
              ),
              const SizedBox(height: 32),
            ]),
          ),
        ]);
      },
    );
  }
}

class _Head extends StatelessWidget {
  const _Head();

  @override
  Widget build(BuildContext context) {
    final s = Theme.of(context).textTheme.labelSmall;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
      child: Row(children: [
        Expanded(child: Text('Rubriek', style: s)),
        SizedBox(width: 90, child: Text('Omzet', style: s, textAlign: TextAlign.right)),
        SizedBox(width: 90, child: Text('Btw', style: s, textAlign: TextAlign.right)),
      ]),
    );
  }
}

class _Rubriek extends StatelessWidget {
  final Map x;
  final bool highlight;
  const _Rubriek(this.x, {this.highlight = false});

  @override
  Widget build(BuildContext context) {
    final st = amountStyle(context, weight: highlight ? FontWeight.w700 : FontWeight.w400);
    String whole(int v) => '€ ${number(v)}';
    return Tooltip(
      message: 'Exact: omzet ${eur(x['base_cents'] as int)}, btw ${eur(x['vat_cents'] as int)}',
      child: Container(
        color: highlight ? Theme.of(context).colorScheme.primary.withValues(alpha: 0.05) : null,
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          SizedBox(width: 34, child: Text(x['code'] as String, style: const TextStyle(fontWeight: FontWeight.w700))),
          Expanded(child: Text(x['label'] as String, style: Theme.of(context).textTheme.bodySmall)),
          SizedBox(width: 90, child: Text(x['has_base'] == true ? whole(x['base_eur'] as int) : '', style: st, textAlign: TextAlign.right)),
          SizedBox(width: 90, child: Text(whole(x['vat_eur'] as int), style: st, textAlign: TextAlign.right)),
        ]),
      ),
    );
  }
}

class _Ib extends StatefulWidget {
  const _Ib();

  @override
  State<_Ib> createState() => _IbState();
}

class _IbState extends State<_Ib> {
  // Until 1 May the year to file is last year's.
  int year = DateTime.now().month < 5 && DateTime.now().year > 2026 ? DateTime.now().year - 1 : DateTime.now().year;

  static const groups = {
    'kantoor': 'Kantoorkosten (software, hosting, telefoon, kleine hardware)',
    'verkoop': 'Verkoopkosten (marketing, representatie)',
    'auto': 'Vervoerskosten',
    'huisvesting': 'Huisvestingskosten',
    'overig': 'Andere kosten',
  };

  @override
  Widget build(BuildContext context) {
    final years = [for (var y = DateTime.now().year; y >= 2026; y--) y];
    return Loaded<Map>(
      key: ValueKey(year),
      load: () async => await Api.instance.get('/api/reports/ib/$year') as Map,
      builder: (context, r, reload) {
        final kosten = Map<String, dynamic>.from(r['kosten'] as Map);
        final filed = r['status'] == 'ingediend';
        return ListView(padding: const EdgeInsets.all(16), children: [
          Readable(
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              Row(children: [
                Expanded(
                  child: DropdownButtonFormField<int>(
                    initialValue: year,
                    decoration: const InputDecoration(labelText: 'Jaar'),
                    items: [for (final y in years) DropdownMenuItem(value: y, child: Text('$y'))],
                    onChanged: (v) => setState(() => year = v!),
                  ),
                ),
                const SizedBox(width: 12),
                Text('Uiterlijk ${longDate(r['deadline'] as String)}'),
              ]),
              const SizedBox(height: 16),
              if (filed) Notice('Ingediend. Dit jaar staat vast.', color: green, icon: Icons.check_circle_outline),
              for (final w in (r['warnings'] as List)) Notice(w as String),
              Section(
                title: 'Winst-en-verliesrekening',
                child: Column(children: [
                  AmountRow('Omzet (excl. btw)', r['omzet_cents'] as int),
                  for (final e in kosten.entries) AmountRow(groups[e.key] ?? e.key, -(e.value as int)),
                  AmountRow('Afschrijvingen', -(r['afschrijving_cents'] as int)),
                  AmountRow('Kilometers (${number((r['km'] as num).toDouble(), decimals: 1)} km × € 0,23)', -(r['km_cents'] as int)),
                  const Divider(),
                  AmountRow('Winst', r['winst_cents'] as int, strong: true),
                ]),
              ),
              Section(
                title: 'Van winst naar belastbare winst',
                child: Column(children: [
                  AmountRow('Kleinschaligheidsinvesteringsaftrek', -(r['kia_cents'] as int), hint: 'Investeringen dit jaar: ${eur(r['investeringen_cents'] as int)}'),
                  AmountRow('Zelfstandigenaftrek', -(r['zelfstandigenaftrek_cents'] as int)),
                  AmountRow('Startersaftrek', -(r['startersaftrek_cents'] as int)),
                  AmountRow('MKB-winstvrijstelling (12,70%)', -(r['mkb_vrijstelling_cents'] as int)),
                  const Divider(),
                  AmountRow('Belastbare winst uit onderneming', r['belastbare_winst_cents'] as int, strong: true),
                  AmountRow('Indicatie inkomstenbelasting', r['indicatie_box1_cents'] as int,
                      hint: 'Alleen box 1 over deze winst, zonder heffingskortingen, Zvw-bijdrage of ander inkomen'),
                ]),
              ),
              Section(
                title: 'Urencriterium',
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                    Text('${number((r['uren'] as num).toDouble(), decimals: 1)} van ${r['uren_nodig']} uur ${r['urencriterium'] == true ? '✓' : ''}'),
                    const SizedBox(height: 8),
                    LinearProgressIndicator(value: ((r['uren'] as num) / (r['uren_nodig'] as num)).clamp(0, 1).toDouble(), minHeight: 6),
                  ]),
                ),
              ),
              Section(
                title: 'Balans 31 december',
                child: Column(children: [
                  AmountRow('Bedrijfsmiddelen (boekwaarde)', r['balans']['activa_cents'] as int),
                  AmountRow('Debiteuren (openstaande facturen)', r['balans']['debiteuren_cents'] as int),
                ]),
              ),
              Wrap(spacing: 12, runSpacing: 12, children: [
                OutlinedButton.icon(
                  onPressed: () async {
                    final bytes = await guard(context, () => Api.instance.bytes('/api/export/$year'));
                    if (bytes == null || !context.mounted) return;
                    await guard(context, () => FilePicker.saveFile(fileName: 'administratie-$year.zip', bytes: bytes, mimeType: 'application/zip'));
                  },
                  icon: const Icon(Icons.archive_outlined),
                  label: const Text('Exporteer jaar (zip)'),
                ),
                if (!filed)
                  FilledButton.icon(
                    onPressed: () async {
                      if (!await confirm(context, 'Aangifte $year markeren als ingediend? Alles in $year gaat op slot.', ok: 'Ingediend')) return;
                      if (!context.mounted) return;
                      await guard(context, () => Api.instance.post('/api/periods/ib_year/$year/file'));
                      reload();
                    },
                    icon: const Icon(Icons.lock_outline),
                    label: const Text('Ingediend'),
                  ),
              ]),
              const SizedBox(height: 12),
              Text('Bedragen uit de belastingtabel ${r['table_year']}: ${r['table_source']}.', style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(height: 32),
            ]),
          ),
        ]);
      },
    );
  }
}
