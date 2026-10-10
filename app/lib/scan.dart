// Getting a receipt or invoice into the app: the camera with on-device text
// recognition (Android, ML Kit, offline), or a file (PDF or image) on either
// platform. Reading the text into amounts happens on the server, in one
// parser for every source.

import 'dart:io';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:google_mlkit_text_recognition/google_mlkit_text_recognition.dart';
import 'package:image_picker/image_picker.dart';

class Capture {
  final Uint8List bytes;
  final String ocrText;
  final bool isPdf;
  Capture(this.bytes, this.ocrText, this.isPdf);
}

bool get canUseCamera => Platform.isAndroid;

Future<String> _ocr(String path) async {
  if (!Platform.isAndroid) return '';
  final recognizer = TextRecognizer(script: TextRecognitionScript.latin);
  try {
    final result = await recognizer.processImage(InputImage.fromFilePath(path));
    // ML Kit returns blocks; keep its line order, one line per row on the bon.
    return result.blocks.expand((b) => b.lines).map((l) => l.text).join('\n');
  } finally {
    await recognizer.close();
  }
}

Future<Capture?> takePhoto() async {
  final shot = await ImagePicker().pickImage(source: ImageSource.camera, imageQuality: 85, maxWidth: 2200);
  if (shot == null) return null;
  return Capture(await shot.readAsBytes(), await _ocr(shot.path), false);
}

Future<Capture?> pickFile() async {
  final files = await FilePicker.pickFiles(type: FileType.custom, allowedExtensions: ['pdf', 'jpg', 'jpeg', 'png', 'webp']);
  if (files.isEmpty) return null;
  final f = files.first;
  final bytes = await f.readAsBytes();
  final pdf = (f.extension ?? '').toLowerCase() == 'pdf';
  var text = '';
  if (!pdf && f.path != null) text = await _ocr(f.path!);
  return Capture(bytes, text, pdf);
}

Future<Uint8List?> pickBankFile() async {
  final files = await FilePicker.pickFiles(type: FileType.custom, allowedExtensions: ['csv', 'txt', 'xml', 'tab']);
  if (files.isEmpty) return null;
  return files.first.readAsBytes();
}
