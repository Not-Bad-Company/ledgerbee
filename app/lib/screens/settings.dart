// Company details (read-only: they come from the shared .env), the
// startersaftrek, categories, devices, and signing out.

import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../widgets.dart';
import 'costs.dart';

class SettingsScreen extends StatelessWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Instellingen')),
      body: Loaded<(Map, Map, List<Map<String, dynamic>>)>(
        load: () async => (
          await Api.instance.get('/api/me') as Map,
          await Api.instance.get('/api/settings') as Map,
          await Categories.all(refresh: true),
        ),
        builder: (context, data, reload) {
          final (me, settings, cats) = data;
          final c = me['company'] as Map;
          String v(String k) => (c[k] as String?)?.isNotEmpty == true ? c[k] as String : 'nog niet bekend';
          return ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                Section(
                  title: 'Onderneming',
                  child: Column(children: [
                    ListTile(title: Text(v('name')), subtitle: Text('Eenmanszaak van ${v('owner')}\n${v('street')}, ${v('postcode')} ${v('city')}'), isThreeLine: true),
                    ListTile(dense: true, title: const Text('KvK-nummer'), trailing: Text(v('kvk'))),
                    ListTile(dense: true, title: const Text('Btw-id'), trailing: Text(v('btw'))),
                    ListTile(dense: true, title: const Text('IBAN op facturen'), trailing: Text(v('iban'))),
                    const Padding(
                      padding: EdgeInsets.fromLTRB(16, 4, 16, 12),
                      child: Text('Deze gegevens komen uit de gedeelde .env van Not Bad Company (NBC_*) en BOEK_IBAN op de server.', style: TextStyle(fontSize: 12)),
                    ),
                  ]),
                ),
                Section(
                  title: 'Belasting',
                  child: Column(children: [
                    ListTile(
                      title: const Text('Startdatum onderneming'),
                      subtitle: const Text('Vanaf hier lopen de btw-kwartalen'),
                      trailing: Text(longDate(settings['start_date'] as String)),
                      onTap: () async {
                        final d = await showDatePicker(context: context, initialDate: parseIso(settings['start_date'] as String) ?? DateTime.now(), firstDate: DateTime(2020), lastDate: DateTime.now());
                        if (d == null || !context.mounted) return;
                        await guard(context, () => Api.instance.put('/api/settings', {'start_date': iso(d)}));
                        reload();
                      },
                    ),
                    SwitchListTile(
                      title: const Text('Startersaftrek'),
                      subtitle: const Text('Maximaal 3 keer in de eerste 5 jaar, en alleen als je het urencriterium haalt'),
                      value: settings['startersaftrek'] == 'ja',
                      onChanged: (on) async {
                        await guard(context, () => Api.instance.put('/api/settings', {'startersaftrek': on ? 'ja' : 'nee'}));
                        reload();
                      },
                    ),
                  ]),
                ),
                Section(
                  title: 'Herinneringen',
                  child: ListTile(
                    leading: Icon(me['mail_configured'] == true ? Icons.mark_email_read_outlined : Icons.mail_lock_outlined),
                    title: Text(me['mail_configured'] == true ? 'Dagelijkse mail naar ${me['owner_email']}' : 'E-mail is niet ingesteld op de server'),
                    subtitle: const Text('14, 7 en 1 dag voor elke deadline, bij te late facturen, en op de 1e van de maand de maandafsluiting. '
                        'De telefoon geeft daarnaast zelf meldingen.'),
                    isThreeLine: true,
                  ),
                ),
                Section(
                  title: 'Categorieën',
                  child: Column(children: [
                    for (final cat in cats)
                      ListTile(
                        dense: true,
                        title: Text(cat['name'] as String, style: TextStyle(decoration: cat['archived'] == true ? TextDecoration.lineThrough : null)),
                        subtitle: Text([
                          if ((cat['deductible_pct'] as int) < 100) '${cat['deductible_pct']}% aftrekbaar',
                          if (cat['vat_deductible'] != true) 'btw niet aftrekbaar',
                          cat['ib_group'],
                        ].join(' · ')),
                        onTap: () async {
                          await _editCategory(context, cat);
                          reload();
                        },
                      ),
                    ListTile(
                      leading: const Icon(Icons.add),
                      title: const Text('Categorie toevoegen'),
                      onTap: () async {
                        await _editCategory(context, {'ib_group': 'overig', 'deductible_pct': 100, 'vat_deductible': true});
                        reload();
                      },
                    ),
                  ]),
                ),
                Section(
                  title: 'Apparaten',
                  child: Column(children: [
                    for (final d in (me['devices'] as List).cast<Map>())
                      ListTile(
                        title: Text(d['name'] as String),
                        subtitle: Text('Laatst gezien ${shortDate((d['last_seen'] as String).substring(0, 10))}'),
                        trailing: d['id'] == me['device_id']
                            ? const Text('dit apparaat')
                            : IconButton(
                                tooltip: 'Uitloggen',
                                icon: const Icon(Icons.logout),
                                onPressed: () async {
                                  await guard(context, () => Api.instance.delete('/api/devices/${d['id']}'));
                                  reload();
                                },
                              ),
                      ),
                  ]),
                ),
                OutlinedButton.icon(
                  onPressed: () async {
                    if (await confirm(context, 'Uitloggen op dit apparaat?', ok: 'Uitloggen')) await Api.instance.logout();
                  },
                  icon: const Icon(Icons.logout),
                  label: const Text('Uitloggen'),
                ),
                const SizedBox(height: 32),
              ]),
            ),
          ]);
        },
      ),
    );
  }

  static Future<void> _editCategory(BuildContext context, Map<String, dynamic> cat) async {
    final c = Map<String, dynamic>.of(cat);
    final name = TextEditingController(text: c['name'] as String? ?? '');
    await showDialog<void>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, set) => AlertDialog(
          title: const Text('Categorie'),
          content: SizedBox(
            width: 380,
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              TextField(controller: name, decoration: const InputDecoration(labelText: 'Naam')),
              const SizedBox(height: 12),
              DropdownButtonFormField<String>(
                initialValue: c['ib_group'] as String,
                decoration: const InputDecoration(labelText: 'Kostensoort in de aangifte'),
                items: const [
                  DropdownMenuItem(value: 'kantoor', child: Text('Kantoorkosten')),
                  DropdownMenuItem(value: 'verkoop', child: Text('Verkoopkosten')),
                  DropdownMenuItem(value: 'auto', child: Text('Vervoerskosten')),
                  DropdownMenuItem(value: 'huisvesting', child: Text('Huisvesting')),
                  DropdownMenuItem(value: 'overig', child: Text('Andere kosten')),
                  DropdownMenuItem(value: 'investering', child: Text('Investering (wordt activum)')),
                ],
                onChanged: (v) => set(() => c['ib_group'] = v),
              ),
              const SizedBox(height: 12),
              DropdownButtonFormField<int>(
                initialValue: c['deductible_pct'] as int,
                decoration: const InputDecoration(labelText: 'Aftrekbaar'),
                items: [for (final p in [100, 80, 75, 50, 0]) DropdownMenuItem(value: p, child: Text('$p%'))],
                onChanged: (v) => set(() => c['deductible_pct'] = v),
              ),
              SwitchListTile(contentPadding: EdgeInsets.zero, title: const Text('Btw aftrekbaar'), value: c['vat_deductible'] == true, onChanged: (v) => set(() => c['vat_deductible'] = v)),
              if (c['created_at'] != null)
                SwitchListTile(contentPadding: EdgeInsets.zero, title: const Text('Gearchiveerd'), value: c['archived'] == true, onChanged: (v) => set(() => c['archived'] = v)),
            ]),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('Annuleren')),
            FilledButton(
              onPressed: () async {
                if (name.text.trim().isEmpty) return;
                final ok = await guard(ctx, () => Api.instance.save('categories', {...c, 'name': name.text.trim()}..remove('updated_at')));
                if (ok != null && ctx.mounted) Navigator.pop(ctx);
              },
              child: const Text('Bewaren'),
            ),
          ],
        ),
      ),
    );
  }
}
