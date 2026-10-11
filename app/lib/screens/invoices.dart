// Sales invoices and clients.

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:url_launcher/url_launcher.dart';

import '../api.dart';
import '../format.dart';
import '../theme.dart';
import '../widgets.dart';

/// The same totals the server computes: btw per rate over the summed base.
({int excl, int vat, int incl, Map<String, (int, int)> perRate}) invoiceTotals(String scheme, List<Map<String, dynamic>> lines) {
  final perRate = <String, (int, int)>{};
  for (final l in lines) {
    final rate = scheme == 'nl' ? (l['vat_rate'] as String? ?? '21') : '0';
    final amount = ((l['quantity_milli'] as int? ?? 0) * (l['unit_cents'] as int? ?? 0) / 1000).round();
    final cur = perRate[rate] ?? (0, 0);
    perRate[rate] = (cur.$1 + amount, 0);
  }
  var excl = 0, vat = 0;
  for (final k in perRate.keys.toList()) {
    final base = perRate[k]!.$1;
    final v = (base * (int.tryParse(k) ?? 0) / 100).round();
    perRate[k] = (base, v);
    excl += base;
    vat += v;
  }
  return (excl: excl, vat: vat, incl: excl + vat, perRate: perRate);
}

class InvoicesScreen extends StatelessWidget {
  const InvoicesScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return DefaultTabController(
      length: 2,
      child: Scaffold(
        appBar: AppBar(
          title: const Text('Facturen'),
          bottom: const TabBar(tabs: [Tab(text: 'Facturen'), Tab(text: 'Klanten')]),
        ),
        body: const TabBarView(children: [_InvoiceList(), _ClientList()]),
      ),
    );
  }
}

class _InvoiceList extends StatefulWidget {
  const _InvoiceList();

  @override
  State<_InvoiceList> createState() => _InvoiceListState();
}

class _InvoiceListState extends State<_InvoiceList> {
  final _key = GlobalKey<LoadedState<(List<Map<String, dynamic>>, Map<String, String>)>>();

  Future<(List<Map<String, dynamic>>, Map<String, String>)> _load() async {
    final invoices = await Api.instance.list('invoices');
    final clients = await Api.instance.list('clients');
    return (invoices, {for (final c in clients) c['id'] as String: c['name'] as String});
  }

  Future<void> _open(Map<String, dynamic>? inv) async {
    await Navigator.push(context, MaterialPageRoute(builder: (_) => InvoiceEditor(invoice: inv)));
    _key.currentState?.reload();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      floatingActionButton: FloatingActionButton.extended(onPressed: () => _open(null), icon: const Icon(Icons.add), label: const Text('Nieuwe factuur')),
      body: Loaded<(List<Map<String, dynamic>>, Map<String, String>)>(
        key: _key,
        load: _load,
        builder: (context, data, reload) {
          final (invoices, clients) = data;
          if (invoices.isEmpty) return const EmptyState(icon: Icons.request_quote_outlined, text: 'Nog geen facturen.');
          final now = today();
          return ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Card(
                child: Column(children: [
                  for (final inv in invoices)
                    ListTile(
                      leading: _statusIcon(inv, now),
                      title: Text(clients[inv['client_id']] ?? 'Geen klant'),
                      subtitle: Text('${inv['number'] ?? 'Concept'} · ${shortDate(inv['date'])}${_overdue(inv, now) ? ' · over de vervaldatum' : ''}'),
                      trailing: Text(eur(invoiceTotals(inv['vat_scheme'] as String, (inv['lines'] as List).cast<Map<String, dynamic>>()).incl), style: amountStyle(context)),
                      onTap: () => _open(inv),
                    ),
                ]),
              ),
            ),
            const SizedBox(height: 72),
          ]);
        },
      ),
    );
  }

  static bool _overdue(Map inv, String now) => inv['status'] == 'verzonden' && (inv['due_date'] as String).compareTo(now) < 0;

  static Widget _statusIcon(Map inv, String now) {
    if (inv['status'] == 'betaald') return Icon(Icons.check_circle, color: green);
    if (_overdue(inv, now)) return Icon(Icons.warning_amber, color: red);
    if (inv['status'] == 'verzonden') return const Icon(Icons.schedule);
    return Icon(Icons.edit_note, color: amber);
  }
}

class _InvLine {
  final TextEditingController description;
  final TextEditingController quantity;
  final TextEditingController unit;
  String rate;
  _InvLine(Map l)
      : description = TextEditingController(text: l['description'] as String? ?? ''),
        quantity = TextEditingController(text: number((l['quantity_milli'] as int? ?? 1000) / 1000, decimals: (l['quantity_milli'] as int? ?? 1000) % 1000 == 0 ? 0 : 2)),
        unit = TextEditingController(text: centsInput(l['unit_cents'] as int? ?? 0)),
        rate = l['vat_rate'] as String? ?? '21';

  int get quantityMilli => ((double.tryParse(quantity.text.replaceAll('.', '').replaceAll(',', '.')) ?? 0) * 1000).round();

  Map<String, dynamic> toJson() =>
      {'description': description.text.trim(), 'quantity_milli': quantityMilli, 'unit_cents': parseEur(unit.text) ?? 0, 'vat_rate': rate};
}

class InvoiceEditor extends StatefulWidget {
  final Map<String, dynamic>? invoice;
  const InvoiceEditor({super.key, this.invoice});

  @override
  State<InvoiceEditor> createState() => _InvoiceEditorState();
}

class _InvoiceEditorState extends State<InvoiceEditor> {
  late Map<String, dynamic> inv = widget.invoice != null ? Map.of(widget.invoice!) : {'date': today(), 'vat_scheme': 'nl', 'status': 'concept', 'lines': []};
  late List<_InvLine> _lines = [for (final l in (inv['lines'] as List).cast<Map>()) _InvLine(l)];
  late final _note = TextEditingController(text: inv['note'] as String? ?? '');
  List<Map<String, dynamic>> _clients = [];
  bool _busy = false;

  bool get _concept => inv['status'] == 'concept';
  bool get _existing => inv['created_at'] != null;

  @override
  void initState() {
    super.initState();
    if (_lines.isEmpty) _lines = [_InvLine({})];
    _loadClients();
  }

  Future<void> _loadClients() async {
    final c = await guard(context, () => Api.instance.list('clients'));
    if (c != null && mounted) setState(() => _clients = c);
  }

  Map<String, dynamic> get _body => {
        ...inv,
        'note': _note.text.trim(),
        'lines': _lines.map((l) => l.toJson()).toList(),
      }..removeWhere((k, _) => ['number', 'sent_at', 'paid_at', 'updated_at', 'reminded_at', 'reminders'].contains(k));

  Future<bool> _store() async {
    final saved = await guard(context, () => Api.instance.save('invoices', _concept ? _body : {'id': inv['id'], 'note': _note.text.trim(), 'created_at': inv['created_at']}));
    if (saved == null) return false;
    setState(() => inv = saved);
    return true;
  }

  Future<void> _storeAndSay(String text) async {
    if (await _store() && mounted) showInfo(context, text);
  }

  Future<void> _act(Future<void> Function() f) async {
    setState(() => _busy = true);
    await f();
    if (mounted) setState(() => _busy = false);
  }

  Future<void> _finalize() async {
    if (inv['client_id'] == null) return showError(context, 'Kies eerst een klant');
    final ok = await confirm(context, 'Definitief maken? De factuur krijgt een nummer en kan daarna niet meer veranderen.', ok: 'Definitief maken');
    if (!ok || !await _store() || !mounted) return;
    final res = await guard(context, () => Api.instance.post('/api/invoices/${inv['id']}/finalize'));
    if (res != null && mounted) setState(() => inv = Map<String, dynamic>.from(res as Map));
  }

  Future<File?> _pdf() async {
    return guard(context, () async {
      final bytes = await Api.instance.bytes('/api/invoices/${inv['id']}/pdf');
      final dir = await getTemporaryDirectory();
      final f = File('${dir.path}/Factuur ${inv['number'] ?? 'concept'}.pdf');
      await f.writeAsBytes(bytes);
      return f;
    });
  }

  Future<void> _openPdf() async {
    if (_concept && !await _store()) return;
    final f = await _pdf();
    if (f != null) await launchUrl(Uri.file(f.path));
  }

  Future<void> _savePdf() async {
    final f = await _pdf();
    if (f == null || !mounted) return;
    await guard(context, () => FilePicker.saveFile(fileName: f.uri.pathSegments.last, bytes: f.readAsBytesSync(), mimeType: 'application/pdf'));
  }

  Future<void> _send() async {
    final client = _clients.firstWhere((c) => c['id'] == inv['client_id'], orElse: () => {});
    final ok = await confirm(context, 'Factuur ${inv['number']} mailen naar ${client['email'] ?? '?'}? Je krijgt een kopie.', ok: 'Versturen');
    if (!ok || !mounted) return;
    final res = await guard(context, () => Api.instance.post('/api/invoices/${inv['id']}/send'));
    if (res != null && mounted) showInfo(context, 'Verstuurd');
  }

  Future<void> _remind() async {
    final client = _clients.firstWhere((c) => c['id'] == inv['client_id'], orElse: () => {});
    final ok = await confirm(context, 'Een vriendelijke herinnering voor factuur ${inv['number']} sturen naar ${client['email'] ?? '?'}? De factuur gaat opnieuw mee.', ok: 'Herinnering sturen');
    if (!ok || !mounted) return;
    final res = await guard(context, () => Api.instance.post('/api/invoices/${inv['id']}/remind'));
    if (res != null && mounted) {
      setState(() {
        inv['reminders'] = ((inv['reminders'] as int?) ?? 0) + 1;
        inv['reminded_at'] = DateTime.now().toIso8601String();
      });
      showInfo(context, 'Herinnering verstuurd');
    }
  }

  bool get _overdue => inv['status'] == 'verzonden' && ((inv['due_date'] as String?) ?? '').compareTo(today()) < 0;

  Future<void> _paid(bool paid) async {
    String? date;
    if (paid) {
      final d = await showDatePicker(context: context, initialDate: DateTime.now(), firstDate: DateTime(2020), lastDate: DateTime.now(), helpText: 'Betaald op');
      if (d == null) return;
      date = iso(d);
    }
    if (!mounted) return;
    final res = await guard(context, () => Api.instance.post('/api/invoices/${inv['id']}/paid', {'date': date}));
    if (res != null && mounted) setState(() => inv = Map<String, dynamic>.from(res as Map));
  }

  @override
  Widget build(BuildContext context) {
    final t = invoiceTotals(inv['vat_scheme'] as String, _lines.map((l) => l.toJson()).toList());
    final scheme = inv['vat_scheme'] as String;
    return Scaffold(
      appBar: AppBar(
        title: Text(inv['number'] != null ? 'Factuur ${inv['number']}' : 'Conceptfactuur'),
        actions: [
          if (_existing || _concept) IconButton(onPressed: _busy ? null : _openPdf, icon: const Icon(Icons.picture_as_pdf_outlined), tooltip: 'PDF bekijken'),
          if (!_concept) IconButton(onPressed: _savePdf, icon: const Icon(Icons.download_outlined), tooltip: 'PDF opslaan'),
          if (_existing && _concept)
            IconButton(
              onPressed: () async {
                if (!await confirm(context, 'Concept verwijderen?', ok: 'Verwijderen')) return;
                if (!context.mounted) return;
                final ok = await guard(context, () async {
                  await Api.instance.remove('invoices', inv['id'] as String);
                  return true;
                });
                if (ok == true && context.mounted) Navigator.pop(context);
              },
              icon: const Icon(Icons.delete_outline),
            ),
        ],
      ),
      body: ListView(padding: const EdgeInsets.all(16), children: [
        Readable(
          child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
            if (!_concept)
              Notice(
                inv['status'] == 'betaald'
                    ? 'Betaald op ${longDate(inv['paid_at'] as String?)}.'
                    : _overdue
                        ? 'Over de vervaldatum (${longDate(inv['due_date'] as String?)}).'
                            '${(inv['reminders'] as int? ?? 0) > 0 ? ' ${inv['reminders']}× herinnerd, laatst op ${shortDate((inv['reminded_at'] as String).substring(0, 10))}.' : ''}'
                        : 'Verstuurd${inv['sent_at'] != null ? ' per mail' : ''}; te betalen vóór ${longDate(inv['due_date'] as String?)}.',
                color: inv['status'] == 'betaald' ? green : (_overdue ? red : null),
                icon: inv['status'] == 'betaald' ? Icons.check_circle_outline : Icons.schedule,
              ),
            Row(children: [
              Expanded(
                child: DropdownButtonFormField<String>(
                  initialValue: _clients.any((c) => c['id'] == inv['client_id']) ? inv['client_id'] as String : null,
                  isExpanded: true,
                  decoration: const InputDecoration(labelText: 'Klant'),
                  items: [for (final c in _clients) DropdownMenuItem(value: c['id'] as String, child: Text(c['name'] as String))],
                  onChanged: _concept ? (v) => setState(() => inv['client_id'] = v) : null,
                ),
              ),
              if (_concept)
                IconButton(
                  tooltip: 'Nieuwe klant',
                  onPressed: () async {
                    final c = await Navigator.push<Map<String, dynamic>>(context, MaterialPageRoute(builder: (_) => const ClientEditor()));
                    await _loadClients();
                    if (c != null) setState(() => inv['client_id'] = c['id']);
                  },
                  icon: const Icon(Icons.person_add_alt),
                ),
            ]),
            const SizedBox(height: 12),
            Row(children: [
              Expanded(child: _concept ? DateField(label: 'Factuurdatum', value: inv['date'] as String?, onChanged: (v) => setState(() => inv['date'] = v)) : _fixed('Factuurdatum', longDate(inv['date'] as String?))),
              const SizedBox(width: 12),
              Expanded(
                child: DropdownButtonFormField<String>(
                  initialValue: scheme,
                  isExpanded: true,
                  decoration: const InputDecoration(labelText: 'Btw'),
                  items: const [
                    DropdownMenuItem(value: 'nl', child: Text('Nederlandse btw')),
                    DropdownMenuItem(value: 'verlegd_eu', child: Text('Verlegd (EU-bedrijf)')),
                    DropdownMenuItem(value: 'export', child: Text('Buiten de EU')),
                  ],
                  onChanged: _concept ? (v) => setState(() => inv['vat_scheme'] = v) : null,
                ),
              ),
            ]),
            if (scheme == 'verlegd_eu') const Padding(padding: EdgeInsets.only(top: 8), child: Text('Zet het btw-id van de klant bij de klant; het komt op de factuur.', style: TextStyle(fontSize: 12))),
            const SizedBox(height: 20),
            Text('Regels', style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 8),
            for (var i = 0; i < _lines.length; i++) _lineCard(i, scheme),
            if (_concept)
              Align(alignment: Alignment.centerLeft, child: TextButton.icon(onPressed: () => setState(() => _lines.add(_InvLine({}))), icon: const Icon(Icons.add), label: const Text('Regel toevoegen'))),
            const SizedBox(height: 8),
            Card(
              child: Column(children: [
                AmountRow('Subtotaal', t.excl),
                if (scheme == 'nl')
                  for (final e in t.perRate.entries) AmountRow('Btw ${e.key}% over ${eur(e.value.$1)}', e.value.$2),
                AmountRow('Totaal', t.incl, strong: true),
              ]),
            ),
            const SizedBox(height: 12),
            TextField(controller: _note, decoration: const InputDecoration(labelText: 'Notitie op de factuur'), maxLines: 2),
            const SizedBox(height: 24),
            if (_concept)
              Row(children: [
                Expanded(child: OutlinedButton(onPressed: _busy ? null : () => _act(() => _storeAndSay('Bewaard')), child: const Text('Bewaren'))),
                const SizedBox(width: 12),
                Expanded(child: FilledButton(onPressed: _busy ? null : () => _act(_finalize), child: const Text('Definitief maken'))),
              ])
            else
              Wrap(spacing: 12, runSpacing: 12, children: [
                if (_overdue)
                  FilledButton.icon(onPressed: _busy ? null : () => _act(_remind), icon: const Icon(Icons.notifications_active_outlined), label: const Text('Herinnering sturen')),
                if (_overdue)
                  OutlinedButton.icon(onPressed: _busy ? null : () => _act(_send), icon: const Icon(Icons.send_outlined), label: const Text('Opnieuw mailen'))
                else
                  FilledButton.icon(onPressed: _busy ? null : () => _act(_send), icon: const Icon(Icons.send_outlined), label: Text(inv['sent_at'] != null ? 'Opnieuw mailen' : 'Mailen naar klant')),
                if (inv['status'] != 'betaald')
                  OutlinedButton.icon(onPressed: () => _paid(true), icon: const Icon(Icons.check), label: const Text('Betaald'))
                else
                  OutlinedButton(onPressed: () => _paid(false), child: const Text('Toch niet betaald')),
                OutlinedButton(onPressed: _busy ? null : () => _act(() => _storeAndSay('Notitie bewaard')), child: const Text('Notitie bewaren')),
              ]),
            const SizedBox(height: 32),
          ]),
        ),
      ]),
    );
  }

  Widget _fixed(String label, String value) => InputDecorator(decoration: InputDecoration(labelText: label), child: Text(value));

  Widget _lineCard(int i, String scheme) {
    final l = _lines[i];
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(children: [
            Row(children: [
              Expanded(child: TextField(controller: l.description, enabled: _concept, decoration: const InputDecoration(labelText: 'Omschrijving'), maxLines: null, onChanged: (_) => setState(() {}))),
              if (_concept && _lines.length > 1) IconButton(onPressed: () => setState(() => _lines.removeAt(i)), icon: const Icon(Icons.close)),
            ]),
            const SizedBox(height: 10),
            Row(children: [
              Expanded(flex: 2, child: TextField(controller: l.quantity, enabled: _concept, decoration: const InputDecoration(labelText: 'Aantal'), keyboardType: const TextInputType.numberWithOptions(decimal: true), onChanged: (_) => setState(() {}))),
              const SizedBox(width: 10),
              Expanded(
                flex: 3,
                child: TextField(
                  controller: l.unit,
                  enabled: _concept,
                  decoration: const InputDecoration(labelText: 'Prijs excl.', prefixText: '€ '),
                  keyboardType: const TextInputType.numberWithOptions(decimal: true),
                  style: amountStyle(context),
                  onChanged: (_) => setState(() {}),
                ),
              ),
              if (scheme == 'nl') ...[
                const SizedBox(width: 10),
                Expanded(
                  flex: 2,
                  child: DropdownButtonFormField<String>(
                    initialValue: l.rate,
                    decoration: const InputDecoration(labelText: 'Btw'),
                    items: const [DropdownMenuItem(value: '21', child: Text('21%')), DropdownMenuItem(value: '9', child: Text('9%')), DropdownMenuItem(value: '0', child: Text('0%'))],
                    onChanged: _concept ? (v) => setState(() => l.rate = v ?? '21') : null,
                  ),
                ),
              ],
            ]),
          ]),
        ),
      ),
    );
  }
}

class _ClientList extends StatefulWidget {
  const _ClientList();

  @override
  State<_ClientList> createState() => _ClientListState();
}

class _ClientListState extends State<_ClientList> {
  final _key = GlobalKey<LoadedState<List<Map<String, dynamic>>>>();

  Future<void> _open(Map<String, dynamic>? c) async {
    await Navigator.push(context, MaterialPageRoute(builder: (_) => ClientEditor(client: c)));
    _key.currentState?.reload();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      floatingActionButton: FloatingActionButton.extended(onPressed: () => _open(null), icon: const Icon(Icons.person_add_alt), label: const Text('Nieuwe klant')),
      body: Loaded<List<Map<String, dynamic>>>(
        key: _key,
        load: () => Api.instance.list('clients'),
        builder: (context, clients, reload) {
          if (clients.isEmpty) return const EmptyState(icon: Icons.people_outline, text: 'Nog geen klanten.');
          return ListView(padding: const EdgeInsets.all(16), children: [
            Readable(
              child: Card(
                child: Column(children: [
                  for (final c in clients)
                    ListTile(
                      title: Text(c['name'] as String),
                      subtitle: Text([c['contact'], c['city'], c['email']].where((x) => (x as String).isNotEmpty).join(' · ')),
                      onTap: () => _open(c),
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

class ClientEditor extends StatefulWidget {
  final Map<String, dynamic>? client;
  const ClientEditor({super.key, this.client});

  @override
  State<ClientEditor> createState() => _ClientEditorState();
}

class _ClientEditorState extends State<ClientEditor> {
  late final Map<String, dynamic> c = widget.client != null ? Map.of(widget.client!) : {'country': 'NL', 'is_business': true, 'payment_days': 14};
  late final Map<String, TextEditingController> f = {
    for (final k in ['name', 'contact', 'email', 'street', 'postcode', 'city', 'country', 'kvk', 'vat_id']) k: TextEditingController(text: c[k] as String? ?? ''),
  };
  late final _days = TextEditingController(text: '${c['payment_days'] ?? 14}');

  Future<void> _save() async {
    if (f['name']!.text.trim().isEmpty) return showError(context, 'Naam is verplicht');
    final body = {
      ...c,
      for (final e in f.entries) e.key: e.value.text.trim(),
      'payment_days': int.tryParse(_days.text) ?? 14,
    }..remove('updated_at');
    final saved = await guard(context, () => Api.instance.save('clients', body));
    if (saved != null && mounted) Navigator.pop(context, saved);
  }

  @override
  Widget build(BuildContext context) {
    Widget field(String k, String label, {TextInputType? type}) =>
        Padding(padding: const EdgeInsets.only(bottom: 12), child: TextField(controller: f[k], decoration: InputDecoration(labelText: label), keyboardType: type));
    return Scaffold(
      appBar: AppBar(title: Text(widget.client == null ? 'Nieuwe klant' : 'Klant')),
      body: ListView(padding: const EdgeInsets.all(16), children: [
        Readable(
          max: 560,
          child: Column(children: [
            field('name', 'Naam (bedrijf of persoon)'),
            field('contact', 'Contactpersoon'),
            field('email', 'E-mail voor facturen', type: TextInputType.emailAddress),
            field('street', 'Straat en huisnummer'),
            Row(children: [Expanded(child: field('postcode', 'Postcode')), const SizedBox(width: 12), Expanded(flex: 2, child: field('city', 'Plaats'))]),
            Row(children: [Expanded(child: field('country', 'Land (NL, DE, …)')), const SizedBox(width: 12), Expanded(child: field('kvk', 'KvK'))]),
            field('vat_id', 'Btw-id (verplicht bij verlegde btw)'),
            Padding(padding: const EdgeInsets.only(bottom: 12), child: TextField(controller: _days, decoration: const InputDecoration(labelText: 'Betaaltermijn (dagen)'), keyboardType: TextInputType.number)),
            SwitchListTile(contentPadding: EdgeInsets.zero, title: const Text('Zakelijke klant'), value: c['is_business'] == true, onChanged: (v) => setState(() => c['is_business'] = v)),
            const SizedBox(height: 16),
            SizedBox(width: double.infinity, child: FilledButton(onPressed: _save, child: const Text('Bewaren'))),
          ]),
        ),
      ]),
    );
  }
}
