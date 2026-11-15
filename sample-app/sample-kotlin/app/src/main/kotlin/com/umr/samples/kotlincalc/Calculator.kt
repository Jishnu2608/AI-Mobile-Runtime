package com.umr.samples.kotlincalc

import java.math.BigDecimal
import java.math.MathContext
import kotlin.math.abs
import kotlin.math.floor

/** The calculator's state and rules, kept apart from the screen so they can be unit-tested. */
class Calculator {
  /** The number on the display (or `Error`). */
  var display: String = "0"
    private set

  /** The pending calculation shown above the display, e.g. `12 +`. */
  var expression: String = ""
    private set

  private var accumulator: Double? = null
  private var operator: Char? = null
  private var startNew = true

  fun digit(d: Char) {
    require(d in '0'..'9')
    if (startNew || display == "0" || display == ERROR) {
      display = d.toString()
      startNew = false
    } else if (display.length < MAX_DIGITS) {
      display += d
    }
  }

  fun dot() {
    if (startNew || display == ERROR) {
      display = "0."
      startNew = false
    } else if ('.' !in display) {
      display += "."
    }
  }

  /** `+`, `−`, `×` or `÷`: a pending one is worked out first (12 + 3 × → 15 ×). */
  fun operator(op: Char) {
    require(op in OPERATORS)
    if (display == ERROR) return
    if (operator != null && !startNew) equals()
    if (display == ERROR) return
    accumulator = value()
    operator = op
    expression = "$display $op"
    startNew = true
  }

  fun equals() {
    val a = accumulator ?: return
    val op = operator ?: return
    val b = value()
    val result = when (op) {
      '+' -> a + b
      '−' -> a - b
      '×' -> a * b
      else -> if (b == 0.0) null else a / b
    }
    display = result?.let(::format) ?: ERROR
    expression = ""
    accumulator = null
    operator = null
    startNew = true
  }

  fun clear() {
    display = "0"
    expression = ""
    accumulator = null
    operator = null
    startNew = true
  }

  fun backspace() {
    if (startNew || display == ERROR) return
    display = display.dropLast(1).ifEmpty { "0" }.let { if (it == "-") "0" else it }
  }

  fun toggleSign() {
    if (display == "0" || display == ERROR) return
    display = if (display.startsWith("-")) display.drop(1) else "-$display"
  }

  fun percent() {
    if (display == ERROR) return
    display = format(value() / 100)
    startNew = true
  }

  private fun value(): Double = display.toDoubleOrNull() ?: 0.0

  companion object {
    const val ERROR = "Error"
    const val OPERATORS = "+−×÷"
    private const val MAX_DIGITS = 15

    /** 3.0 → "3", 0.1 + 0.2 → "0.3": at most 12 significant digits, no trailing zeros. */
    fun format(v: Double): String {
      if (v.isNaN() || v.isInfinite()) return ERROR
      if (v == floor(v) && abs(v) < 1e15) return v.toLong().toString()
      return BigDecimal(v).round(MathContext(12)).stripTrailingZeros().toPlainString()
    }
  }
}
