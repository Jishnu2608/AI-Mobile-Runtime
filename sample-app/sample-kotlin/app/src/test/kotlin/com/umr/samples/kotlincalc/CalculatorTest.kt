package com.umr.samples.kotlincalc

import org.junit.Assert.assertEquals
import org.junit.Test

class CalculatorTest {
  private fun Calculator.keys(s: String): Calculator {
    for (c in s) {
      when (c) {
        in '0'..'9' -> digit(c)
        '.' -> dot()
        '=' -> equals()
        'C' -> clear()
        '<' -> backspace()
        '~' -> toggleSign()
        '%' -> percent()
        else -> operator(c)
      }
    }
    return this
  }

  @Test fun continuesFromAResult() = assertEquals("20", Calculator().keys("12+3=+5=").display)

  @Test fun addsDecimalsExactly() = assertEquals("0.3", Calculator().keys("0.1+0.2=").display)

  @Test fun divisionByZeroIsAnError() = assertEquals(Calculator.ERROR, Calculator().keys("5÷0=").display)

  @Test fun chainedOperatorsWorkLeftToRight() = assertEquals("60", Calculator().keys("12+3×4=").display)

  @Test fun signPercentAndBackspace() {
    assertEquals("-7", Calculator().keys("7~").display)
    assertEquals("0.5", Calculator().keys("50%").display)
    assertEquals("12", Calculator().keys("123<").display)
  }

  @Test fun expressionShowsThePendingStep() = assertEquals("12 +", Calculator().keys("12+").expression)
}
