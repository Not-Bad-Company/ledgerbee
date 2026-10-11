import 'dart:io';

import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:timezone/data/latest_all.dart' as tzdata;
import 'package:timezone/timezone.dart' as tz;

import 'api.dart';
import 'format.dart';

class Notify {
  static final _plugin = FlutterLocalNotificationsPlugin();
  static bool _ready = false;

  static const _details = NotificationDetails(
    android: AndroidNotificationDetails('deadlines', 'Deadlines', channelDescription: 'Btw- en inkomstenbelastingaangifte', importance: Importance.high),
    linux: LinuxNotificationDetails(),
  );

  static Future<void> init() async {
    try {
      tzdata.initializeTimeZones();
      tz.setLocalLocation(tz.getLocation('Europe/Amsterdam'));
      await _plugin.initialize(
        settings: const InitializationSettings(
          android: AndroidInitializationSettings('@mipmap/ic_launcher'),
          linux: LinuxInitializationSettings(defaultActionName: 'Openen'),
        ),
      );
      _ready = true;
    } catch (_) {
    }
  }

  static Future<void> scheduleDeadlines() async {
    if (!_ready) return;
    List deadlines;
    try {
      deadlines = await Api.instance.get('/api/deadlines') as List;
    } catch (_) {
      return;
    }
    if (Platform.isAndroid) {
      final android = _plugin.resolvePlatformSpecificImplementation<AndroidFlutterLocalNotificationsPlugin>();
      await android?.requestNotificationsPermission();
      await _plugin.cancelAll();
      var id = 1;
      final now = tz.TZDateTime.now(tz.local);
      for (final d in deadlines.cast<Map>()) {
        if (d['filed'] == true) continue;
        final due = parseIso(d['due'] as String)!;
        for (final before in [14, 7, 1, 0]) {
          final at = tz.TZDateTime(tz.local, due.year, due.month, due.day, 9).subtract(Duration(days: before));
          if (at.isBefore(now)) continue;
          await _plugin.zonedSchedule(
            id: id++,
            scheduledDate: at,
            notificationDetails: _details,
            androidScheduleMode: AndroidScheduleMode.inexactAllowWhileIdle,
            title: d['title'] as String,
            body: before == 0 ? 'Vandaag is de laatste dag.' : 'Nog $before dag${before == 1 ? '' : 'en'}: uiterlijk ${longDate(d['due'] as String)}.',
          );
        }
      }
    } else {
      final prefs = await SharedPreferences.getInstance();
      if (prefs.getString('notified_on') == today()) return;
      await prefs.setString('notified_on', today());
      var id = 1;
      for (final d in deadlines.cast<Map>()) {
        final left = d['days_left'] as int;
        if (d['filed'] == true || left > 14) continue;
        await _plugin.show(
          id: id++,
          title: d['title'] as String,
          body: left < 0 ? 'Deadline verstreken op ${longDate(d['due'] as String)}.' : 'Nog $left dag(en): uiterlijk ${longDate(d['due'] as String)}.',
          notificationDetails: _details,
        );
      }
    }
  }
}
