import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../main.dart';
import '../theme.dart';
import '../widgets.dart';
import 'costs.dart';
import 'settings.dart';

class OverviewScreen extends StatelessWidget {
  const OverviewScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(toolbarHeight: 8, shape: const Border()),
      floatingActionButton: const ScanButton(),
      body: Loaded<Map>(
        load: () async => await Api.instance.get('/api/overview') as Map,
        builder: (context, o, reload) {
          final todo = o['todo'] as Map;
          final year = o['year'] as Map;
          final quarter = o['quarter'] as Map;
          final company = o['company'] as Map;
          final deadlines = (o['deadlines'] as List).cast<Map>().where((d) => d['filed'] != true).toList();
          final overdue = (todo['overdue_invoices'] as List).cast<Map>();
          final uren = (year['uren'] as num).toDouble();
          final nodig = (year['uren_nodig'] as num).toDouble();

          final tasks = <Widget>[
            if ((todo['concept_documents'] as int) > 0)
              _Task(Icons.receipt_long, '${count(todo['concept_documents'] as int, 'bon', 'bonnen')} nog bevestigen', 'Kosten'),
            if ((todo['missing_receipts'] as int) > 0)
              _Task(Icons.help_outline, '${count(todo['missing_receipts'] as int, 'betaling', 'betalingen')} zonder bonnetje', 'Bank'),
            if ((todo['bank_suggestions'] as int) > 0)
              _Task(Icons.link, '${count(todo['bank_suggestions'] as int, 'banktransactie', 'banktransacties')} te koppelen', 'Bank'),
            for (final inv in overdue)
              _Task(Icons.warning_amber, 'Factuur ${inv['number']} aan ${inv['client'] ?? ''} is over de vervaldatum (${shortDate(inv['due_date'])})', 'Facturen',
                  color: red),
            if ((todo['concept_invoices'] as int) > 0)
              _Task(Icons.edit_note, count(todo['concept_invoices'] as int, 'conceptfactuur', 'conceptfacturen'), 'Facturen'),
            if (todo['last_bank_import'] == null) const _Task(Icons.upload_file, 'Nog geen bankafschrift geïmporteerd', 'Bank'),
          ];

          final next = deadlines.isEmpty ? null : deadlines.first;
          final owner = (company['owner'] as String? ?? '').trim();
          final first = owner.isEmpty ? '' : owner.split(' ').first;

          return ListView(padding: const EdgeInsets.fromLTRB(16, 8, 16, 16), children: [
            Readable(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(4, 4, 4, 20),
                  child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                    Text(first.isEmpty ? _greeting() : '${_greeting()}, $first', style: Theme.of(context).textTheme.headlineMedium),
                    const SizedBox(height: 4),
                    Text(_summary(tasks.length), style: Theme.of(context).textTheme.bodyLarge?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                  ]),
                ),
                if ((company['name'] as String).isEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 20),
                    child: Card(
                      child: Padding(
                        padding: const EdgeInsets.all(20),
                        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                          Text('Eerst je bedrijf', style: labelStyle(color: accent)),
                          const SizedBox(height: 6),
                          Text('Naam, adres, KvK, btw-id en IBAN', style: Theme.of(context).textTheme.titleLarge),
                          const SizedBox(height: 6),
                          Text('Ze komen op elke factuur. Daarna kun je meteen bonnen scannen en factureren.', style: Theme.of(context).textTheme.bodyMedium?.copyWith(color: muted)),
                          const SizedBox(height: 16),
                          FilledButton(
                            onPressed: () async {
                              await SettingsScreen.editCompany(context, company);
                              reload();
                            },
                            child: const Text('Gegevens invullen'),
                          ),
                        ]),
                      ),
                    ),
                  ),
                if ((company['name'] as String).isNotEmpty && (company['btw'] as String).isEmpty)
                  const Notice('Het btw-identificatienummer ontbreekt nog. Facturen moeten het vermelden: vul het in bij Instellingen → Onderneming '
                      'zodra de Belastingdienst het stuurt.'),
                if (next != null) Padding(padding: const EdgeInsets.only(bottom: 20), child: _Envelope(deadline: next, quarter: quarter)),
                if (deadlines.length > 1)
                  Section(
                    title: 'Daarna',
                    child: Column(children: [
                      for (final d in deadlines.skip(1).take(3))
                        ListTile(
                          leading: Icon(Icons.event_outlined, color: (d['days_left'] as int) <= 14 ? red : null),
                          title: Text(d['title'] as String),
                          subtitle: Text('Uiterlijk ${longDate(d['due'] as String)}'),
                          trailing: Text(_left(d['days_left'] as int), style: amountStyle(context, size: 14, color: (d['days_left'] as int) <= 14 ? red : null)),
                          onTap: () => currentSection.value = sectionIndex('Aangifte'),
                        ),
                    ]),
                  ),
                if (tasks.isNotEmpty) Section(title: 'Te doen', child: Column(children: tasks)),
                Section(
                  title: '${year['year']}',
                  child: Column(children: [
                    AmountRow('Omzet', year['omzet_cents'] as int),
                    AmountRow('Kosten, afschrijving, kilometers', -(year['kosten_cents'] as int)),
                    const Divider(),
                    AmountRow('Winst', year['winst_cents'] as int, strong: true),
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
                      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                        Text('Urencriterium: ${number(uren, decimals: 1)} van ${number(nodig)} uur'),
                        const SizedBox(height: 6),
                        LinearProgressIndicator(value: (uren / nodig).clamp(0, 1), minHeight: 6, borderRadius: BorderRadius.circular(3)),
                        const SizedBox(height: 6),
                        Text('${number((year['km'] as num).toDouble(), decimals: 1)} zakelijke kilometers', style: Theme.of(context).textTheme.bodySmall),
                      ]),
                    ),
                  ]),
                ),
                const SizedBox(height: 72),
              ]),
            ),
          ]);
        },
      ),
    );
  }

  static String _left(int days) => days < 0 ? 'verlopen' : days == 0 ? 'vandaag' : days == 1 ? 'morgen' : '$days dagen';

  static String _greeting() {
    const forced = int.fromEnvironment('BOEK_HOUR', defaultValue: -1);
    final h = kDebugMode && forced >= 0 ? forced : DateTime.now().hour;
    return h < 6 ? 'Goedenacht' : h < 12 ? 'Goedemorgen' : h < 18 ? 'Goedemiddag' : 'Goedenavond';
  }

  static String _summary(int tasks) => tasks == 0 ? 'Alles is bij. Niets dat op je wacht.' : tasks == 1 ? 'Eén ding wacht op je.' : '$tasks dingen wachten op je.';
}

class _Envelope extends StatelessWidget {
  final Map deadline;
  final Map quarter;
  const _Envelope({required this.deadline, required this.quarter});

  @override
  Widget build(BuildContext context) {
    final days = deadline['days_left'] as int;
    final btw = deadline['kind'] == 'btw_q';
    final total = quarter['total_cents'] as int;
    final t = Theme.of(context).textTheme;
    final urgent = days <= 14;
    return Material(
      color: surface,
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(2)), side: BorderSide(color: rule)),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => currentSection.value = sectionIndex('Aangifte'),
        child: Container(
          decoration: BoxDecoration(border: Border(top: BorderSide(color: urgent ? accent : Colors.transparent, width: 2))),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(20, 18, 20, 20),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text('Volgende deadline', style: labelStyle(color: urgent ? accent : muted)),
              const SizedBox(height: 6),
              Text(deadline['title'] as String, style: t.titleLarge),
              Text('Uiterlijk ${longDate(deadline['due'] as String)}', style: t.bodyMedium?.copyWith(color: muted)),
              const SizedBox(height: 32),
              Row(crossAxisAlignment: CrossAxisAlignment.end, children: [
                Expanded(
                  child: Text.rich(
                    TextSpan(children: [
                      TextSpan(text: days <= 0 ? (days == 0 ? 'Vandaag' : 'Verlopen') : '$days', style: figureStyle(size: 64, color: urgent ? accent : fg)),
                      if (days > 0) TextSpan(text: days == 1 ? ' dag' : ' dagen', style: labelStyle(size: 14)),
                    ]),
                  ),
                ),
                if (btw)
                  Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
                    Text(total >= 0 ? 'Te betalen' : 'Terug', style: labelStyle()),
                    const SizedBox(height: 2),
                    Text(eur(total.abs()), style: figureStyle(size: 24)),
                  ]),
              ]),
              if (btw && (quarter['warnings'] as List).isNotEmpty) ...[
                const SizedBox(height: 16),
                const Divider(),
                const SizedBox(height: 10),
                for (final w in (quarter['warnings'] as List)) Text(w as String, style: t.bodySmall),
              ],
            ]),
          ),
        ),
      ),
    );
  }
}

class _Task extends StatelessWidget {
  final IconData icon;
  final String text;
  final String section;
  final Color? color;
  const _Task(this.icon, this.text, this.section, {this.color});

  @override
  Widget build(BuildContext context) => ListTile(
        leading: Icon(icon, color: color),
        title: Text(text),
        trailing: const Icon(Icons.chevron_right),
        onTap: () => currentSection.value = sectionIndex(section),
      );
}
