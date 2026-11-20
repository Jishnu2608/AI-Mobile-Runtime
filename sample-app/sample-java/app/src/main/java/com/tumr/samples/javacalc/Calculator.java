package com.tumr.samples.javacalc;

import java.math.BigDecimal;
import java.math.MathContext;

/** The calculator's state and rules, kept apart from the screen so they can be unit-tested. */
public final class Calculator {
    public static final String ERROR = "Error";
    public static final String OPERATORS = "+−×÷";
    private static final int MAX_DIGITS = 15;

    private String display = "0";
    private String expression = "";
    private Double accumulator = null;
    private Character operator = null;
    private boolean startNew = true;

    /** The number on the display (or {@code Error}). */
    public String display() {
        return display;
    }

    /** The pending calculation shown above the display, e.g. {@code 12 +}. */
    public String expression() {
        return expression;
    }

    public void digit(char d) {
        if (d < '0' || d > '9') throw new IllegalArgumentException("not a digit: " + d);
        if (startNew || display.equals("0") || display.equals(ERROR)) {
            display = String.valueOf(d);
            startNew = false;
        } else if (display.length() < MAX_DIGITS) {
            display += d;
        }
    }

    public void dot() {
        if (startNew || display.equals(ERROR)) {
            display = "0.";
            startNew = false;
        } else if (!display.contains(".")) {
            display += ".";
        }
    }

    /** {@code + − × ÷}: a pending one is worked out first (12 + 3 × → 15 ×). */
    public void operator(char op) {
        if (OPERATORS.indexOf(op) < 0) throw new IllegalArgumentException("not an operator: " + op);
        if (display.equals(ERROR)) return;
        if (operator != null && !startNew) equals();
        if (display.equals(ERROR)) return;
        accumulator = value();
        operator = op;
        expression = display + " " + op;
        startNew = true;
    }

    public void equals() {
        if (accumulator == null || operator == null) return;
        double a = accumulator;
        double b = value();
        Double result;
        switch (operator) {
            case '+': result = a + b; break;
            case '−': result = a - b; break;
            case '×': result = a * b; break;
            default: result = b == 0.0 ? null : a / b; break;
        }
        display = result == null ? ERROR : format(result);
        expression = "";
        accumulator = null;
        operator = null;
        startNew = true;
    }

    public void clear() {
        display = "0";
        expression = "";
        accumulator = null;
        operator = null;
        startNew = true;
    }

    public void backspace() {
        if (startNew || display.equals(ERROR)) return;
        display = display.substring(0, display.length() - 1);
        if (display.isEmpty() || display.equals("-")) display = "0";
    }

    public void toggleSign() {
        if (display.equals("0") || display.equals(ERROR)) return;
        display = display.startsWith("-") ? display.substring(1) : "-" + display;
    }

    public void percent() {
        if (display.equals(ERROR)) return;
        display = format(value() / 100);
        startNew = true;
    }

    private double value() {
        try {
            return Double.parseDouble(display);
        } catch (NumberFormatException e) {
            return 0.0;
        }
    }

    /** 3.0 → "3", 0.1 + 0.2 → "0.3": at most 12 significant digits, no trailing zeros. */
    public static String format(double v) {
        if (Double.isNaN(v) || Double.isInfinite(v)) return ERROR;
        if (v == Math.floor(v) && Math.abs(v) < 1e15) return Long.toString((long) v);
        return new BigDecimal(v).round(new MathContext(12)).stripTrailingZeros().toPlainString();
    }
}
