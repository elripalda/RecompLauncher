package com.recomplauncher.app

import android.content.res.Configuration
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  // The phone layout draws under the clock bar and beside the camera hole itself, and fades what
  // scrolls under them. Big Picture mode is full screen like a console and keeps clear of the hole.
  private var phone = false
  // Dark icons on the bars, for the phone layout's light theme.
  private var lightBars = false
  // Called when the room at the edges changes (ScreenPlugin tells the page).
  var onInsets: (() -> Unit)? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // With three buttons at the bottom, Android would put its own grey strip behind them.
    if (Build.VERSION.SDK_INT >= 29) window.isNavigationBarContrastEnforced = false
    showBars()
    watchEdges()
  }

  fun useEdges(phone: Boolean, light: Boolean) {
    this.phone = phone
    lightBars = light
    showBars()
    ViewCompat.requestApplyInsets(findViewById(android.R.id.content))
  }

  // What the system draws over the window at each edge right now: top, right, bottom, left.
  fun edgeInsets(): IntArray {
    val all = ViewCompat.getRootWindowInsets(window.decorView) ?: return intArrayOf(0, 0, 0, 0)
    val i = all.getInsets(WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout())
    return intArrayOf(i.top, i.right, i.bottom, i.left)
  }

  // Big Picture mode: nothing is drawn under the camera hole, the page sits beside it (sideways)
  // or below it (upright), with black around the hole. The phone layout gets the whole window.
  private fun watchEdges() {
    window.decorView.setBackgroundColor(Color.BLACK)
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val cut = insets.getInsets(WindowInsetsCompat.Type.displayCutout())
      if (phone) v.setPadding(0, 0, 0, 0) else v.setPadding(cut.left, cut.top, cut.right, cut.bottom)
      onInsets?.invoke()
      insets
    }
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus) showBars()
  }

  override fun onConfigurationChanged(newConfig: Configuration) {
    super.onConfigurationChanged(newConfig)
    showBars()
  }

  // The phone layout held upright (or on a tablet, either way) shows the clock and the system's
  // bottom strip like any app. A phone held sideways, and Big Picture mode, are full screen: a
  // swipe from the edge shows the bars for a moment.
  private fun showBars() {
    val bars = WindowCompat.getInsetsController(window, window.decorView)
    bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    val cfg = resources.configuration
    val sideways = cfg.orientation == Configuration.ORIENTATION_LANDSCAPE
    val small = cfg.smallestScreenWidthDp < 600
    if (phone && !(sideways && small)) {
      bars.isAppearanceLightStatusBars = lightBars
      bars.isAppearanceLightNavigationBars = lightBars
      bars.show(WindowInsetsCompat.Type.systemBars())
    } else {
      bars.hide(WindowInsetsCompat.Type.systemBars())
    }
  }
}
