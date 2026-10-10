// Business assets of € 450 or more (excl. btw): depreciated over their
// useful life instead of being a cost at once, and counted for the KIA.

import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../theme.dart';
import '../widgets.dart';

class AssetsScreen extends StatefulWidget {
  const AssetsScreen({super.key});

  @override
  State<AssetsScreen> createState() => _AssetsScreenState();
}

class _AssetsScreenState extends State<AssetsScreen> {
  final _key = GlobalKey<LoadedState<List<Map<String, dynamic>>>>();

  Future<void> _edit(Map<String, dynamic> a) async {
    final changed = await showDialog<bool>(context: context, builder: (_) => _AssetDialog(a: a));
    if (changed == true) _key.currentState?.reload();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Activa')),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => _edit({'purchase_date': today(), 'useful_years': 5, 'residual_cents': 0, 'business_pct': 100}),
        icon: const Icon(Icons.add),
        label: const Text('Bedrijfsmiddel'),
      ),
      body: Loaded<List<Map<String, dynamic>>>(
        key: _key,
        load: () => Api.instance.list('assets'),
        builder: (context, assets, reload) {
          if (assets.isEmpty) {
            return const EmptyState(
              icon: Icons.devices_outlined,
              text: 'Koop je iets voor de zaak van € 450 of meer (excl. btw), zoals een laptop, zet het dan hier. '
                  'Het wordt over een paar jaar afgeschreven, en telt mee voor de investeringsaftrek (KIA) vanaf € 2.901 per jaar. '
                  'De btw trek je wel meteen af via de bon bij Kosten (categorie Investering).',
            );
          }
          return ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Card(
                child: Column(children: [
                  for (final a in assets)
                    ListTile(
                      title: Text(a['name'] as String),
                      subtitle: Text('${shortDate(a['purchase_date'])} · ${a['useful_years']} jaar${(a['business_pct'] as int) < 100 ? ' · ${a['business_pct']}% zakelijk' : ''}'
                          '${a['disposed_date'] != null ? ' · verkocht ${shortDate(a['disposed_date'])}' : ''}'),
                      trailing: Text(eur(a['cost_cents'] as int), style: amountStyle(context)),
                      onTap: () => _edit(a),
                    ),
                ]),
              ),
            ),
          ]);
        },
      ),
    );
  }
}

class _AssetDialog extends StatefulWidget {
  final Map<String, dynamic> a;
  const _AssetDialog({required this.a});

  @override
  State<_AssetDialog> createState() => _AssetDialogState();
}

class _AssetDialogState extends State<_AssetDialog> {
  late final a = Map<String, dynamic>.of(widget.a);
  late final _name = TextEditingController(text: a['name'] as String? ?? '');
  late final _cost = TextEditingController(text: a['cost_cents'] == null ? '' : centsInput(a['cost_cents'] as int));
  late final _residual = TextEditingController(text: centsInput(a['residual_cents'] as int? ?? 0));

  Future<void> _save() async {
    final cost = parseEur(_cost.text);
    if (_name.text.trim().isEmpty || cost == null || cost <= 0) return showError(context, 'Naam en aanschafprijs zijn verplicht');
    final body = {...a, 'name': _name.text.trim(), 'cost_cents': cost, 'residual_cents': parseEur(_residual.text) ?? 0}..remove('updated_at');
    final ok = await guard(context, () => Api.instance.save('assets', body));
    if (ok != null && mounted) Navigator.pop(context, true);
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Bedrijfsmiddel'),
      content: SizedBox(
        width: 420,
        child: SingleChildScrollView(
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            TextField(controller: _name, decoration: const InputDecoration(labelText: 'Omschrijving (bv. MacBook Pro)')),
            const SizedBox(height: 12),
            DateField(label: 'Aanschafdatum', value: a['purchase_date'] as String?, onChanged: (v) => setState(() => a['purchase_date'] = v)),
            const SizedBox(height: 12),
            MoneyField(label: 'Aanschafprijs excl. btw', controller: _cost),
            const SizedBox(height: 12),
            Row(children: [
              Expanded(
                child: DropdownButtonFormField<int>(
                  initialValue: a['useful_years'] as int? ?? 5,
                  decoration: const InputDecoration(labelText: 'Afschrijven in'),
                  items: [for (final y in [2, 3, 4, 5, 7, 10]) DropdownMenuItem(value: y, child: Text('$y jaar'))],
                  onChanged: (v) => setState(() => a['useful_years'] = v),
                ),
              ),
              const SizedBox(width: 12),
              Expanded(child: MoneyField(label: 'Restwaarde', controller: _residual)),
            ]),
            const SizedBox(height: 12),
            DropdownButtonFormField<int>(
              initialValue: a['business_pct'] as int? ?? 100,
              decoration: const InputDecoration(labelText: 'Zakelijk gebruik'),
              items: [for (final p in [100, 90, 75, 50]) DropdownMenuItem(value: p, child: Text('$p%'))],
              onChanged: (v) => setState(() => a['business_pct'] = v),
            ),
            const SizedBox(height: 12),
            CheckboxListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Verkocht of weggedaan'),
              value: a['disposed_date'] != null,
              onChanged: (v) => setState(() => a['disposed_date'] = v == true ? today() : null),
            ),
            if (a['disposed_date'] != null) DateField(label: 'Datum', value: a['disposed_date'] as String?, onChanged: (v) => setState(() => a['disposed_date'] = v)),
          ]),
        ),
      ),
      actions: [
        if (a['created_at'] != null)
          TextButton(
            onPressed: () async {
              final ok = await guard(context, () async {
                await Api.instance.remove('assets', a['id'] as String);
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
