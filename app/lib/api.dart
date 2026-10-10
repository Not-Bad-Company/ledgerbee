// The server connection: login, JSON requests, file uploads, a read cache
// so the app opens without a network, and an outbox so a receipt scanned
// offline is sent once the server is reachable again.

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:uuid/uuid.dart';

class ApiException implements Exception {
  final int status;
  final String message;
  ApiException(this.status, this.message);
  bool get offline => status == 0;
  @override
  String toString() => message;
}

const _uuid = Uuid();
String newId() => _uuid.v4();

class Api extends ChangeNotifier {
  Api._();
  static final Api instance = Api._();

  static const _storage = FlutterSecureStorage();
  late SharedPreferences _prefs;
  String baseUrl = '';
  String? _token;
  bool online = true;
  int pending = 0;
  bool _flushing = false;

  bool get signedIn => _token != null && baseUrl.isNotEmpty;

  Future<void> init() async {
    _prefs = await SharedPreferences.getInstance();
    baseUrl = _prefs.getString('base_url') ?? '';
    try {
      _token = await _storage.read(key: 'token');
    } catch (_) {
      // No keyring on this desktop session: fall back to preferences.
      _token = _prefs.getString('token_fallback');
    }
    pending = _outbox().length;
    // Debug builds only: start signed in to a test server (screenshots, tests).
    const testUrl = String.fromEnvironment('BOEK_URL');
    const testToken = String.fromEnvironment('BOEK_TOKEN');
    if (kDebugMode && testUrl != '' && testToken != '') {
      baseUrl = testUrl;
      _token = testToken;
    }
  }

  Future<void> _saveToken(String? token) async {
    _token = token;
    try {
      if (token == null) {
        await _storage.delete(key: 'token');
      } else {
        await _storage.write(key: 'token', value: token);
      }
    } catch (_) {
      if (token == null) {
        await _prefs.remove('token_fallback');
      } else {
        await _prefs.setString('token_fallback', token);
      }
    }
  }

  Future<void> login(String server, String email, String password, String code) async {
    var url = server.trim();
    if (!url.startsWith('http')) url = 'https://$url';
    url = url.replaceAll(RegExp(r'/+$'), '');
    final res = await _send('POST', '$url/api/login', body: {
      'email': email.trim(),
      'password': password,
      'code': code.trim(),
      'device': Platform.isAndroid ? 'Android-telefoon' : 'Linux (${Platform.localHostname})',
    }, auth: false);
    baseUrl = url;
    await _prefs.setString('base_url', url);
    await _saveToken(res['token'] as String);
    notifyListeners();
  }

  Future<void> logout() async {
    await _saveToken(null);
    await _prefs.remove('cache');
    notifyListeners();
  }

  Map<String, String> get _headers => {
        'content-type': 'application/json',
        if (_token != null) 'authorization': 'Bearer $_token',
      };

  Future<dynamic> _send(String method, String url, {Object? body, bool auth = true, Uint8List? bytes}) async {
    final req = http.Request(method, Uri.parse(url));
    req.headers.addAll(auth ? _headers : {'content-type': 'application/json'});
    if (bytes != null) {
      req.headers['content-type'] = 'application/octet-stream';
      req.bodyBytes = bytes;
    } else if (body != null) {
      req.body = jsonEncode(body);
    }
    http.StreamedResponse streamed;
    try {
      streamed = await req.send().timeout(Duration(seconds: bytes != null ? 120 : 20));
    } on Exception catch (_) {
      _setOnline(false);
      throw ApiException(0, 'Geen verbinding met de server');
    }
    _setOnline(true);
    final res = await http.Response.fromStream(streamed);
    if (res.statusCode == 401 && auth) {
      await _saveToken(null);
      notifyListeners();
      throw ApiException(401, 'Opnieuw inloggen');
    }
    final ct = res.headers['content-type'] ?? '';
    if (res.statusCode >= 400) {
      String msg = 'Fout ${res.statusCode}';
      try {
        msg = (jsonDecode(res.body) as Map)['error'] as String? ?? msg;
      } catch (_) {}
      throw ApiException(res.statusCode, msg);
    }
    if (ct.contains('json')) return jsonDecode(utf8.decode(res.bodyBytes));
    return res.bodyBytes;
  }

  void _setOnline(bool value) {
    if (online != value) {
      online = value;
      notifyListeners();
      if (value) unawaited(flush());
    }
  }

  /// A GET that falls back to the last answer when the server is unreachable.
  Future<dynamic> get(String path) async {
    try {
      final res = await _send('GET', '$baseUrl$path');
      _cachePut(path, res);
      return res;
    } on ApiException catch (e) {
      if (e.offline) {
        final cached = _cacheGet(path);
        if (cached != null) return cached;
      }
      rethrow;
    }
  }

  Future<dynamic> post(String path, [Object? body]) => _send('POST', '$baseUrl$path', body: body ?? {});
  Future<dynamic> put(String path, Object body) => _send('PUT', '$baseUrl$path', body: body);
  Future<dynamic> delete(String path) => _send('DELETE', '$baseUrl$path');
  Future<Uint8List> bytes(String path) async => await _send('GET', '$baseUrl$path') as Uint8List;
  Future<Map<String, dynamic>> upload(Uint8List data) async =>
      Map<String, dynamic>.from(await _send('POST', '$baseUrl/api/files', bytes: data) as Map);
  Future<Map<String, dynamic>> uploadTo(String path, Uint8List data) async =>
      Map<String, dynamic>.from(await _send('POST', '$baseUrl$path', bytes: data) as Map);

  // Typed helpers for the generic tables.
  Future<List<Map<String, dynamic>>> list(String entity, {String? from, String? to, String? status}) async {
    final q = <String, String>{'from': ?from, 'to': ?to, 'status': ?status};
    final query = q.isEmpty ? '' : '?${Uri(queryParameters: q).query}';
    final res = await get('/api/data/$entity$query') as List;
    return res.map((e) => Map<String, dynamic>.from(e as Map)).toList();
  }

  Future<Map<String, dynamic>> save(String entity, Map<String, dynamic> row) async {
    final id = row['id'] as String?;
    final exists = row['created_at'] != null;
    final res = exists ? await put('/api/data/$entity/$id', row) : await post('/api/data/$entity', {...row, 'id': id ?? newId()});
    return Map<String, dynamic>.from(res as Map);
  }

  Future<void> remove(String entity, String id) => delete('/api/data/$entity/$id');

  // Cache.
  Map<String, dynamic> _cache() {
    try {
      return Map<String, dynamic>.from(jsonDecode(_prefs.getString('cache') ?? '{}') as Map);
    } catch (_) {
      return {};
    }
  }

  void _cachePut(String path, dynamic value) {
    if (value is Uint8List) return;
    final c = _cache();
    c[path] = value;
    // Bounded: the newest 60 answers.
    while (c.length > 60) {
      c.remove(c.keys.first);
    }
    _prefs.setString('cache', jsonEncode(c));
  }

  dynamic _cacheGet(String path) => _cache()[path];

  // Outbox: documents created while offline, with the photo on disk.
  List<Map<String, dynamic>> _outbox() {
    try {
      return (jsonDecode(_prefs.getString('outbox') ?? '[]') as List).map((e) => Map<String, dynamic>.from(e as Map)).toList();
    } catch (_) {
      return [];
    }
  }

  Future<void> _setOutbox(List<Map<String, dynamic>> items) async {
    await _prefs.setString('outbox', jsonEncode(items));
    pending = items.length;
    notifyListeners();
  }

  /// Sends a new document now, or keeps it (and its file) until the server
  /// is reachable. The id is made here, so a resend creates nothing twice.
  Future<Map<String, dynamic>?> createDocument(Map<String, dynamic> doc, Uint8List? file) async {
    doc = {...doc, 'id': doc['id'] ?? newId()};
    try {
      if (file != null) {
        final up = await upload(file);
        doc['file_sha'] = up['sha'];
        doc['file_type'] = up['type'];
      }
      return await save('documents', doc);
    } on ApiException catch (e) {
      if (!e.offline) rethrow;
      String? filePath;
      if (file != null) {
        final dir = await getApplicationSupportDirectory();
        final f = File('${dir.path}/outbox-${doc['id']}');
        await f.writeAsBytes(file);
        filePath = f.path;
      }
      await _setOutbox([..._outbox(), {'doc': doc, 'file': filePath}]);
      return null;
    }
  }

  Future<void> flush() async {
    if (_flushing || !signedIn) return;
    _flushing = true;
    try {
      var items = _outbox();
      while (items.isNotEmpty) {
        final item = items.first;
        final doc = Map<String, dynamic>.from(item['doc'] as Map);
        final path = item['file'] as String?;
        try {
          if (path != null && File(path).existsSync() && doc['file_sha'] == null) {
            final up = await upload(await File(path).readAsBytes());
            doc['file_sha'] = up['sha'];
            doc['file_type'] = up['type'];
          }
          await post('/api/data/documents', doc);
          if (path != null && File(path).existsSync()) File(path).deleteSync();
        } on ApiException catch (e) {
          if (e.offline) break;
          // The server refused it for good (a filed period): drop it but say so.
          debugPrint('outbox item dropped: ${e.message}');
        }
        items = items.sublist(1);
        await _setOutbox(items);
      }
    } finally {
      _flushing = false;
    }
  }
}
