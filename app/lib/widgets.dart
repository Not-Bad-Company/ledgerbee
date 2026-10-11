import 'dart:math' as math;

import 'package:flutter/material.dart';

import 'api.dart';
import 'format.dart';
import 'theme.dart';

class Loaded<T> extends StatefulWidget {
  final Future<T> Function() load;
  final Widget Function(BuildContext context, T data, Future<void> Function() reload) builder;
  const Loaded({super.key, required this.load, required this.builder});

  @override
  State<Loaded<T>> createState() => LoadedState<T>();
}

class LoadedState<T> extends State<Loaded<T>> {
  T? _data;
  Object? _error;
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    reload();
  }

  Future<void> reload() async {
    setState(() => _loading = _data == null);
    try {
      final d = await widget.load();
      if (mounted) {
        setState(() {
          _data = d;
          _error = null;
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = e;
          _loading = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_loading) return const Center(child: CircularProgressIndicator());
    if (_data == null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(mainAxisSize: MainAxisSize.min, children: [
            Text('$_error', textAlign: TextAlign.center),
            const SizedBox(height: 12),
            OutlinedButton(onPressed: reload, child: const Text('Opnieuw')),
          ]),
        ),
      );
    }
    return RefreshIndicator(onRefresh: reload, child: widget.builder(context, _data as T, reload));
  }
}

void showError(BuildContext context, Object e) {
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$e'), backgroundColor: Theme.of(context).colorScheme.error));
}

void showInfo(BuildContext context, String text) {
  ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
}

Future<bool> confirm(BuildContext context, String text, {String ok = 'Ja'}) async {
  final r = await showDialog<bool>(
    context: context,
    builder: (c) => AlertDialog(
      content: Text(text),
      actions: [
        TextButton(onPressed: () => Navigator.pop(c, false), child: const Text('Annuleren')),
        FilledButton(onPressed: () => Navigator.pop(c, true), child: Text(ok)),
      ],
    ),
  );
  return r ?? false;
}

Future<T?> guard<T>(BuildContext context, Future<T> Function() f) async {
  try {
    return await f();
  } catch (e) {
    if (context.mounted) showError(context, e);
    return null;
  }
}

class Section extends StatelessWidget {
  final String title;
  final Widget child;
  final Widget? trailing;
  const Section({super.key, required this.title, required this.child, this.trailing});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 20),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 0, 4, 8),
          child: Row(children: [
            Expanded(
              child: Text(title, style: labelStyle()),
            ),
            ?trailing,
          ]),
        ),
        Card(child: child),
      ]),
    );
  }
}

class AmountRow extends StatelessWidget {
  final String label;
  final num? cents;
  final String? text;
  final bool strong;
  final String? hint;
  const AmountRow(this.label, this.cents, {super.key, this.strong = false, this.text, this.hint});

  @override
  Widget build(BuildContext context) {
    final style = amountStyle(context, weight: strong ? FontWeight.w700 : FontWeight.w400, size: strong ? 16 : 14);
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(label, style: TextStyle(fontWeight: strong ? FontWeight.w600 : FontWeight.w400)),
            if (hint != null) Text(hint!, style: Theme.of(context).textTheme.bodySmall?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
          ]),
        ),
        const SizedBox(width: 12),
        Text(text ?? eur(cents), style: style),
      ]),
    );
  }
}

class EmptyState extends StatelessWidget {
  final IconData icon;
  final String text;
  final Widget? action;
  const EmptyState({super.key, required this.icon, required this.text, this.action});

  @override
  Widget build(BuildContext context) {
    return ListView(children: [
      const SizedBox(height: 80),
      Icon(icon, size: 48, color: Theme.of(context).colorScheme.outline),
      const SizedBox(height: 12),
      Padding(padding: const EdgeInsets.symmetric(horizontal: 32), child: Text(text, textAlign: TextAlign.center)),
      if (action != null) ...[const SizedBox(height: 16), Center(child: action!)],
    ]);
  }
}

class Notice extends StatelessWidget {
  final String text;
  final Color? color;
  final IconData icon;
  const Notice(this.text, {super.key, this.color, this.icon = Icons.info_outline});

  @override
  Widget build(BuildContext context) {
    final c = color ?? amber;
    return Container(
      margin: const EdgeInsets.only(bottom: 12),
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(color: c.withValues(alpha: 0.10), borderRadius: BorderRadius.circular(2), border: Border.all(color: c.withValues(alpha: 0.30))),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(icon, size: 18, color: c),
        const SizedBox(width: 10),
        Expanded(child: Text(text)),
      ]),
    );
  }
}

class DateField extends StatelessWidget {
  final String label;
  final String? value;
  final ValueChanged<String> onChanged;
  const DateField({super.key, required this.label, required this.value, required this.onChanged});

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: () async {
        final d = await showDatePicker(
          context: context,
          initialDate: parseIso(value) ?? DateTime.now(),
          firstDate: DateTime(2020),
          lastDate: DateTime(DateTime.now().year + 2),
          locale: const Locale('nl'),
        );
        if (d != null) onChanged(iso(d));
      },
      child: InputDecorator(
        decoration: InputDecoration(labelText: label, suffixIcon: const Icon(Icons.calendar_today, size: 18)),
        child: Text(longDate(value)),
      ),
    );
  }
}

class MoneyField extends StatelessWidget {
  final String label;
  final TextEditingController controller;
  final ValueChanged<int?>? onChanged;
  final String? helper;
  const MoneyField({super.key, required this.label, required this.controller, this.onChanged, this.helper});

  @override
  Widget build(BuildContext context) {
    return TextFormField(
      controller: controller,
      decoration: InputDecoration(labelText: label, prefixText: '€ ', helperText: helper),
      keyboardType: const TextInputType.numberWithOptions(decimal: true, signed: true),
      style: amountStyle(context),
      validator: (v) => v == null || v.trim().isEmpty || parseEur(v) != null ? null : 'Geen bedrag',
      onChanged: (v) => onChanged?.call(parseEur(v)),
    );
  }
}

class ConnectionBanner extends StatelessWidget {
  const ConnectionBanner({super.key});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Api.instance,
      builder: (context, _) {
        final api = Api.instance;
        if (api.online && api.pending == 0) return const SizedBox.shrink();
        final text = !api.online
            ? 'Offline${api.pending > 0 ? ' · ${api.pending} scan(s) wachten' : ''}: je ziet de laatst geladen gegevens'
            : '${api.pending} scan(s) worden verstuurd…';
        return Material(
          color: amber.withValues(alpha: 0.15),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 6),
            child: Row(children: [
              Icon(Icons.cloud_off, size: 16, color: amber),
              const SizedBox(width: 8),
              Expanded(child: Text(text, style: const TextStyle(fontSize: 13))),
              if (!api.online) TextButton(onPressed: () => api.flush().then((_) => api.get('/api/health')).catchError((_) => null), child: const Text('Opnieuw')),
            ]),
          ),
        );
      },
    );
  }
}

class Readable extends StatelessWidget {
  final Widget child;
  final double max;
  const Readable({super.key, required this.child, this.max = 860});

  @override
  Widget build(BuildContext context) =>
      Align(alignment: Alignment.topCenter, child: ConstrainedBox(constraints: BoxConstraints(maxWidth: max), child: child));
}

class AppMark extends StatelessWidget {
  final double size;
  const AppMark({super.key, this.size = 40});

  @override
  Widget build(BuildContext context) => Align(
        alignment: AlignmentDirectional.centerStart,
        widthFactor: 1,
        heightFactor: 1,
        child: SizedBox.square(dimension: size, child: CustomPaint(painter: _MarkPainter())),
      );
}

class _MarkPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    final w = size.width;
    canvas.drawRect(Offset.zero & size, Paint()..color = night);
    final c = Offset(w / 2, w / 2);
    final r = w * 0.34;
    final hex = Path();
    for (var i = 0; i < 6; i++) {
      final a = (-90 + 60 * i) * math.pi / 180;
      final p = c + Offset(r * math.cos(a), r * math.sin(a));
      i == 0 ? hex.moveTo(p.dx, p.dy) : hex.lineTo(p.dx, p.dy);
    }
    hex.close();
    canvas.drawPath(hex, Paint()
      ..color = accent
      ..style = PaintingStyle.stroke
      ..strokeWidth = w * 0.055
      ..strokeJoin = StrokeJoin.round);
    final fill = Paint()..color = accent;
    final t = w * 0.045;
    final left = c.dx - r / 2;
    canvas.drawRect(Rect.fromLTWH(left + r * 0.35, c.dy - r * 0.32, r * 0.65, t), fill);
    canvas.drawRect(Rect.fromLTWH(left, c.dy + r * 0.12, r, t), fill);
    canvas.drawRect(Rect.fromLTWH(left, c.dy + r * 0.12 + t + w * 0.035, r, t), fill);
  }

  @override
  bool shouldRepaint(_MarkPainter old) => false;
}
