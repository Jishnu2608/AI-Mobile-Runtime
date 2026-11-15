import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'calculator.dart';

void main() {
  runApp(const CalculatorApp());
}

class CalculatorApp extends StatelessWidget {
  const CalculatorApp({super.key});

  @override
  Widget build(BuildContext context) {
    return const MaterialApp(
      title: 'Calculator (Dart)',
      debugShowCheckedModeBanner: false,
      home: CalculatorScreen(),
    );
  }
}

const _background = Color(0xFF101318);
const _digit = Color(0xFF262B33);
const _function = Color(0xFF3A414C);
const _accent = Color(0xFFFF9F0A);

class CalculatorScreen extends StatefulWidget {
  const CalculatorScreen({super.key});

  @override
  State<CalculatorScreen> createState() => _CalculatorScreenState();
}

class _CalculatorScreenState extends State<CalculatorScreen> {
  final _calc = Calculator();

  static const _rows = [
    ['AC', '⌫', '%', '÷'],
    ['7', '8', '9', '×'],
    ['4', '5', '6', '−'],
    ['1', '2', '3', '+'],
    ['±', '0', '.', '='],
  ];

  void _press(String key) {
    setState(() {
      switch (key) {
        case 'AC':
          _calc.clear();
        case '⌫':
          _calc.backspace();
        case '%':
          _calc.percent();
        case '±':
          _calc.toggleSign();
        case '.':
          _calc.dot();
        case '=':
          _calc.equals();
        default:
          if ('0123456789'.contains(key)) {
            _calc.digit(key);
          } else {
            _calc.operator(key);
          }
      }
    });
  }

  Color _colorOf(String key) {
    if (Calculator.operators.contains(key) || key == '=') return _accent;
    if (const ['AC', '⌫', '%', '±'].contains(key)) return _function;
    return _digit;
  }

  @override
  Widget build(BuildContext context) {
    // light status bar icons on the dark background
    return AnnotatedRegion<SystemUiOverlayStyle>(
      value: SystemUiOverlayStyle.light,
      child: Scaffold(
        backgroundColor: _background,
        body: SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisAlignment: MainAxisAlignment.end,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(
                  _calc.expression,
                  textAlign: TextAlign.end,
                  style: const TextStyle(
                    color: Color(0xFF9AA4B2),
                    fontSize: 24,
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.only(bottom: 16),
                  child: Text(
                    _calc.display,
                    textAlign: TextAlign.end,
                    maxLines: 1,
                    style: const TextStyle(
                      color: Colors.white,
                      fontSize: 64,
                      fontWeight: FontWeight.w300,
                    ),
                  ),
                ),
                for (final row in _rows)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 6),
                    child: Row(
                      children: [
                        for (final key in row)
                          Expanded(
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                              ),
                              child: AspectRatio(
                                aspectRatio: 1,
                                child: ElevatedButton(
                                  onPressed: () => _press(key),
                                  style: ElevatedButton.styleFrom(
                                    backgroundColor: _colorOf(key),
                                    foregroundColor: Colors.white,
                                    shape: const CircleBorder(),
                                    padding: EdgeInsets.zero,
                                  ),
                                  child: Text(
                                    key,
                                    style: const TextStyle(fontSize: 26),
                                  ),
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
