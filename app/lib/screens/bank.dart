import 'package:flutter/material.dart';

import '../api.dart';
import '../format.dart';
import '../scan.dart';
import '../theme.dart';
import '../widgets.dart';
import 'costs.dart';

class _BankData {
  final List<Map<String, dynamic>> transactions;
  final List<Map<String, dynamic>> suggestions;
  final List<Map<String, dynamic>> missing;
  _BankData(this.transactions, this.suggestions, this.missing);
}

class BankScreen extends StatefulWidget {
  const BankScreen({super.key});

  @override
  State<BankScreen> createState() => _BankScreenState();
}

class _BankScreenState extends State<BankScreen> {
  final _key = GlobalKey<LoadedState<_BankData>>();

  Future<_BankData> _load() async {
    final api = Api.instance;
    final tx = await api.list('bank_transactions');
    final s = (await api.get('/api/bank/suggestions') as List).map((e) => Map<String, dynamic>.from(e as Map)).toList();
    final m = (await api.get('/api/bank/missing') as List).map((e) => Map<String, dynamic>.from(e as Map)).toList();
    return _BankData(tx, s, m);
  }

  void _reload() => _key.currentState?.reload();

  Future<void> _import() async {
    final bytes = await guard(context, pickBankFile);
    if (bytes == null || !mounted) return;
    final res = await guard(context, () => Api.instance.uploadTo('/api/bank/import', bytes));
    if (res == null || !mounted) return;
    showInfo(context, '${res['added']} nieuw, ${res['duplicates']} al bekend, ${res['matched']} factuur/facturen meteen als betaald gemarkeerd');
    _reload();
  }

  Future<void> _link(String tx, String kind, String id) async {
    await guard(context, () => Api.instance.post('/api/bank/$tx/link', {'kind': kind, 'id': id}));
    _reload();
  }

  Future<void> _ignore(Map tx, bool value) async {
    await guard(context, () => Api.instance.save('bank_transactions', {'id': tx['id'], 'created_at': tx['created_at'], 'ignored': value}));
    _reload();
  }

  Future<void> _makeCost(Map tx) async {
    final amount = -(tx['amount_cents'] as int);
    final vat = vatFromIncl(amount, '21');
    final saved = await Navigator.push<Map<String, dynamic>?>(
      context,
      MaterialPageRoute(
        builder: (_) => DocumentEditor(doc: {
          'kind': 'factuur',
          'date': tx['date'],
          'supplier_name': tx['counterparty'],
          'total_cents': amount,
          'status': 'concept',
          'parse_notes': 'Gemaakt van een banktransactie: voeg de factuur toe zodra je die hebt (bewaarplicht).',
          'lines': [
            {'amount_excl_cents': amount - vat, 'vat_rate': '21', 'vat_cents': vat, 'business_pct': 100},
          ],
        }),
      ),
    );
    if (saved != null) await _link(tx['id'] as String, 'document', saved['id'] as String);
    _reload();
  }

  Future<void> _actions(Map tx) async {
    final out = (tx['amount_cents'] as int) < 0;
    final matched = (tx['matched_id'] as String).isNotEmpty;
    final action = await showModalBottomSheet<String>(
      context: context,
      builder: (c) => SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          ListTile(title: Text(tx['counterparty'] as String), subtitle: Text('${shortDate(tx['date'])} · ${tx['description']}')),
          const Divider(),
          if (out && !matched) ListTile(leading: const Icon(Icons.add_card), title: const Text('Kosten hiervan maken'), onTap: () => Navigator.pop(c, 'cost')),
          if (matched) ListTile(leading: const Icon(Icons.link_off), title: const Text('Koppeling verwijderen'), onTap: () => Navigator.pop(c, 'unlink')),
          if (tx['ignored'] != true)
            ListTile(leading: const Icon(Icons.visibility_off_outlined), title: const Text('Privé / niet zakelijk'), subtitle: const Text('Telt niet mee en hoeft geen bon'), onTap: () => Navigator.pop(c, 'ignore'))
          else
            ListTile(leading: const Icon(Icons.visibility_outlined), title: const Text('Toch zakelijk'), onTap: () => Navigator.pop(c, 'unignore')),
        ]),
      ),
    );
    switch (action) {
      case 'cost':
        await _makeCost(tx);
      case 'unlink':
        await _link(tx['id'] as String, '', '');
      case 'ignore':
        await _ignore(tx, true);
      case 'unignore':
        await _ignore(tx, false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Bank'), actions: [
        IconButton(onPressed: () => showDialog(context: context, builder: (_) => const _HowTo()), icon: const Icon(Icons.help_outline), tooltip: 'Hoe exporteer ik?'),
      ]),
      floatingActionButton: FloatingActionButton.extended(onPressed: _import, icon: const Icon(Icons.upload_file), label: const Text('Afschrift importeren')),
      body: Loaded<_BankData>(
        key: _key,
        load: _load,
        builder: (context, data, reload) {
          if (data.transactions.isEmpty) {
            return EmptyState(
              icon: Icons.account_balance_outlined,
              text: 'Importeer een CSV- of CAMT.053-bestand van je zakelijke rekening. Betaalde facturen worden dan vanzelf herkend, en betalingen zonder bonnetje komen in een lijst.',
              action: OutlinedButton(onPressed: () => showDialog(context: context, builder: (_) => const _HowTo()), child: const Text('Hoe exporteer ik?')),
            );
          }
          final byId = {for (final t in data.transactions) t['id']: t};
          return ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                if (data.suggestions.isNotEmpty)
                  Section(
                    title: 'Voorstellen',
                    child: Column(children: [
                      for (final s in data.suggestions)
                        if (byId[s['transaction_id']] case final tx?)
                          ListTile(
                            title: Text('${tx['counterparty']} ${eur(tx['amount_cents'] as int)}', style: amountStyle(context)),
                            subtitle: Text('${shortDate(tx['date'])} → ${s['label']}'),
                            trailing: Row(mainAxisSize: MainAxisSize.min, children: [
                              IconButton(onPressed: () => _link(tx['id'] as String, s['kind'] as String, s['id'] as String), icon: Icon(Icons.check, color: green), tooltip: 'Koppelen'),
                              IconButton(onPressed: () => _actions(tx), icon: const Icon(Icons.more_vert)),
                            ]),
                          ),
                    ]),
                  ),
                if (data.missing.isNotEmpty)
                  Section(
                    title: 'Bonnetje ontbreekt (${data.missing.length})',
                    child: Column(children: [
                      for (final tx in data.missing.take(30))
                        ListTile(
                          leading: Icon(Icons.help_outline, color: amber),
                          title: Text(tx['counterparty'] as String),
                          subtitle: Text(shortDate(tx['date'])),
                          trailing: Text(eur(tx['amount_cents'] as int), style: amountStyle(context)),
                          onTap: () => _actions(tx),
                        ),
                    ]),
                  ),
                Section(
                  title: 'Transacties',
                  child: Column(children: [
                    for (final tx in data.transactions.take(300))
                      ListTile(
                        dense: true,
                        leading: Icon(
                          tx['ignored'] == true
                              ? Icons.visibility_off_outlined
                              : (tx['matched_id'] as String).isNotEmpty
                                  ? Icons.link
                                  : Icons.circle_outlined,
                          size: 18,
                          color: (tx['matched_id'] as String).isNotEmpty ? green : null,
                        ),
                        title: Text(tx['counterparty'] as String, maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: Text('${shortDate(tx['date'])} · ${tx['description']}', maxLines: 1, overflow: TextOverflow.ellipsis),
                        trailing: Text(eur(tx['amount_cents'] as int, sign: true),
                            style: amountStyle(context, color: (tx['amount_cents'] as int) > 0 ? green : null)),
                        onTap: () => _actions(tx),
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
}

class _HowTo extends StatelessWidget {
  const _HowTo();

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Afschrift exporteren'),
      content: const SingleChildScrollView(
        child: Text(
          'Download in je internetbankieren de transacties van je zakelijke rekening als CSV (of CAMT.053 / XML):\n\n'
          '• ING: Mijn ING → rekening → Downloaden → CSV (puntkomma).\n'
          '• Rabobank: Rekeningen → Transacties downloaden → CSV of CAMT.053.\n'
          '• ABN AMRO: Rekeningen → Downloaden → TXT of CAMT.053.\n'
          '• Knab en bunq: Transacties → Exporteren → CSV.\n\n'
          'Dubbele regels worden herkend, dus overlappende periodes importeren kan gewoon. Doe het één keer per maand.',
        ),
      ),
      actions: [TextButton(onPressed: () => Navigator.pop(context), child: const Text('Sluiten'))],
    );
  }
}
