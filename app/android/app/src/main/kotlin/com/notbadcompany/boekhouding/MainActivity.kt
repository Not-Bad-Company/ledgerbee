package com.notbadcompany.boekhouding

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

/**
 * Receives a PDF or image shared to the app ("Delen met Boekhouding"), copies
 * it into the cache and hands Flutter its path over the "boekhouding/share"
 * channel: `initial` returns the file that started the app, `shared` is
 * called for one that arrives while it runs.
 */
class MainActivity : FlutterActivity() {
    private var channel: MethodChannel? = null
    private var pending: String? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        channel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "boekhouding/share").also {
            it.setMethodCallHandler { call, result ->
                if (call.method == "initial") {
                    result.success(pending)
                    pending = null
                } else {
                    result.notImplemented()
                }
            }
        }
        pending = copyShared(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        copyShared(intent)?.let { channel?.invokeMethod("shared", it) }
    }

    private fun copyShared(intent: Intent?): String? {
        if (intent?.action != Intent.ACTION_SEND) return null
        val uri: Uri = (if (Build.VERSION.SDK_INT >= 33) {
            intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
        } else {
            @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_STREAM)
        }) ?: return null
        return try {
            val name = displayName(uri) ?: "gedeeld"
            val dir = File(cacheDir, "shared").apply { mkdirs() }
            val out = File(dir, name.replace('/', '_'))
            contentResolver.openInputStream(uri)?.use { input -> out.outputStream().use { input.copyTo(it) } } ?: return null
            out.absolutePath
        } catch (e: Exception) {
            null
        }
    }

    private fun displayName(uri: Uri): String? =
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
}
