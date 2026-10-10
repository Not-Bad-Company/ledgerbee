// Business kilometres with an own car: € 0,23 per km off the winst. The
// Belastingdienst wants a rittenregistratie: date, from, to, purpose, km.

import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../theme.dart';
import '../widgets.dart';

class TripsScreen extends StatefulWidget {
  const TripsScreen({super.key});

  @override
  State<TripsScreen> createState() => _TripsScreenState();
}

class _TripsScreenState extends State<TripsScreen> {
  final _key = GlobalKey<LoadedState<(List<Map<String, dynamic>>, List<Map<String, dynamic>>)>>();

  Future<void> _edit(Map<String, dynamic> t, List<Map<String, dynamic>> clients) async {
    final changed = await showDialog<bool>(context: context, builder: (_) => _TripDialog(t: t, clients: clients));
    if (changed == true) _key.currentState?.reload();
  }

  @override
  Widget build(BuildContext context) {
    final year = DateTime.now().year;
    return Loaded<(List<Map<String, dynamic>>, List<Map<String, dynamic>>)>(
      key: _key,
      load: () async => (await Api.instance.list('trips', from: '$year-01-01', to: '$year-12-31'), await Api.instance.list('clients')),
      builder: (context, data, reload) {
        final (trips, clients) = data;
        final km = trips.fold<int>(0, (s, t) => s + (t['km_tenths'] as int) * (t['return_trip'] == true ? 2 : 1)) / 10;
        return Scaffold(
          appBar: AppBar(title: const Text('Ritten')),
          floatingActionButton: FloatingActionButton.extended(
            onPressed: () => _edit({'date': today(), 'return_trip': true, 'origin': trips.isEmpty ? '' : trips.first['origin']}, clients),
            icon: const Icon(Icons.add),
            label: const Text('Rit toevoegen'),
          ),
          body: ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                Card(
                  child: AmountRow('$year: ${number(km, decimals: 1)} km', (km * 23).round(), hint: 'Aftrekbaar à € 0,23 per km (eigen auto, privé)'),
                ),
                const SizedBox(height: 16),
                if (trips.isEmpty)
                  const Padding(
                    padding: EdgeInsets.all(24),
                    child: Text('Rij je met je eigen auto voor het werk, leg dan elke rit vast: datum, van, naar, doel en kilometers.', textAlign: TextAlign.center),
                  )
                else
                  Card(
                    child: Column(children: [
                      for (final t in trips)
                        ListTile(
                          title: Text('${t['origin']} → ${t['destination']}${t['return_trip'] == true ? ' en terug' : ''}'),
                          subtitle: Text('${shortDate(t['date'])} · ${t['purpose']}'),
                          trailing: Text('${number((t['km_tenths'] as int) * (t['return_trip'] == true ? 2 : 1) / 10, decimals: 1)} km', style: amountStyle(context)),
                          onTap: () => _edit(t, clients),
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

class _TripDialog extends StatefulWidget {
  final Map<String, dynamic> t;
  final List<Map<String, dynamic>> clients;
  const _TripDialog({required this.t, required this.clients});

  @override
  State<_TripDialog> createState() => _TripDialogState();
}

class _TripDialogState extends State<_TripDialog> {
  late final t = Map<String, dynamic>.of(widget.t);
  late final _from = TextEditingController(text: t['origin'] as String? ?? '');
  late final _to = TextEditingController(text: t['destination'] as String? ?? '');
  late final _purpose = TextEditingController(text: t['purpose'] as String? ?? '');
  late final _km = TextEditingController(text: t['km_tenths'] == null ? '' : number((t['km_tenths'] as int) / 10, decimals: 1));

  Future<void> _save() async {
    final km = double.tryParse(_km.text.replaceAll(',', '.'));
    if (km == null || km <= 0) return showError(context, 'Vul de kilometers in');
    if (_purpose.text.trim().isEmpty) return showError(context, 'Het doel van de rit is verplicht voor de Belastingdienst');
    final body = {...t, 'origin': _from.text.trim(), 'destination': _to.text.trim(), 'purpose': _purpose.text.trim(), 'km_tenths': (km * 10).round()}..remove('updated_at');
    final ok = await guard(context, () => Api.instance.save('trips', body));
    if (ok != null && mounted) Navigator.pop(context, true);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Rit'),
      content: SizedBox(
        width: 400,
        child: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            DateField(label: 'Datum', value: t['date'] as String?, onChanged: (v) => setState(() => t['date'] = v)),
            const SizedBox(height: 12),
            TextField(controller: _from, decoration: const InputDecoration(labelText: 'Van')),
            const SizedBox(height: 12),
            TextField(controller: _to, decoration: const InputDecoration(labelText: 'Naar')),
            const SizedBox(height: 12),
            TextField(controller: _purpose, decoration: const InputDecoration(labelText: 'Zakelijk doel')),
            const SizedBox(height: 12),
            TextField(controller: _km, decoration: const InputDecoration(labelText: 'Kilometers (enkele reis)', suffixText: 'km'), keyboardType: const TextInputType.numberWithOptions(decimal: true)),
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Heen en terug'),
              value: t['return_trip'] == true,
              onChanged: (v) => setState(() => t['return_trip'] = v ?? false),
            ),
            DropdownButtonFormField<String?>(
              initialValue: t['client_id'] as String?,
              isExpanded: true,
              decoration: const InputDecoration(labelText: 'Klant (optioneel)'),
              items: [const DropdownMenuItem(value: null, child: Text('Geen')), for (final c in widget.clients) DropdownMenuItem(value: c['id'] as String, child: Text(c['name'] as String))],
              onChanged: (v) => setState(() => t['client_id'] = v),
            ),
          ]),
        ),
      ),
      actions: [
        if (t['created_at'] != null)
          TextButton(
            onPressed: () async {
              final ok = await guard(context, () async {
                await Api.instance.remove('trips', t['id'] as String);
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
