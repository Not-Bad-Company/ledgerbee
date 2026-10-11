import 'package:flutter/material.dart';

const night = Color(0xFF07080B);
const surface = Color(0xFF0C0E12);
const raised = Color(0xFF12151B);
const rule = Color(0x299AA3B2);
const fg = Color(0xFFE4E8EE);
const muted = Color(0xFF9AA3B2);
const faint = Color(0xFF748090);
const accent = Color(0xFFD9A441);

const green = Color(0xFF86C9A4);
const red = Color(0xFFEF8F7F);
const amber = accent;

const display = 'Schibsted Grotesk';
const sans = 'Public Sans';
const mono = 'JetBrains Mono';

const tabular = [FontFeature.tabularFigures()];

const _square = RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(2)));
const _ruled = RoundedRectangleBorder(borderRadius: BorderRadius.all(Radius.circular(2)), side: BorderSide(color: rule));

TextStyle labelStyle({Color color = muted, double size = 12}) => TextStyle(fontFamily: mono, fontSize: size, color: color, letterSpacing: 0.2, height: 1.4);

ThemeData buildTheme() {
  final scheme = ColorScheme.fromSeed(seedColor: accent, brightness: Brightness.dark).copyWith(
    surface: night,
    surfaceContainerLowest: night,
    surfaceContainerLow: surface,
    surfaceContainer: surface,
    surfaceContainerHigh: raised,
    surfaceContainerHighest: raised,
    primary: accent,
    onPrimary: night,
    primaryContainer: const Color(0xFF3A2C10),
    onPrimaryContainer: accent,
    secondary: muted,
    secondaryContainer: const Color(0xFF1B1F27),
    onSecondaryContainer: fg,
    onSurface: fg,
    onSurfaceVariant: muted,
    outline: faint,
    outlineVariant: rule,
    error: red,
    onError: night,
  );
  final base = ThemeData(colorScheme: scheme, useMaterial3: true, brightness: Brightness.dark, fontFamily: sans);
  TextStyle? d(TextStyle? s, FontWeight w, [double spacing = -0.3]) => s?.copyWith(fontFamily: display, fontWeight: w, letterSpacing: spacing, color: fg);
  final text = base.textTheme.apply(bodyColor: fg, displayColor: fg).copyWith(
        displaySmall: d(base.textTheme.displaySmall, FontWeight.w700, -1),
        headlineMedium: d(base.textTheme.headlineMedium, FontWeight.w700, -0.8),
        headlineSmall: d(base.textTheme.headlineSmall, FontWeight.w700, -0.5),
        titleLarge: d(base.textTheme.titleLarge, FontWeight.w700),
        titleMedium: base.textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w600, color: fg),
        bodySmall: base.textTheme.bodySmall?.copyWith(color: muted),
      );
  const monoButton = TextStyle(fontFamily: mono, fontSize: 14, fontWeight: FontWeight.w500, letterSpacing: 0.2);
  return base.copyWith(
    textTheme: text,
    scaffoldBackgroundColor: night,
    canvasColor: night,
    appBarTheme: const AppBarTheme(
      backgroundColor: night,
      surfaceTintColor: Colors.transparent,
      foregroundColor: fg,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      titleTextStyle: TextStyle(fontFamily: display, fontSize: 24, fontWeight: FontWeight.w700, letterSpacing: -0.5, color: fg),
      shape: Border(bottom: BorderSide(color: rule)),
    ),
    cardTheme: const CardThemeData(elevation: 0, color: surface, surfaceTintColor: Colors.transparent, margin: EdgeInsets.zero, clipBehavior: Clip.antiAlias, shape: _ruled),
    inputDecorationTheme: const InputDecorationTheme(
      filled: true,
      fillColor: surface,
      isDense: true,
      border: OutlineInputBorder(borderRadius: BorderRadius.all(Radius.circular(2)), borderSide: BorderSide(color: rule)),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.all(Radius.circular(2)), borderSide: BorderSide(color: rule)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.all(Radius.circular(2)), borderSide: BorderSide(color: accent, width: 1.2)),
      labelStyle: TextStyle(color: muted),
      floatingLabelStyle: TextStyle(fontFamily: mono, fontSize: 13, color: accent),
      helperStyle: TextStyle(color: faint),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(backgroundColor: accent, foregroundColor: night, shape: _square, textStyle: monoButton, minimumSize: const Size(48, 46)),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(foregroundColor: accent, side: const BorderSide(color: accent), shape: _square, textStyle: monoButton, minimumSize: const Size(48, 46)),
    ),
    textButtonTheme: TextButtonThemeData(style: TextButton.styleFrom(foregroundColor: accent, shape: _square, textStyle: monoButton)),
    floatingActionButtonTheme: const FloatingActionButtonThemeData(
      backgroundColor: accent,
      foregroundColor: night,
      elevation: 0,
      focusElevation: 0,
      hoverElevation: 0,
      highlightElevation: 0,
      shape: _square,
      extendedTextStyle: monoButton,
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: night,
      surfaceTintColor: Colors.transparent,
      indicatorColor: const Color(0xFF3A2C10),
      indicatorShape: _square,
      height: 66,
      iconTheme: WidgetStateProperty.resolveWith((s) => IconThemeData(color: s.contains(WidgetState.selected) ? accent : muted)),
      labelTextStyle: WidgetStateProperty.resolveWith((s) => TextStyle(fontFamily: mono, fontSize: 11, color: s.contains(WidgetState.selected) ? fg : muted)),
    ),
    navigationRailTheme: const NavigationRailThemeData(
      backgroundColor: night,
      indicatorColor: Color(0xFF3A2C10),
      indicatorShape: _square,
      selectedIconTheme: IconThemeData(color: accent),
      unselectedIconTheme: IconThemeData(color: muted),
      selectedLabelTextStyle: TextStyle(fontFamily: mono, fontSize: 11, color: fg),
      unselectedLabelTextStyle: TextStyle(fontFamily: mono, fontSize: 11, color: muted),
    ),
    tabBarTheme: const TabBarThemeData(
      labelColor: fg,
      unselectedLabelColor: muted,
      indicatorColor: accent,
      dividerColor: rule,
      labelStyle: TextStyle(fontFamily: mono, fontSize: 13, fontWeight: FontWeight.w500),
      unselectedLabelStyle: TextStyle(fontFamily: mono, fontSize: 13),
    ),
    bottomSheetTheme: const BottomSheetThemeData(backgroundColor: raised, surfaceTintColor: Colors.transparent, showDragHandle: true, shape: _square),
    dialogTheme: const DialogThemeData(backgroundColor: raised, surfaceTintColor: Colors.transparent, shape: _ruled),
    popupMenuTheme: const PopupMenuThemeData(color: raised, surfaceTintColor: Colors.transparent, shape: _ruled),
    snackBarTheme: const SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: raised,
      contentTextStyle: TextStyle(fontFamily: sans, color: fg),
      actionTextColor: accent,
      shape: _ruled,
    ),
    chipTheme: base.chipTheme.copyWith(
      backgroundColor: night,
      selectedColor: const Color(0xFF3A2C10),
      side: const BorderSide(color: rule),
      shape: _square,
      labelStyle: const TextStyle(fontFamily: mono, fontSize: 12, color: fg),
    ),
    segmentedButtonTheme: SegmentedButtonThemeData(
      style: SegmentedButton.styleFrom(
        shape: _square,
        side: const BorderSide(color: rule),
        selectedBackgroundColor: const Color(0xFF3A2C10),
        selectedForegroundColor: accent,
        textStyle: const TextStyle(fontFamily: mono, fontSize: 12),
      ),
    ),
    switchTheme: SwitchThemeData(
      thumbColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? night : muted),
      trackColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? accent : surface),
      trackOutlineColor: WidgetStateProperty.resolveWith((s) => s.contains(WidgetState.selected) ? accent : rule),
    ),
    listTileTheme: const ListTileThemeData(contentPadding: EdgeInsets.symmetric(horizontal: 16), iconColor: muted),
    dividerTheme: const DividerThemeData(color: rule, space: 1, thickness: 1),
    progressIndicatorTheme: const ProgressIndicatorThemeData(color: accent, linearTrackColor: rule, circularTrackColor: Colors.transparent),
  );
}

TextStyle amountStyle(BuildContext context, {double? size, FontWeight weight = FontWeight.w600, Color? color}) =>
    TextStyle(fontFamily: sans, fontFeatures: tabular, fontSize: size, fontWeight: weight, color: color ?? fg);

TextStyle figureStyle({double size = 48, Color color = fg}) =>
    TextStyle(fontFamily: display, fontFeatures: tabular, fontSize: size, fontWeight: FontWeight.w700, letterSpacing: -size * 0.035, height: 1, color: color);
