package com.tumr.samples.javacalc;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

public class CalculatorTest {
    private static Calculator keys(String s) {
        Calculator c = new Calculator();
        for (char k : s.toCharArray()) {
            if (k >= '0' && k <= '9') c.digit(k);
            else if (k == '.') c.dot();
            else if (k == '=') c.equals();
            else if (k == 'C') c.clear();
            else if (k == '<') c.backspace();
            else if (k == '~') c.toggleSign();
            else if (k == '%') c.percent();
            else c.operator(k);
        }
        return c;
    }

    @Test public void continuesFromAResult() { assertEquals("20", keys("12+3=+5=").display()); }

    @Test public void addsDecimalsExactly() { assertEquals("0.3", keys("0.1+0.2=").display()); }

    @Test public void divisionByZeroIsAnError() { assertEquals(Calculator.ERROR, keys("5÷0=").display()); }

    @Test public void chainedOperatorsWorkLeftToRight() { assertEquals("60", keys("12+3×4=").display()); }

    @Test public void signPercentAndBackspace() {
        assertEquals("-7", keys("7~").display());
        assertEquals("0.5", keys("50%").display());
        assertEquals("12", keys("123<").display());
    }

    @Test public void expressionShowsThePendingStep() { assertEquals("12 +", keys("12+").expression()); }
}
