package com.umr.samples.kotlincalc

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

class MainActivity : ComponentActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    // light status bar icons on the dark background
    enableEdgeToEdge(statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
    setContent { CalculatorScreen() }
  }
}

private val Background = Color(0xFF101318)
private val Digit = Color(0xFF262B33)
private val Function = Color(0xFF3A414C)
private val Accent = Color(0xFF34C759)

@Composable
fun CalculatorScreen() {
  val calc = remember { Calculator() }
  // bumped after every key so Compose redraws the display
  var version by remember { mutableIntStateOf(0) }
  fun press(action: Calculator.() -> Unit) {
    calc.action()
    version++
  }
  val rows = listOf(
    listOf("AC", "⌫", "%", "÷"),
    listOf("7", "8", "9", "×"),
    listOf("4", "5", "6", "−"),
    listOf("1", "2", "3", "+"),
    listOf("±", "0", ".", "="),
  )
  Column(
    Modifier.fillMaxSize().background(Background).safeDrawingPadding().padding(16.dp),
    verticalArrangement = Arrangement.Bottom,
  ) {
    Text(
      text = if (version >= 0) calc.expression else "",
      color = Color(0xFF9AA4B2),
      fontSize = 24.sp,
      textAlign = TextAlign.End,
      modifier = Modifier.fillMaxWidth(),
    )
    Text(
      text = calc.display,
      color = Color.White,
      fontSize = 64.sp,
      fontWeight = FontWeight.Light,
      textAlign = TextAlign.End,
      maxLines = 1,
      modifier = Modifier.fillMaxWidth().padding(bottom = 16.dp),
    )
    for (row in rows) {
      Row(horizontalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.padding(vertical = 6.dp)) {
        for (key in row) {
          val color = when {
            key in Calculator.OPERATORS || key == "=" -> Accent
            key in listOf("AC", "⌫", "%", "±") -> Function
            else -> Digit
          }
          Button(
            onClick = {
              press {
                when (key) {
                  "AC" -> clear()
                  "⌫" -> backspace()
                  "%" -> percent()
                  "±" -> toggleSign()
                  "." -> dot()
                  "=" -> equals()
                  else -> if (key[0].isDigit()) digit(key[0]) else operator(key[0])
                }
              }
            },
            shape = CircleShape,
            colors = ButtonDefaults.buttonColors(containerColor = color, contentColor = Color.White),
            modifier = Modifier.weight(1f).aspectRatio(1f),
          ) {
            Text(key, fontSize = 26.sp)
          }
        }
      }
    }
    Spacer(Modifier.padding(4.dp).align(Alignment.CenterHorizontally))
  }
}
