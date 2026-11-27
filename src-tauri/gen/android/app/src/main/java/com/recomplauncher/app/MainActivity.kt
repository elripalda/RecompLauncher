package com.recomplauncher.app

import android.graphics.Color
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    hideBars()
    keepClearOfCamera()
  }

  // Nothing is drawn under the camera hole: the page sits beside it (sideways) or below it
  // (upright), with black around the hole.
  private fun keepClearOfCamera() {
    window.decorView.setBackgroundColor(Color.BLACK)
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val cut = insets.getInsets(WindowInsetsCompat.Type.displayCutout())
      v.setPadding(cut.left, cut.top, cut.right, cut.bottom)
      insets
    }
  }

  // Full screen like a console: the clock and the navigation bar stay hidden, and a swipe from the
  // edge shows them for a moment.
  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus) hideBars()
  }

  private fun hideBars() {
    val bars = WindowCompat.getInsetsController(window, window.decorView)
    bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    bars.hide(WindowInsetsCompat.Type.systemBars())
  }
}
