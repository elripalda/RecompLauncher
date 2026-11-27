package com.recomplauncher.app

import android.app.Activity
import android.content.pm.ActivityInfo
import android.os.Build
import android.view.HapticFeedbackConstants
import android.view.InputDevice
import android.view.WindowManager
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class OrientationArgs {
  var landscape: Boolean = false
}

@InvokeArg
class EdgesArgs {
  var phone: Boolean = false
  var light: Boolean = false
}

@InvokeArg
class HapticArgs {
  var kind: String = "tap"
}

// The launcher's own window: which way it turns, what it draws under, a tick under the finger,
// and going to the background. Called from src-tauri/src/android.rs.
@TauriPlugin
class ScreenPlugin(private val activity: Activity) : Plugin(activity) {
  private val main: MainActivity? get() = activity as? MainActivity
  private var page: WebView? = null

  // When the room at the edges changes (the phone turned, the bars came or went), the page is
  // told to ask for the new sizes.
  override fun load(webView: WebView) {
    page = webView
    main?.onInsets = {
      webView.post { webView.evaluateJavascript("window.phoneInsets&&window.phoneInsets()", null) }
    }
  }

  // Big Picture mode is drawn sideways and turns only between the two sideways ways. The phone
  // layout follows the phone, as the player's rotation lock says.
  @Command
  fun orientation(invoke: Invoke) {
    val args = invoke.parseArgs(OrientationArgs::class.java)
    activity.runOnUiThread {
      // The page puts a dark cover up before it asks, so the turn is a plain fade between two dark
      // screens instead of the spinning snapshot of a half-drawn page.
      val w = activity.window
      w.attributes = w.attributes.also { it.rotationAnimation = WindowManager.LayoutParams.ROTATION_ANIMATION_CROSSFADE }
      activity.requestedOrientation =
        if (args.landscape) ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE else ActivityInfo.SCREEN_ORIENTATION_FULL_USER
      // Turning the phone by hand afterwards rotates as usual.
      w.decorView.postDelayed({ w.attributes = w.attributes.also { it.rotationAnimation = WindowManager.LayoutParams.ROTATION_ANIMATION_ROTATE } }, 2000)
      invoke.resolve()
    }
  }

  // The phone layout draws under the clock bar and beside the camera hole; Big Picture mode hides
  // the bars and keeps clear of the hole.
  @Command
  fun edges(invoke: Invoke) {
    val args = invoke.parseArgs(EdgesArgs::class.java)
    activity.runOnUiThread {
      main?.useEdges(args.phone, args.light)
      invoke.resolve()
    }
  }

  // The room the system takes at each edge, in the page's own pixels.
  @Command
  fun insets(invoke: Invoke) {
    activity.runOnUiThread {
      val px = main?.edgeInsets() ?: intArrayOf(0, 0, 0, 0)
      val density = activity.resources.displayMetrics.density
      val ret = JSObject()
      ret.put("top", (px[0] / density).toDouble())
      ret.put("right", (px[1] / density).toDouble())
      ret.put("bottom", (px[2] / density).toDouble())
      ret.put("left", (px[3] / density).toDouble())
      invoke.resolve(ret)
    }
  }

  // What kind of screen this is: the smaller side in dp (600 and up is a tablet), and whether a
  // controller is built in (a handheld like a Retroid or an Odin), which a plugged-in or Bluetooth
  // one is not.
  @Command
  fun device(invoke: Invoke) {
    val ret = JSObject()
    ret.put("sw", activity.resources.configuration.smallestScreenWidthDp)
    var pad = false
    for (id in InputDevice.getDeviceIds()) {
      val d = InputDevice.getDevice(id) ?: continue
      val gamepad = d.sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD
      val stick = d.sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
      val external = if (Build.VERSION.SDK_INT >= 29) d.isExternal else true
      if (gamepad && stick && !external && !d.isVirtual) pad = true
    }
    ret.put("pad", pad)
    invoke.resolve(ret)
  }

  // A light tick under the finger. Android itself leaves it out when the player turned touch
  // feedback off.
  @Command
  fun haptic(invoke: Invoke) {
    val args = invoke.parseArgs(HapticArgs::class.java)
    val ok = if (Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.VIRTUAL_KEY
    val kind = when (args.kind) {
      "tick" -> HapticFeedbackConstants.CLOCK_TICK
      "ok" -> ok
      else -> HapticFeedbackConstants.VIRTUAL_KEY
    }
    activity.runOnUiThread {
      (page ?: activity.window.decorView).performHapticFeedback(kind)
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
