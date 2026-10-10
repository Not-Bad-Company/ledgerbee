import 'package:boekhouding/api.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('plain http only inside the home network or Tailscale', () {
    for (final h in ['localhost', '127.0.0.1', '192.168.2.13', '10.0.0.4', '172.20.1.1', '100.66.214.122', 'pi', 'nas.local', 'pi.tail1234.ts.net']) {
      expect(privateHost(h), isTrue, reason: h);
    }
    for (final h in ['boeken.example.nl', '8.8.8.8', '172.32.0.1', '100.128.0.1', '192.169.1.1', '300.1.1.1']) {
      expect(privateHost(h), isFalse, reason: h);
    }
  });
}
