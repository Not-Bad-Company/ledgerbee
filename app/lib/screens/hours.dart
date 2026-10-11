import 'dart:async';

import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../api.dart';
import '../format.dart';
import '../theme.dart';
import '../widgets.dart';

int? parseDuration(String s) {
  s = s.trim().toLowerCase();
  final hm = RegExp(r'^(\d+):(\d{1,2})$').firstMatch(s);
  if (hm != null) return int.parse(hm[1]!) * 60 + int.parse(hm[2]!);
  if (s.endsWith('m')) return int.tryParse(s.substring(0, s.length - 1).trim());
  final h = double.tryParse(s.replaceAll(',', '.').replaceAll('u', '').trim());
  return h == null ? null : (h * 60).round();
}

class HoursScreen extends StatefulWidget {
  const HoursScreen({super.key});

  @override
  State<HoursScreen> createState() => _HoursScreenState();
}

class _HoursScreenState extends State<HoursScreen> {
  final _key = GlobalKey<LoadedState<(List<Map<String, dynamic>>, List<Map<String, dynamic>>)>>();
  DateTime? _timer;
  Timer? _tick;

  @override
  void initState() {
    super.initState();
    SharedPreferences.getInstance().then((p) {
      final s = p.getString('timer_start');
      if (s != null && mounted) {
        setState(() => _timer = DateTime.tryParse(s));
        _startTick();
      }
    });
  }

  @override
  void dispose() {
    _tick?.cancel();
    super.dispose();
  }

  void _startTick() {
    _tick?.cancel();
    _tick = Timer.periodic(const Duration(seconds: 30), (_) => mounted ? setState(() {}) : null);
  }

  Future<void> _toggleTimer(List<Map<String, dynamic>> clients) async {
    final p = await SharedPreferences.getInstance();
    if (_timer == null) {
      final now = DateTime.now();
      await p.setString('timer_start', now.toIso8601String());
      setState(() => _timer = now);
      _startTick();
      return;
    }
    final minutes = DateTime.now().difference(_timer!).inMinutes;
    final start = _timer!;
    await p.remove('timer_start');
    _tick?.cancel();
    setState(() => _timer = null);
    if (minutes < 1 || !mounted) return;
    await _edit({'date': iso(start), 'minutes': minutes}, clients);
  }

  Future<void> _edit(Map<String, dynamic> h, List<Map<String, dynamic>> clients) async {
    final changed = await showDialog<bool>(context: context, builder: (_) => _HourDialog(h: h, clients: clients));
    if (changed == true) _key.currentState?.reload();
  }

  @override
  Widget build(BuildContext context) {
    final year = DateTime.now().year;
    return Loaded<(List<Map<String, dynamic>>, List<Map<String, dynamic>>)>(
      key: _key,
      load: () async => (await Api.instance.list('hours', from: '$year-01-01', to: '$year-12-31'), await Api.instance.list('clients')),
      builder: (context, data, reload) {
        final (hours, clients) = data;
        final names = {for (final c in clients) c['id']: c['name']};
        final total = hours.fold<int>(0, (s, h) => s + (h['minutes'] as int));
        final weekStart = DateTime.now().subtract(Duration(days: DateTime.now().weekday - 1));
        final week = hours.where((h) => (h['date'] as String).compareTo(iso(weekStart)) >= 0).fold<int>(0, (s, h) => s + (h['minutes'] as int));
        final left = (1225 * 60 - total).clamp(0, 1225 * 60);
        final weeksLeft = (DateTime(year, 12, 31).difference(DateTime.now()).inDays + 1) / 7;
        final perWeek = weeksLeft <= 0 ? left / 60 : left / 60 / weeksLeft;
        return Scaffold(
          appBar: AppBar(title: const Text('Uren')),
          floatingActionButton: FloatingActionButton.extended(
            onPressed: () => _edit({'date': today()}, clients),
            icon: const Icon(Icons.add),
            label: const Text('Uren invoeren'),
          ),
          body: ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                      Text('$year: ${hoursText(total)} van 1.225 u', style: amountStyle(context, size: 18, weight: FontWeight.w700)),
                      const SizedBox(height: 8),
                      LinearProgressIndicator(value: (total / (1225 * 60)).clamp(0, 1), minHeight: 8, borderRadius: BorderRadius.circular(4)),
                      const SizedBox(height: 8),
                      Text(
                          'Deze week ${hoursText(week)}. '
                          '${left == 0 ? 'Urencriterium gehaald.' : 'Nog ${hoursText(left)} vóór 31 december, ongeveer ${number(perWeek)} u per week.'}',
                          style: Theme.of(context).textTheme.bodySmall),
                      if (left > 0 && perWeek > 60)
                        Padding(
                          padding: const EdgeInsets.only(top: 4),
                          child: Text(
                              'Dat haal je dit jaar niet meer met nieuwe uren. Uren die je dit jaar al aan de start van de zaak besteedde '
                              '(voorbereiding, website, inschrijving) tellen ook: zet ze erbij met hun datum.',
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(color: amber)),
                        ),
                      const SizedBox(height: 12),
                      FilledButton.tonalIcon(
                        onPressed: () => _toggleTimer(clients),
                        icon: Icon(_timer == null ? Icons.play_arrow : Icons.stop),
                        label: Text(_timer == null ? 'Timer starten' : 'Timer stoppen (${hoursText(DateTime.now().difference(_timer!).inMinutes)})'),
                      ),
                    ]),
                  ),
                ),
                const SizedBox(height: 16),
                if (hours.isNotEmpty)
                  Card(
                    child: Column(children: [
                      for (final h in hours)
                        ListTile(
                          title: Text((h['description'] as String).isEmpty ? (names[h['client_id']] ?? 'Werk') as String : h['description'] as String),
                          subtitle: Text([shortDate(h['date']), if (names[h['client_id']] != null) names[h['client_id']]].join(' · ')),
                          trailing: Text(hoursText(h['minutes'] as int), style: amountStyle(context)),
                          onTap: () => _edit(h, clients),
                        ),
                    ]),
                  ),
                const SizedBox(height: 72),
              ]),
            ),
          ]),
        );
      },
    );
  }
}

class _HourDialog extends StatefulWidget {
  final Map<String, dynamic> h;
  final List<Map<String, dynamic>> clients;
  const _HourDialog({required this.h, required this.clients});

  @override
  State<_HourDialog> createState() => _HourDialogState();
}

class _HourDialogState extends State<_HourDialog> {
  late final h = Map<String, dynamic>.of(widget.h);
  late final _dur = TextEditingController(text: h['minutes'] == null ? '' : hoursText(h['minutes'] as int).replaceAll(' u ', ':').replaceAll(' u', ''));
  late final _desc = TextEditingController(text: h['description'] as String? ?? '');

  Future<void> _save() async {
    final m = parseDuration(_dur.text);
    if (m == null || m <= 0) return showError(context, 'Vul de duur in, bijvoorbeeld 2,5 of 2:30');
    final ok = await guard(context, () => Api.instance.save('hours', {...h, 'minutes': m, 'description': _desc.text.trim()}..remove('updated_at')));
    if (ok != null && mounted) Navigator.pop(context, true);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: Text(h['created_at'] == null ? 'Uren' : 'Uren aanpassen'),
      content: SizedBox(
        width: 380,
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          DateField(label: 'Datum', value: h['date'] as String?, onChanged: (v) => setState(() => h['date'] = v)),
          const SizedBox(height: 12),
          TextField(controller: _dur, decoration: const InputDecoration(labelText: 'Duur (2,5 of 2:30)'), autofocus: h['minutes'] == null),
          const SizedBox(height: 12),
          DropdownButtonFormField<String?>(
            initialValue: h['client_id'] as String?,
            isExpanded: true,
            decoration: const InputDecoration(labelText: 'Klant (optioneel)'),
            items: [const DropdownMenuItem(value: null, child: Text('Geen / eigen werk')), for (final c in widget.clients) DropdownMenuItem(value: c['id'] as String, child: Text(c['name'] as String))],
            onChanged: (v) => setState(() => h['client_id'] = v),
          ),
          const SizedBox(height: 12),
          TextField(controller: _desc, decoration: const InputDecoration(labelText: 'Wat heb je gedaan?')),
        ]),
      ),
      actions: [
        if (h['created_at'] != null)
          TextButton(
            onPressed: () async {
              final ok = await guard(context, () async {
                await Api.instance.remove('hours', h['id'] as String);
                return true;
              });
              if (ok == true && context.mounted) Navigator.pop(context, true);
            },
            child: const Text('Verwijderen'),
          ),
        TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('Annuleren')),
        FilledButton(onPressed: _save, child: const Text('Bewaren')),
      ],
    );
  }
}
