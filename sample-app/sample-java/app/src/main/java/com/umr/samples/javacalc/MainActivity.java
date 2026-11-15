package com.umr.samples.javacalc;

import android.app.Activity;
import android.content.res.ColorStateList;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.util.TypedValue;
import android.view.Gravity;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.TextView;

public class MainActivity extends Activity {
    private static final String[][] ROWS = {
        {"AC", "⌫", "%", "÷"},
        {"7", "8", "9", "×"},
        {"4", "5", "6", "−"},
        {"1", "2", "3", "+"},
        {"±", "0", ".", "="},
    };

    private final Calculator calc = new Calculator();
    private TextView display;
    private TextView expression;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        setContentView(R.layout.activity_main);
        display = findViewById(R.id.display);
        expression = findViewById(R.id.expression);
        LinearLayout keys = findViewById(R.id.keys);
        for (String[] row : ROWS) {
            LinearLayout line = new LinearLayout(this);
            line.setOrientation(LinearLayout.HORIZONTAL);
            for (String key : row) {
                line.addView(key(key));
            }
            keys.addView(line, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        }
        show();
    }

    private Button key(String key) {
        Button b = new Button(this);
        b.setText(key);
        b.setAllCaps(false);
        b.setTextSize(TypedValue.COMPLEX_UNIT_SP, 26);
        b.setTextColor(0xFFFFFFFF);
        b.setGravity(Gravity.CENTER);
        GradientDrawable round = new GradientDrawable();
        round.setShape(GradientDrawable.OVAL);
        int color;
        if (Calculator.OPERATORS.contains(key) || key.equals("=")) color = getColor(R.color.accent);
        else if (key.equals("AC") || key.equals("⌫") || key.equals("%") || key.equals("±")) color = getColor(R.color.function);
        else color = getColor(R.color.digit);
        round.setColor(ColorStateList.valueOf(color));
        b.setBackground(round);
        b.setStateListAnimator(null);
        int size = getResources().getDisplayMetrics().widthPixels / 4 - dp(16);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(0, size, 1f);
        lp.setMargins(dp(6), dp(6), dp(6), dp(6));
        b.setLayoutParams(lp);
        b.setOnClickListener(v -> {
            press(key);
            show();
        });
        return b;
    }

    private void press(String key) {
        switch (key) {
            case "AC": calc.clear(); break;
            case "⌫": calc.backspace(); break;
            case "%": calc.percent(); break;
            case "±": calc.toggleSign(); break;
            case ".": calc.dot(); break;
            case "=": calc.equals(); break;
            default:
                char c = key.charAt(0);
                if (Character.isDigit(c)) calc.digit(c);
                else calc.operator(c);
        }
    }

    private void show() {
        display.setText(calc.display());
        expression.setText(calc.expression());
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }
}
