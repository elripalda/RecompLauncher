package com.recomplauncher.app

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInfo
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.Manifest
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.MessageDigest

@InvokeArg
class ApkFileArgs {
  lateinit var path: String
}

@InvokeArg
class PackageArgs {
  lateinit var pkg: String
}

@InvokeArg
class PackagesArgs {
  var pkgs: Array<String> = arrayOf()
}

// The games' own apps: what's in a downloaded APK, installing and updating it through Android's
// installer, opening it, uninstalling it, and which are installed. Called from src-tauri/src/android.rs.
@TauriPlugin
class ApkPlugin(private val activity: Activity) : Plugin(activity) {
  private val pm: PackageManager get() = activity.packageManager

  private fun sha256(bytes: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }

  @Suppress("DEPRECATION")
  private fun signers(info: PackageInfo?): List<String> {
    if (info == null) return emptyList()
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
      val s = info.signingInfo ?: return emptyList()
      val list = if (s.hasMultipleSigners()) s.apkContentsSigners else s.signingCertificateHistory
      return list?.map { sha256(it.toByteArray()) } ?: emptyList()
    }
    return info.signatures?.map { sha256(it.toByteArray()) } ?: emptyList()
  }

  private fun flags(): Int =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) PackageManager.GET_SIGNING_CERTIFICATES
    else @Suppress("DEPRECATION") PackageManager.GET_SIGNATURES

  private fun versionCode(info: PackageInfo): Long =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) info.longVersionCode
    else @Suppress("DEPRECATION") info.versionCode.toLong()

  private fun installedInfo(pkg: String): PackageInfo? =
    try { pm.getPackageInfo(pkg, flags()) } catch (e: PackageManager.NameNotFoundException) { null }

  // What a downloaded APK is: its package, version and signing certificates.
  @Command
  fun read(invoke: Invoke) {
    val args = invoke.parseArgs(ApkFileArgs::class.java)
    val info = pm.getPackageArchiveInfo(args.path, flags())
    if (info == null) {
      invoke.reject("That file isn't an Android app.")
      return
    }
    val out = JSObject()
    out.put("pkg", info.packageName)
    out.put("versionCode", versionCode(info))
    out.put("versionName", info.versionName ?: "")
    out.put("sigs", signers(info).joinToString(","))
    invoke.resolve(out)
  }

  // Which of these are installed: package -> {versionCode, versionName, sigs}.
  @Command
  fun installed(invoke: Invoke) {
    val args = invoke.parseArgs(PackagesArgs::class.java)
    val out = JSObject()
    for (pkg in args.pkgs) {
      val info = installedInfo(pkg) ?: continue
      val one = JSObject()
      one.put("versionCode", versionCode(info))
      one.put("versionName", info.versionName ?: "")
      one.put("sigs", signers(info).joinToString(","))
      out.put(pkg, one)
    }
    invoke.resolve(out)
  }

  // Whether the launcher may install apps yet, without opening anything (first-time setup and the
  // Install button ask before a download starts).
  @Command
  fun installAllowed(invoke: Invoke) {
    val out = JSObject()
    out.put("ok", Build.VERSION.SDK_INT < Build.VERSION_CODES.O || pm.canRequestPackageInstalls())
    invoke.resolve(out)
  }

  // Android asks once whether the launcher may install apps; until then its settings page opens.
  @Command
  fun canInstall(invoke: Invoke) {
    val ok = Build.VERSION.SDK_INT < Build.VERSION_CODES.O || pm.canRequestPackageInstalls()
    if (!ok) {
      val i = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + activity.packageName))
      activity.startActivity(i)
    }
    val out = JSObject()
    out.put("ok", ok)
    invoke.resolve(out)
  }

  // Cloud saves: a game that keeps its saves in the phone's shared storage can be backed up once
  // the player allows All files access (Android 11 and newer; storage access before that). The
  // games' private folders and Android/data stay out of reach either way. Also tells the phone's
  // name, shown beside each save copy.
  private fun filesGranted(): Boolean =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
    else activity.checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

  @Command
  fun filesAccess(invoke: Invoke) {
    val out = JSObject()
    out.put("granted", filesGranted())
    @Suppress("DEPRECATION")
    out.put("root", Environment.getExternalStorageDirectory().absolutePath)
    out.put("device", listOf(Build.MANUFACTURER, Build.MODEL).filter { !it.isNullOrBlank() }.joinToString(" "))
    invoke.resolve(out)
  }

  // Opens Android's All files access page for the launcher (or asks for storage access on Android
  // 10 and older). The window checks again when the player comes back.
  @Command
  fun askFilesAccess(invoke: Invoke) {
    if (!filesGranted()) {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        val i = Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:" + activity.packageName))
        try { activity.startActivity(i) } catch (e: Exception) { activity.startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION)) }
      } else {
        activity.requestPermissions(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE), 4711)
      }
    }
    val out = JSObject()
    out.put("granted", filesGranted())
    invoke.resolve(out)
  }

  // Installs (or updates) the APK through Android's own installer, which asks the player to
  // confirm. Resolves when Android says how it went.
  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(ApkFileArgs::class.java)
    // Copying a big game into the installer takes a while: never on the screen's thread.
    Thread { installFile(invoke, File(args.path)) }.start()
  }

  private fun installFile(invoke: Invoke, file: File) {
    try {
      val installer = pm.packageInstaller
      val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
      params.setSize(file.length())
      val id = installer.createSession(params)
      val session = installer.openSession(id)
      file.inputStream().use { input ->
        session.openWrite("game.apk", 0, file.length()).use { out ->
          input.copyTo(out, 1 shl 16)
          session.fsync(out)
        }
      }
      val action = activity.packageName + ".INSTALL_RESULT." + id
      val receiver = object : BroadcastReceiver() {
        override fun onReceive(ctx: Context, intent: Intent) {
          when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
              @Suppress("DEPRECATION")
              val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
              if (confirm != null) {
                confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                activity.startActivity(confirm)
              }
            }
            PackageInstaller.STATUS_SUCCESS -> {
              activity.unregisterReceiver(this)
              invoke.resolve()
            }
            else -> {
              activity.unregisterReceiver(this)
              val msg = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: ""
              invoke.reject(when (status) {
                PackageInstaller.STATUS_FAILURE_ABORTED -> "CANCELLED"
                PackageInstaller.STATUS_FAILURE_CONFLICT -> "A different copy of this game is installed (signed by someone else). Uninstall it first, then install this one."
                PackageInstaller.STATUS_FAILURE_INCOMPATIBLE -> "This game doesn't run on this device."
                PackageInstaller.STATUS_FAILURE_STORAGE -> "There isn't enough space on this device."
                PackageInstaller.STATUS_FAILURE_BLOCKED -> "Android blocked the install."
                else -> "Android couldn't install it. $msg".trim()
              })
            }
          }
        }
      }
      val filter = IntentFilter(action)
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        activity.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
      } else {
        @Suppress("UnspecifiedRegisterReceiverFlag")
        activity.registerReceiver(receiver, filter)
      }
      val pending = PendingIntent.getBroadcast(
        activity, id, Intent(action).setPackage(activity.packageName),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE
      )
      session.commit(pending.intentSender)
      session.close()
    } catch (e: Exception) {
      invoke.reject("Android couldn't install it: ${e.message}")
    }
  }

  // Opens an installed game.
  @Command
  fun open(invoke: Invoke) {
    val args = invoke.parseArgs(PackageArgs::class.java)
    val i = pm.getLaunchIntentForPackage(args.pkg) ?: pm.getLeanbackLaunchIntentForPackage(args.pkg)
    if (i == null) {
      invoke.reject("It isn't installed on this device.")
      return
    }
    i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.startActivity(i)
    invoke.resolve()
  }

  // Android's own Uninstall question for the game. The launcher checks afterwards whether it went.
  @Command
  fun uninstall(invoke: Invoke) {
    val args = invoke.parseArgs(PackageArgs::class.java)
    @Suppress("DEPRECATION")
    val i = Intent(Intent.ACTION_UNINSTALL_PACKAGE, Uri.parse("package:" + args.pkg))
    i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.startActivity(i)
    invoke.resolve()
  }
}
