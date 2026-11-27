package com.recomplauncher.app

import android.app.Activity
import android.content.pm.ActivityInfo
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class OrientationArgs {
  var landscape: Boolean = false
}

// The launcher's own window: which way it turns, and going to the background. Called from
// src-tauri/src/android.rs.
@TauriPlugin
class ScreenPlugin(private val activity: Activity) : Plugin(activity) {
  // Big Picture mode is drawn sideways and turns only between the two sideways ways. The phone
  // layout follows the phone, as the player's rotation lock says.
  @Command
  fun orientation(invoke: Invoke) {
    val args = invoke.parseArgs(OrientationArgs::class.java)
    activity.runOnUiThread {
      activity.requestedOrientation =
        if (args.landscape) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE else ActivityInfo.SCREEN_ORIENTATION_FULL_USER
      invoke.resolve()
    }
  }

  // Back on the phone layout's Home: the launcher goes to the background, as other apps do.
  @Command
  fun toBack(invoke: Invoke) {
    activity.runOnUiThread {
      activity.moveTaskToBack(true)
      invoke.resolve()
    }
  }
}
