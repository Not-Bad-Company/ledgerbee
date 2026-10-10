// Dark, after the one object every Dutch entrepreneur knows: the blue
// envelope. Deep night-blue surfaces, envelope blue for what needs doing,
// mint for money in, coral for money owed, and Manrope with tabular figures
// so columns of amounts line up.

import 'package:flutter/material.dart';

const night = Color(0xFF0C1424); // background
const _surface = Color(0xFF131D31); // cards
const _raised = Color(0xFF1A263E); // sheets, dialogs, inputs
const _line = Color(0xFF26344F);
const envelope = Color(0xFF2C63C9); // the envelope itself
const _envelopeLight = Color(0xFF8DB1FF); // envelope blue as text on night
const _text = Color(0xFFE7ECF5);
const _muted = Color(0xFF8F9DB8);

const green = Color(0xFF6CD3A5);
const red = Color(0xFFFF8B7B);
const amber = Color(0xFFF1C25B);

const tabular = [FontFeature.tabularFigures()];

ThemeData buildTheme() {
  final scheme = ColorScheme.fromSeed(seedColor: envelope, brightness: Brightness.dark).copyWith(
    surface: night,
    surfaceContainerLowest: night,
    surfaceContainerLow: _surface,
    surfaceContainer: _surface,
    surfaceContainerHigh: _raised,
    surfaceContainerHighest: _raised,
    primary: _envelopeLight,
    onPrimary: const Color(0xFF0A1A3D),
    primaryContainer: envelope,
    onPrimaryContainer: Colors.white,
    secondaryContainer: const Color(0xFF223A6B),
    onSecondaryContainer: _text,
    onSurface: _text,
    onSurfaceVariant: _muted,
    outline: const Color(0xFF4A5A78),
    outlineVariant: _line,
    error: red,
    onError: const Color(0xFF3A0B05),
  );
  final base = ThemeData(colorScheme: scheme, useMaterial3: true, brightness: Brightness.dark, fontFamily: 'Manrope');
  final text = base.textTheme.apply(bodyColor: _text, displayColor: _text).copyWith(
        headlineMedium: base.textTheme.headlineMedium?.copyWith(fontWeight: FontWeight.w700, letterSpacing: -0.6, color: _text),
        headlineSmall: base.textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.w700, letterSpacing: -0.4, color: _text),
        titleLarge: base.textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w700, letterSpacing: -0.3, color: _text),
        titleMedium: base.textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w600, color: _text),
      );
  final shape = RoundedRectangleBorder(borderRadius: BorderRadius.circular(16));
  return base.copyWith(
    textTheme: text,
    scaffoldBackgroundColor: night,
    canvasColor: night,
    appBarTheme: AppBarTheme(
      backgroundColor: night,
      surfaceTintColor: Colors.transparent,
      foregroundColor: _text,
      elevation: 0,
      scrolledUnderElevation: 0,
      centerTitle: false,
      titleTextStyle: text.headlineSmall,
    ),
    cardTheme: CardThemeData(
      elevation: 0,
      color: _surface,
      surfaceTintColor: Colors.transparent,
      margin: EdgeInsets.zero,
      clipBehavior: Clip.antiAlias,
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16), side: const BorderSide(color: _line)),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: _raised,
      isDense: true,
      border: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: BorderSide.none),
      enabledBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: const BorderSide(color: _line)),
      focusedBorder: OutlineInputBorder(borderRadius: BorderRadius.circular(12), borderSide: const BorderSide(color: _envelopeLight, width: 1.5)),
      labelStyle: const TextStyle(color: _muted),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        backgroundColor: envelope,
        foregroundColor: Colors.white,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
        textStyle: const TextStyle(fontFamily: 'Manrope', fontWeight: FontWeight.w700),
      ),
    ),
    outlinedButtonTheme: OutlinedButtonThemeData(
      style: OutlinedButton.styleFrom(shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)), side: const BorderSide(color: _line)),
    ),
    floatingActionButtonTheme: FloatingActionButtonThemeData(
      backgroundColor: envelope,
      foregroundColor: Colors.white,
      shape: shape,
      extendedTextStyle: const TextStyle(fontFamily: 'Manrope', fontWeight: FontWeight.w700),
    ),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: _surface,
      surfaceTintColor: Colors.transparent,
      indicatorColor: const Color(0xFF223A6B),
      height: 68,
      labelTextStyle: WidgetStateProperty.resolveWith((s) => TextStyle(
            fontFamily: 'Manrope',
            fontSize: 12,
            fontWeight: s.contains(WidgetState.selected) ? FontWeight.w700 : FontWeight.w500,
            color: s.contains(WidgetState.selected) ? _text : _muted,
          )),
    ),
    navigationRailTheme: const NavigationRailThemeData(
      backgroundColor: night,
      indicatorColor: Color(0xFF223A6B),
      selectedLabelTextStyle: TextStyle(fontFamily: 'Manrope', fontWeight: FontWeight.w700, fontSize: 12, color: _text),
      unselectedLabelTextStyle: TextStyle(fontFamily: 'Manrope', fontSize: 12, color: _muted),
    ),
    bottomSheetTheme: const BottomSheetThemeData(backgroundColor: _raised, surfaceTintColor: Colors.transparent, showDragHandle: true),
    dialogTheme: DialogThemeData(backgroundColor: _raised, surfaceTintColor: Colors.transparent, shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(20))),
    snackBarTheme: SnackBarThemeData(
      behavior: SnackBarBehavior.floating,
      backgroundColor: const Color(0xFF243453),
      contentTextStyle: const TextStyle(fontFamily: 'Manrope', color: _text),
      shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
    ),
    chipTheme: base.chipTheme.copyWith(backgroundColor: _raised, side: const BorderSide(color: _line), shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(10))),
    listTileTheme: const ListTileThemeData(contentPadding: EdgeInsets.symmetric(horizontal: 16), iconColor: _muted),
    dividerTheme: const DividerThemeData(color: _line, space: 1, thickness: 1),
    progressIndicatorTheme: const ProgressIndicatorThemeData(color: _envelopeLight, linearTrackColor: _line),
  );
}

TextStyle amountStyle(BuildContext context, {double? size, FontWeight weight = FontWeight.w600, Color? color}) =>
    TextStyle(fontFamily: 'Manrope', fontFeatures: tabular, fontSize: size, fontWeight: weight, color: color ?? Theme.of(context).colorScheme.onSurface);
