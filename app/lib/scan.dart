// Getting a receipt or invoice into the app: the camera with on-device text
// recognition (Android, ML Kit, offline), or a file (PDF or image) on either
// platform. Reading the text into amounts happens on the server, in one
// parser for every source.

import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:google_mlkit_text_recognition/google_mlkit_text_recognition.dart';
import 'package:image_picker/image_picker.dart';

class Capture {
  final Uint8List bytes;
  final String ocrText;
  final bool isPdf;
  Capture(this.bytes, this.ocrText, this.isPdf);
}

bool get canUseCamera => Platform.isAndroid;

/// The text on an image, or '' when it cannot be read. Never throws: a photo
/// without readable text is still worth keeping, and is filled in by hand.
Future<String> _ocr(String path) async {
  if (!Platform.isAndroid) return '';
  final recognizer = TextRecognizer(script: TextRecognitionScript.latin);
  try {
    final result = await recognizer.processImage(InputImage.fromFilePath(path));
    // ML Kit returns blocks; keep its line order, one line per row on the bon.
    return result.blocks.expand((b) => b.lines).map((l) => l.text).join('\n');
  } catch (e) {
    debugPrint('text recognition failed: $e');
    return '';
  } finally {
    await recognizer.close();
  }
}

/// What a platform error means to someone holding the phone.
String friendlyError(Object e) {
  final s = '$e';
  if (s.contains('PlatformException') || s.contains('java.') || s.contains('Exception:')) {
    return 'Dit bestand kon niet worden geopend. Probeer het als PDF of foto, of voer de kosten met de hand in.';
  }
  return s;
}

Future<Capture?> takePhoto() async {
  final shot = await ImagePicker().pickImage(source: ImageSource.camera, imageQuality: 85, maxWidth: 2200);
  if (shot == null) return null;
  return Capture(await shot.readAsBytes(), await _ocr(shot.path), false);
}

const _accepted = ['pdf', 'jpg', 'jpeg', 'png', 'webp', 'heic'];

Future<Capture?> pickFile() async {
  // Any file: some Android file managers refuse a custom extension filter.
  final files = await FilePicker.pickFiles(type: FileType.any);
  if (files.isEmpty) return null;
  final f = files.first;
  return captureFile(await f.readAsBytes(), f.name, f.path);
}

/// A picked or shared file: PDFs go to the server as they are, images are
/// read on the device first.
Future<Capture> captureFile(Uint8List bytes, String name, String? path) async {
  final pdf = isPdf(bytes) || name.toLowerCase().endsWith('.pdf');
  final ext = name.contains('.') ? name.split('.').last.toLowerCase() : '';
  if (!pdf && ext.isNotEmpty && !_accepted.contains(ext)) {
    throw Exception('Kies een PDF of een foto (jpg, png); een .$ext kan de app niet lezen.');
  }
  var text = '';
  if (!pdf && path != null) text = await _ocr(path);
  return Capture(bytes, text, pdf);
}

bool isPdf(Uint8List b) => b.length > 4 && b[0] == 0x25 && b[1] == 0x50 && b[2] == 0x44 && b[3] == 0x46;

Future<Uint8List?> pickBankFile() async {
  final files = await FilePicker.pickFiles(type: FileType.custom, allowedExtensions: ['csv', 'txt', 'xml', 'tab']);
  if (files.isEmpty) return null;
  return files.first.readAsBytes();
}
