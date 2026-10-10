import 'package:flutter/material.dart';

import '../api.dart';
import '../widgets.dart';

class LoginScreen extends StatefulWidget {
  const LoginScreen({super.key});

  @override
  State<LoginScreen> createState() => _LoginScreenState();
}

class _LoginScreenState extends State<LoginScreen> {
  final _server = TextEditingController(text: Api.instance.baseUrl.isEmpty ? 'http://100.66.214.122:8090' : Api.instance.baseUrl);
  final _email = TextEditingController(text: 'daanhessen@notbadcompany.com');
  final _password = TextEditingController();
  final _code = TextEditingController();
  bool _busy = false;

  Future<void> _submit() async {
    setState(() => _busy = true);
    await guard(context, () => Api.instance.login(_server.text, _email.text, _password.text, _code.text));
    if (mounted) setState(() => _busy = false);
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(24),
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 380),
              child: AutofillGroup(
                child: Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
                  const AppMark(size: 56),
                  const SizedBox(height: 20),
                  Text('Hoi Daan', style: t.headlineMedium),
                  Text('Log in op de administratie van Not Bad Company.', style: t.bodyLarge?.copyWith(color: Theme.of(context).colorScheme.onSurfaceVariant)),
                  const SizedBox(height: 32),
                  TextField(controller: _server, decoration: const InputDecoration(labelText: 'Server', helperText: 'De Pi, via Tailscale'), keyboardType: TextInputType.url),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _email,
                    decoration: const InputDecoration(labelText: 'E-mail'),
                    keyboardType: TextInputType.emailAddress,
                    autofillHints: const [AutofillHints.email],
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _password,
                    decoration: const InputDecoration(labelText: 'Wachtwoord'),
                    obscureText: true,
                    autofillHints: const [AutofillHints.password],
                  ),
                  const SizedBox(height: 12),
                  TextField(
                    controller: _code,
                    decoration: const InputDecoration(labelText: 'Code uit je authenticator-app', helperText: 'Of een herstelcode'),
                    keyboardType: TextInputType.visiblePassword,
                    autofillHints: const [AutofillHints.oneTimeCode],
                    onSubmitted: (_) => _submit(),
                  ),
                  const SizedBox(height: 24),
                  FilledButton(
                    onPressed: _busy ? null : _submit,
                    child: Padding(
                      padding: const EdgeInsets.symmetric(vertical: 12),
                      child: _busy ? const SizedBox(height: 18, width: 18, child: CircularProgressIndicator(strokeWidth: 2)) : const Text('Inloggen'),
                    ),
                  ),
                ]),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
