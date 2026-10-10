// Costs: every bonnetje and incoming invoice, scanning, and the editor that
// turns a draft into a confirmed cost.

import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:url_launcher/url_launcher.dart';

import '../api.dart';
import '../format.dart';
import '../scan.dart';
import '../theme.dart';
import '../widgets.dart';

/// Categories change rarely: loaded once per session.
class Categories {
  static List<Map<String, dynamic>>? _all;
  static Future<List<Map<String, dynamic>>> all({bool refresh = false}) async {
    if (_all == null || refresh) _all = await Api.instance.list('categories');
    return _all!;
  }
}

class CostsScreen extends StatefulWidget {
  const CostsScreen({super.key});

  @override
  State<CostsScreen> createState() => _CostsScreenState();
}

class _CostsScreenState extends State<CostsScreen> {
  final _key = GlobalKey<LoadedState<List<Map<String, dynamic>>>>();
  String _filter = 'alle';

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('Kosten'), actions: [
        PopupMenuButton<String>(
          initialValue: _filter,
          icon: const Icon(Icons.filter_list),
          onSelected: (v) => setState(() => _filter = v),
          itemBuilder: (_) => const [
            PopupMenuItem(value: 'alle', child: Text('Alles')),
            PopupMenuItem(value: 'concept', child: Text('Nog bevestigen')),
            PopupMenuItem(value: 'bevestigd', child: Text('Bevestigd')),
          ],
        ),
      ]),
      floatingActionButton: ScanButton(onDone: () => _key.currentState?.reload()),
      body: Loaded<List<Map<String, dynamic>>>(
        key: _key,
        load: () => Api.instance.list('documents'),
        builder: (context, docs, reload) {
          final shown = _filter == 'alle' ? docs : docs.where((d) => d['status'] == _filter).toList();
          if (shown.isEmpty) {
            return const EmptyState(icon: Icons.receipt_long_outlined, text: 'Nog geen kosten. Scan een bonnetje of kies een PDF-factuur met de knop rechtsonder.');
          }
          final byMonth = <String, List<Map<String, dynamic>>>{};
          for (final d in shown) {
            byMonth.putIfAbsent((d['date'] as String).substring(0, 7), () => []).add(d);
          }
          return ListView(padding: const EdgeInsets.all(16), children: [
            for (final e in byMonth.entries)
              Readable(
                child: Section(
                  title: _monthTitle(e.key),
                  trailing: Text(eur(e.value.fold<int>(0, (s, d) => s + (d['total_cents'] as int))), style: amountStyle(context, size: 12)),
                  child: Column(children: [
                    for (final d in e.value)
                      ListTile(
                        leading: Icon(d['kind'] == 'factuur' ? Icons.description_outlined : Icons.receipt_outlined,
                            color: d['status'] == 'concept' ? amber : null),
                        title: Text((d['supplier_name'] as String).isEmpty ? 'Onbekend' : d['supplier_name'] as String),
                        subtitle: Text('${shortDate(d['date'])}${d['status'] == 'concept' ? ' · nog bevestigen' : ''}'),
                        trailing: Text(eur(d['total_cents'] as int), style: amountStyle(context)),
                        onTap: () async {
                          await Navigator.push(context, MaterialPageRoute(builder: (_) => DocumentEditor(doc: d)));
                          reload();
                        },
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

  static String _monthTitle(String ym) {
    final d = DateTime.parse('$ym-01');
    return '${longDate(iso(d)).split(' ')[1]} ${d.year}';
  }
}

/// Camera, file, or by hand. On the phone a tap goes straight to the camera.
class ScanButton extends StatelessWidget {
  final VoidCallback? onDone;
  const ScanButton({super.key, this.onDone});

  Future<void> _start(BuildContext context, String how) async {
    Capture? cap;
    try {
      if (how == 'camera') cap = await takePhoto();
      if (how == 'file') cap = await pickFile();
    } catch (e) {
      if (context.mounted) showError(context, e);
      return;
    }
    if (how != 'manual' && cap == null) return;
    if (!context.mounted) return;
    Map<String, dynamic> doc = {'kind': cap?.isPdf == true ? 'factuur' : 'bonnetje', 'date': today(), 'status': 'concept', 'lines': []};
    Uint8List? file = cap?.bytes;

    if (cap != null) {
      try {
        if (cap.isPdf) {
          // The server reads the PDF's text layer and keeps the file.
          final up = await Api.instance.upload(cap.bytes);
          file = null;
          doc['file_sha'] = up['sha'];
          doc['file_type'] = up['type'];
          doc['ocr_text'] = up['text'] ?? '';
          if (up['draft'] != null) doc = _applyDraft(doc, Map<String, dynamic>.from(up['draft'] as Map));
        } else if (cap.ocrText.isNotEmpty) {
          doc['ocr_text'] = cap.ocrText;
          final draft = Map<String, dynamic>.from(await Api.instance.post('/api/parse', {'text': cap.ocrText}) as Map);
          doc = _applyDraft(doc, draft);
        }
      } on ApiException catch (e) {
        if (!e.offline) {
          if (context.mounted) showError(context, e);
          return;
        }
        // Offline: keep the photo and its text; the server fills it in later.
        await Api.instance.createDocument({...doc, 'ocr_text': cap.ocrText}, cap.bytes);
        if (context.mounted) showInfo(context, 'Bewaard. Hij wordt verstuurd en ingevuld zodra er verbinding is.');
        onDone?.call();
        return;
      }
    }
    if (!context.mounted) return;
    await Navigator.push(context, MaterialPageRoute(builder: (_) => DocumentEditor(doc: doc, file: file)));
    onDone?.call();
  }

  static Map<String, dynamic> _applyDraft(Map<String, dynamic> doc, Map<String, dynamic> d) {
    return {
      ...doc,
      'date': d['date'] ?? doc['date'],
      'total_cents': d['total_cents'] ?? 0,
      'supplier_name': d['supplier_name'] ?? '',
      'supplier_id': d['supplier_id'],
      'parse_notes': ((d['notes'] as List?) ?? []).join('\n'),
      'lines': [
        for (final l in (d['lines'] as List? ?? []).cast<Map>())
          {'amount_excl_cents': l['amount_excl_cents'], 'vat_rate': l['vat_rate'], 'vat_cents': l['vat_cents'], 'category_id': d['category_id'], 'business_pct': 100},
      ],
    };
  }

  @override
  Widget build(BuildContext context) {
    return FloatingActionButton.extended(
      onPressed: () async {
        if (canUseCamera) return _start(context, 'camera');
        final how = await showModalBottomSheet<String>(
          context: context,
          builder: (c) => SafeArea(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              ListTile(leading: const Icon(Icons.picture_as_pdf_outlined), title: const Text('PDF of foto kiezen'), onTap: () => Navigator.pop(c, 'file')),
              ListTile(leading: const Icon(Icons.edit_outlined), title: const Text('Met de hand invoeren'), onTap: () => Navigator.pop(c, 'manual')),
            ]),
          ),
        );
        if (how != null && context.mounted) await _start(context, how);
      },
      icon: Icon(canUseCamera ? Icons.document_scanner_outlined : Icons.add),
      label: Text(canUseCamera ? 'Scan bon' : 'Kosten toevoegen'),
    ).withLongPress(context, (how) => _start(context, how));
  }
}

extension on FloatingActionButton {
  /// On the phone a long press offers the file picker and manual entry.
  Widget withLongPress(BuildContext context, Future<void> Function(String) start) {
    if (!canUseCamera) return this;
    return GestureDetector(
      onLongPress: () async {
        final how = await showModalBottomSheet<String>(
          context: context,
          builder: (c) => SafeArea(
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              ListTile(leading: const Icon(Icons.picture_as_pdf_outlined), title: const Text('PDF of foto kiezen'), onTap: () => Navigator.pop(c, 'file')),
              ListTile(leading: const Icon(Icons.edit_outlined), title: const Text('Met de hand invoeren'), onTap: () => Navigator.pop(c, 'manual')),
            ]),
          ),
        );
        if (how != null) await start(how);
      },
      child: this,
    );
  }
}

class _Line {
  final String? id;
  final TextEditingController description;
  final TextEditingController excl;
  final TextEditingController vat;
  String rate;
  String? category;
  int businessPct;
  _Line(Map l)
      : id = l['id'] as String?,
        description = TextEditingController(text: l['description'] as String? ?? ''),
        excl = TextEditingController(text: centsInput(l['amount_excl_cents'] as int? ?? 0)),
        vat = TextEditingController(text: centsInput(l['vat_cents'] as int? ?? 0)),
        rate = l['vat_rate'] as String? ?? '21',
        category = l['category_id'] as String?,
        businessPct = l['business_pct'] as int? ?? 100;

  int get exclCents => parseEur(excl.text) ?? 0;
  int get vatCents => int.tryParse(rate) == null ? 0 : parseEur(vat.text) ?? 0;

  Map<String, dynamic> toJson() => {
        'description': description.text.trim(),
        'amount_excl_cents': exclCents,
        'vat_rate': rate,
        'vat_cents': vatCents,
        'category_id': category,
        'business_pct': businessPct,
      };
}

class DocumentEditor extends StatefulWidget {
  final Map<String, dynamic> doc;
  final Uint8List? file;
  const DocumentEditor({super.key, required this.doc, this.file});

  @override
  State<DocumentEditor> createState() => _DocumentEditorState();
}

class _DocumentEditorState extends State<DocumentEditor> {
  final _form = GlobalKey<FormState>();
  late Map<String, dynamic> d = Map.of(widget.doc);
  late final _supplier = TextEditingController(text: d['supplier_name'] as String? ?? '');
  late final _total = TextEditingController(text: centsInput(d['total_cents'] as int? ?? 0));
  late final _note = TextEditingController(text: d['note'] as String? ?? '');
  late final List<_Line> _lines = [for (final l in (d['lines'] as List? ?? []).cast<Map>()) _Line(l)];
  List<Map<String, dynamic>> _cats = [];
  bool _busy = false;

  bool get _existing => d['created_at'] != null;

  @override
  void initState() {
    super.initState();
    if (_lines.isEmpty) _lines.add(_Line({'vat_rate': '21'}));
    Categories.all().then((c) => mounted ? setState(() => _cats = c.where((x) => x['archived'] != true).toList()) : null).catchError((_) => null);
  }

  int get _linesTotal => _lines.fold(0, (s, l) => s + l.exclCents + l.vatCents);

  /// One line taking the whole total at its rate.
  void _spread() {
    final total = parseEur(_total.text) ?? 0;
    final l = _lines.first;
    final vat = vatFromIncl(total, l.rate);
    setState(() {
      l.excl.text = centsInput(total - vat);
      l.vat.text = centsInput(vat);
      _lines.removeRange(1, _lines.length);
    });
  }

  Future<void> _save(String status) async {
    if (!_form.currentState!.validate()) return;
    if (status == 'bevestigd' && (_linesTotal - (parseEur(_total.text) ?? 0)).abs() > 2) {
      final go = await confirm(context, 'De regels tellen op tot ${eur(_linesTotal)}, het totaal is ${_total.text}. Toch bevestigen?', ok: 'Bevestigen');
      if (!go) return;
    }
    final body = {
      ...d,
      'supplier_name': _supplier.text.trim(),
      'total_cents': parseEur(_total.text) ?? 0,
      'note': _note.text.trim(),
      'status': status,
      'lines': _lines.map((l) => l.toJson()).toList(),
    };
    for (final k in ['created_at', 'updated_at']) {
      body.remove(k);
    }
    if (!mounted) return;
    setState(() => _busy = true);
    Map<String, dynamic>? saved;
    final ok = await guard(context, () async {
      if (_existing) {
        saved = await Api.instance.save('documents', {...body, 'created_at': d['created_at']});
      } else {
        saved = await Api.instance.createDocument(body, widget.file);
        if (saved == null && mounted) showInfo(context, 'Offline bewaard; wordt verstuurd zodra er verbinding is.');
      }
      return true;
    });
    if (!mounted) return;
    setState(() => _busy = false);
    if (ok == true) Navigator.pop(context, saved);
  }

  Future<void> _delete() async {
    if (!await confirm(context, 'Deze kosten verwijderen? Het bestand blijft bewaard (bewaarplicht).', ok: 'Verwijderen')) return;
    if (!mounted) return;
    final ok = await guard(context, () async {
      await Api.instance.remove('documents', d['id'] as String);
      return true;
    });
    if (ok == true && mounted) Navigator.pop(context);
  }

  Future<void> _openFile() async {
    await guard(context, () async {
      final bytes = widget.file ?? await Api.instance.bytes('/api/files/${d['file_sha']}');
      final ext = (d['file_type'] as String? ?? '').contains('pdf') ? 'pdf' : 'jpg';
      final dir = await getTemporaryDirectory();
      final f = File('${dir.path}/bon-${d['id'] ?? 'nieuw'}.$ext');
      await f.writeAsBytes(bytes);
      await launchUrl(Uri.file(f.path));
    });
  }

  @override
  Widget build(BuildContext context) {
    final hasFile = widget.file != null || (d['file_sha'] as String? ?? '').isNotEmpty;
    final isImage = widget.file != null && !(d['kind'] == 'factuur' && (d['file_type'] as String? ?? '').contains('pdf'));
    final notes = (d['parse_notes'] as String? ?? '').trim();
    return Scaffold(
      appBar: AppBar(
        title: Text(_existing ? 'Kosten' : 'Nieuwe kosten'),
        actions: [
          if (hasFile) IconButton(onPressed: _openFile, icon: const Icon(Icons.open_in_new), tooltip: 'Bestand openen'),
          if (_existing) IconButton(onPressed: _delete, icon: const Icon(Icons.delete_outline), tooltip: 'Verwijderen'),
        ],
      ),
      body: Form(
        key: _form,
        child: ListView(padding: const EdgeInsets.all(16), children: [
          Readable(
            child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              if (isImage)
                Padding(
                  padding: const EdgeInsets.only(bottom: 16),
                  child: ClipRRect(borderRadius: BorderRadius.circular(8), child: Image.memory(widget.file!, height: 220, fit: BoxFit.cover)),
                )
              else if (_existing && (d['file_type'] as String? ?? '').startsWith('image'))
                Padding(
                  padding: const EdgeInsets.only(bottom: 16),
                  child: FutureBuilder<Uint8List>(
                    future: Api.instance.bytes('/api/files/${d['file_sha']}'),
                    builder: (c, s) => s.hasData
                        ? ClipRRect(borderRadius: BorderRadius.circular(8), child: Image.memory(s.data!, height: 220, fit: BoxFit.cover))
                        : const SizedBox(height: 40),
                  ),
                ),
              if (notes.isNotEmpty) Notice(notes),
              SegmentedButton<String>(
                segments: const [
                  ButtonSegment(value: 'bonnetje', label: Text('Bonnetje'), icon: Icon(Icons.receipt_outlined)),
                  ButtonSegment(value: 'factuur', label: Text('Factuur'), icon: Icon(Icons.description_outlined)),
                ],
                selected: {d['kind'] as String? ?? 'bonnetje'},
                onSelectionChanged: (s) => setState(() => d['kind'] = s.first),
              ),
              const SizedBox(height: 16),
              TextFormField(controller: _supplier, decoration: const InputDecoration(labelText: 'Leverancier / winkel'), textCapitalization: TextCapitalization.words),
              const SizedBox(height: 12),
              Row(children: [
                Expanded(child: DateField(label: 'Datum', value: d['date'] as String?, onChanged: (v) => setState(() => d['date'] = v))),
                const SizedBox(width: 12),
                Expanded(child: MoneyField(label: 'Totaal incl. btw', controller: _total, onChanged: (_) => setState(() {}))),
              ]),
              const SizedBox(height: 20),
              Row(children: [
                Text('Regels', style: Theme.of(context).textTheme.titleSmall),
                const Spacer(),
                TextButton.icon(onPressed: _spread, icon: const Icon(Icons.auto_fix_high, size: 18), label: const Text('Totaal op één regel')),
              ]),
              for (var i = 0; i < _lines.length; i++) _lineCard(i),
              Align(
                alignment: Alignment.centerLeft,
                child: TextButton.icon(onPressed: () => setState(() => _lines.add(_Line({'vat_rate': '21', 'category_id': _lines.last.category}))), icon: const Icon(Icons.add), label: const Text('Regel toevoegen')),
              ),
              _sumLine(context),
              const SizedBox(height: 12),
              SwitchListTile(
                contentPadding: EdgeInsets.zero,
                title: const Text('Privé betaald'),
                subtitle: const Text('Met een privérekening of contant; geen banktransactie verwacht'),
                value: d['paid_private'] == true,
                onChanged: (v) => setState(() => d['paid_private'] = v),
              ),
              TextFormField(controller: _note, decoration: const InputDecoration(labelText: 'Notitie'), maxLines: 2),
              const SizedBox(height: 24),
              Row(children: [
                Expanded(child: OutlinedButton(onPressed: _busy ? null : () => _save('concept'), child: const Text('Later afmaken'))),
                const SizedBox(width: 12),
                Expanded(child: FilledButton(onPressed: _busy ? null : () => _save('bevestigd'), child: const Text('Bevestigen'))),
              ]),
              const SizedBox(height: 32),
            ]),
          ),
        ]),
      ),
    );
  }

  Widget _sumLine(BuildContext context) {
    final total = parseEur(_total.text) ?? 0;
    final diff = _linesTotal - total;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Text(
        diff.abs() <= 2 ? 'Regels: ${eur(_linesTotal)} ✓' : 'Regels: ${eur(_linesTotal)}, verschil met totaal ${eur(diff)}',
        style: amountStyle(context, size: 13, color: diff.abs() <= 2 ? green : red),
      ),
    );
  }

  Widget _lineCard(int i) {
    final l = _lines[i];
    final reverse = l.rate.startsWith('verlegd');
    final noVat = int.tryParse(l.rate) == null || l.rate == '0';
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(children: [
            Row(children: [
              Expanded(child: TextFormField(controller: l.description, decoration: const InputDecoration(labelText: 'Omschrijving (optioneel)'))),
              if (_lines.length > 1) IconButton(onPressed: () => setState(() => _lines.removeAt(i)), icon: const Icon(Icons.close)),
            ]),
            const SizedBox(height: 10),
            Row(children: [
              Expanded(
                flex: 3,
                child: DropdownButtonFormField<String>(
                  initialValue: l.rate,
                  isExpanded: true,
                  decoration: const InputDecoration(labelText: 'Btw'),
                  items: [for (final e in vatRates.entries) DropdownMenuItem(value: e.key, child: Text(e.value))],
                  onChanged: (v) => setState(() {
                    l.rate = v ?? '21';
                    final incl = l.exclCents + (parseEur(l.vat.text) ?? 0);
                    final vat = vatFromIncl(incl, l.rate);
                    l.excl.text = centsInput(int.tryParse(l.rate) == null ? incl : incl - vat);
                    l.vat.text = centsInput(vat);
                  }),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(flex: 3, child: MoneyField(label: 'Excl. btw', controller: l.excl, onChanged: (_) => setState(() {}))),
              if (!noVat) ...[
                const SizedBox(width: 10),
                Expanded(flex: 2, child: MoneyField(label: 'Btw', controller: l.vat, onChanged: (_) => setState(() {}))),
              ],
            ]),
            if (reverse)
              const Padding(
                padding: EdgeInsets.only(top: 8),
                child: Text('Verlegde btw: je geeft 21% aan en trekt het in dezelfde aangifte weer af.', style: TextStyle(fontSize: 12)),
              ),
            const SizedBox(height: 10),
            Row(children: [
              Expanded(
                flex: 3,
                child: DropdownButtonFormField<String>(
                  initialValue: _cats.any((c) => c['id'] == l.category) ? l.category : null,
                  isExpanded: true,
                  decoration: const InputDecoration(labelText: 'Categorie'),
                  items: [for (final c in _cats) DropdownMenuItem(value: c['id'] as String, child: Text(c['name'] as String, overflow: TextOverflow.ellipsis))],
                  onChanged: (v) => setState(() => l.category = v),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                flex: 2,
                child: DropdownButtonFormField<int>(
                  initialValue: [100, 75, 50, 25].contains(l.businessPct) ? l.businessPct : 100,
                  decoration: const InputDecoration(labelText: 'Zakelijk'),
                  items: const [
                    DropdownMenuItem(value: 100, child: Text('100%')),
                    DropdownMenuItem(value: 75, child: Text('75%')),
                    DropdownMenuItem(value: 50, child: Text('50%')),
                    DropdownMenuItem(value: 25, child: Text('25%')),
                  ],
                  onChanged: (v) => setState(() => l.businessPct = v ?? 100),
                ),
              ),
            ]),
          ]),
        ),
      ),
    );
  }
}
