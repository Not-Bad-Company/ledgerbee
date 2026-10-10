// The first screen: what is due, what is waiting, and how the year goes.

import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../main.dart';
import '../theme.dart';
import '../widgets.dart';
import 'costs.dart';

class OverviewScreen extends StatelessWidget {
  const OverviewScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(toolbarHeight: 8),
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
                if ((company['btw'] as String).isEmpty)
                  const Notice('Het btw-identificatienummer ontbreekt nog. Facturen moeten het vermelden: zet NBC_BTW in de gedeelde .env zodra de '
                      'Belastingdienst het stuurt, en herstart de server.'),
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
    final h = DateTime.now().hour;
    return h < 6 ? 'Goedenacht' : h < 12 ? 'Goedemorgen' : h < 18 ? 'Goedemiddag' : 'Goedenavond';
  }

  static String _summary(int tasks) => tasks == 0 ? 'Alles is bij. Niets dat op je wacht.' : tasks == 1 ? 'Eén ding wacht op je.' : '$tasks dingen wachten op je.';
}

/// The next deadline, drawn as the blue envelope it stands for.
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
    const onEnvelope = Colors.white;
    final soft = Colors.white.withValues(alpha: 0.72);
    return Material(
      color: envelope,
      borderRadius: BorderRadius.circular(20),
      clipBehavior: Clip.antiAlias,
      child: InkWell(
        onTap: () => currentSection.value = sectionIndex('Aangifte'),
        child: CustomPaint(
          painter: _FlapPainter(),
          child: Padding(
            padding: const EdgeInsets.fromLTRB(20, 18, 20, 20),
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Text(deadline['title'] as String, style: t.titleMedium?.copyWith(color: onEnvelope, fontWeight: FontWeight.w700)),
              Text('Uiterlijk ${longDate(deadline['due'] as String)}', style: t.bodyMedium?.copyWith(color: soft)),
              const SizedBox(height: 28),
              Row(crossAxisAlignment: CrossAxisAlignment.end, children: [
                Expanded(
                  child: Text.rich(
                    TextSpan(children: [
                      TextSpan(text: days <= 0 ? (days == 0 ? 'Vandaag' : 'Verlopen') : '$days', style: amountStyle(context, size: 56, weight: FontWeight.w800, color: onEnvelope).copyWith(height: 1, letterSpacing: -2)),
                      if (days > 0) TextSpan(text: days == 1 ? '  dag' : '  dagen', style: t.titleMedium?.copyWith(color: soft)),
                    ]),
                  ),
                ),
                if (btw)
                  Column(crossAxisAlignment: CrossAxisAlignment.end, children: [
                    Text(total >= 0 ? 'Te betalen' : 'Terug', style: t.bodySmall?.copyWith(color: soft)),
                    Text(eur(total.abs()), style: amountStyle(context, size: 20, weight: FontWeight.w700, color: onEnvelope)),
                  ]),
              ]),
              if (btw && (quarter['warnings'] as List).isNotEmpty) ...[
                const SizedBox(height: 12),
                for (final w in (quarter['warnings'] as List)) Text(w as String, style: t.bodySmall?.copyWith(color: soft)),
              ],
            ]),
          ),
        ),
      ),
    );
  }
}

class _FlapPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final flap = Path()
      ..moveTo(0, 0)
      ..lineTo(size.width / 2, size.height * 0.42)
      ..lineTo(size.width, 0)
      ..close();
    canvas.drawPath(flap, Paint()..color = Colors.white.withValues(alpha: 0.06));
    canvas.drawPath(
      Path()
        ..moveTo(0, 0)
        ..lineTo(size.width / 2, size.height * 0.42)
        ..lineTo(size.width, 0),
      Paint()
        ..color = Colors.white.withValues(alpha: 0.14)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1,
    );
  }

  @override
  bool shouldRepaint(_FlapPainter old) => false;
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
