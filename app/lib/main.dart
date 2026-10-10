import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:intl/date_symbol_data_local.dart';

import 'dart:io';

import 'api.dart';
import 'scan.dart';
import 'notify.dart';
import 'screens/assets.dart';
import 'screens/bank.dart';
import 'screens/costs.dart';
import 'screens/hours.dart';
import 'screens/invoices.dart';
import 'screens/login.dart';
import 'screens/overview.dart';
import 'screens/reports.dart';
import 'screens/settings.dart';
import 'screens/trips.dart';
import 'theme.dart';
import 'widgets.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await initializeDateFormatting('nl');
  await Api.instance.init();
  await Notify.init();
  runApp(const BoekhoudingApp());
}

class BoekhoudingApp extends StatelessWidget {
  const BoekhoudingApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Boekhouding',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(),
      themeMode: ThemeMode.dark,
      locale: const Locale('nl'),
      supportedLocales: const [Locale('nl')],
      localizationsDelegates: GlobalMaterialLocalizations.delegates,
      home: ListenableBuilder(
        listenable: Api.instance,
        builder: (context, _) => Api.instance.signedIn ? const Shell() : const LoginScreen(),
      ),
    );
  }
}

class Destination {
  final String label;
  final IconData icon;
  final Widget Function() build;
  const Destination(this.label, this.icon, this.build);
}

final destinations = <Destination>[
  Destination('Overzicht', Icons.dashboard_outlined, () => const OverviewScreen()),
  Destination('Kosten', Icons.receipt_long_outlined, () => const CostsScreen()),
  Destination('Facturen', Icons.request_quote_outlined, () => const InvoicesScreen()),
  Destination('Bank', Icons.account_balance_outlined, () => const BankScreen()),
  Destination('Aangifte', Icons.fact_check_outlined, () => const ReportsScreen()),
  Destination('Uren', Icons.schedule_outlined, () => const HoursScreen()),
  Destination('Ritten', Icons.directions_car_outlined, () => const TripsScreen()),
  Destination('Activa', Icons.devices_outlined, () => const AssetsScreen()),
  Destination('Instellingen', Icons.settings_outlined, () => const SettingsScreen()),
];

/// Lets any screen switch the main section (the overview's cards do).
// Debug builds can open on another section (screenshots): --dart-define=BOEK_SECTION=2.
final ValueNotifier<int> currentSection = ValueNotifier(kDebugMode ? const int.fromEnvironment('BOEK_SECTION') : 0);

int sectionIndex(String label) => destinations.indexWhere((d) => d.label == label);

class Shell extends StatefulWidget {
  const Shell({super.key});

  @override
  State<Shell> createState() => _ShellState();
}

class _ShellState extends State<Shell> with WidgetsBindingObserver {
  static const _share = MethodChannel('boekhouding/share');

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    Api.instance.flush();
    Notify.scheduleDeadlines();
    if (Platform.isAndroid) {
      // A file shared to the app while it runs, or the one that started it.
      // See MainActivity.kt: it copies the shared file and sends its path.
      _share.setMethodCallHandler((call) async {
        if (call.method == 'shared') await _shared(call.arguments as String?);
      });
      _share.invokeMethod<String>('initial').then(_shared);
    }
  }

  Future<void> _shared(String? path) async {
    if (path == null || !mounted) return;
    try {
      final cap = await captureFile(await File(path).readAsBytes(), path.split('/').last, path);
      if (!mounted) return;
      currentSection.value = sectionIndex('Kosten');
      await openCapture(context, cap);
    } catch (e) {
      if (mounted) showError(context, friendlyError(e));
    }
  }

  @override
  void dispose() {
    if (Platform.isAndroid) _share.setMethodCallHandler(null);
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) Api.instance.flush();
  }

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<int>(
      valueListenable: currentSection,
      builder: (context, index, _) {
        final wide = MediaQuery.sizeOf(context).width >= 840;
        final body = Column(children: [
          const ConnectionBanner(),
          Expanded(child: KeyedSubtree(key: ValueKey(index), child: destinations[index].build())),
        ]);
        if (wide) {
          return Scaffold(
            body: Row(children: [
              NavigationRail(
                selectedIndex: index,
                onDestinationSelected: (i) => currentSection.value = i,
                labelType: NavigationRailLabelType.all,
                leading: const Padding(padding: EdgeInsets.symmetric(vertical: 16), child: AppMark(size: 36)),
                destinations: [for (final d in destinations) NavigationRailDestination(icon: Icon(d.icon), label: Text(d.label))],
              ),
              const VerticalDivider(width: 1),
              Expanded(child: body),
            ]),
          );
        }
        // Phone: four tabs and the rest under "Meer".
        const tabs = 4;
        final inMore = index >= tabs;
        return Scaffold(
          body: SafeArea(bottom: false, child: body),
          bottomNavigationBar: NavigationBar(
            selectedIndex: inMore ? tabs : index,
            onDestinationSelected: (i) async {
              if (i < tabs) {
                currentSection.value = i;
                return;
              }
              final picked = await showModalBottomSheet<int>(
                context: context,
                showDragHandle: true,
                builder: (c) => SafeArea(
                  child: Column(mainAxisSize: MainAxisSize.min, children: [
                    for (var j = tabs; j < destinations.length; j++)
                      ListTile(leading: Icon(destinations[j].icon), title: Text(destinations[j].label), onTap: () => Navigator.pop(c, j)),
                  ]),
                ),
              );
              if (picked != null) currentSection.value = picked;
            },
            destinations: [
              for (var j = 0; j < tabs; j++) NavigationDestination(icon: Icon(destinations[j].icon), label: destinations[j].label),
              NavigationDestination(icon: const Icon(Icons.more_horiz), label: inMore ? destinations[index].label : 'Meer'),
            ],
          ),
        );
      },
    );
  }
}
