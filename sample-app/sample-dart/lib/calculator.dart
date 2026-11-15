/// The calculator's state and rules, kept apart from the screen so they can be unit-tested.
class Calculator {
  static const error = 'Error';
  static const operators = '+−×÷';
  static const _maxDigits = 15;

  /// The number on the display (or `Error`).
  String display = '0';

  /// The pending calculation shown above the display, e.g. `12 +`.
  String expression = '';

  double? _accumulator;
  String? _operator;
  bool _startNew = true;

  void digit(String d) {
    if (d.length != 1 || !'0123456789'.contains(d))
      throw ArgumentError('not a digit: $d');
    if (_startNew || display == '0' || display == error) {
      display = d;
      _startNew = false;
    } else if (display.length < _maxDigits) {
      display += d;
    }
  }

  void dot() {
    if (_startNew || display == error) {
      display = '0.';
      _startNew = false;
    } else if (!display.contains('.')) {
      display += '.';
    }
  }

  /// `+ − × ÷`: a pending one is worked out first (12 + 3 × → 15 ×).
  void operator(String op) {
    if (op.length != 1 || !operators.contains(op))
      throw ArgumentError('not an operator: $op');
    if (display == error) return;
    if (_operator != null && !_startNew) equals();
    if (display == error) return;
    _accumulator = _value();
    _operator = op;
    expression = '$display $op';
    _startNew = true;
  }

  void equals() {
    final a = _accumulator;
    final op = _operator;
    if (a == null || op == null) return;
    final b = _value();
    final double? result = switch (op) {
      '+' => a + b,
      '−' => a - b,
      '×' => a * b,
      _ => b == 0 ? null : a / b,
    };
    display = result == null ? error : format(result);
    expression = '';
    _accumulator = null;
    _operator = null;
    _startNew = true;
  }

  void clear() {
    display = '0';
    expression = '';
    _accumulator = null;
    _operator = null;
    _startNew = true;
  }

  void backspace() {
    if (_startNew || display == error) return;
    display = display.substring(0, display.length - 1);
    if (display.isEmpty || display == '-') display = '0';
  }

  void toggleSign() {
    if (display == '0' || display == error) return;
    display = display.startsWith('-') ? display.substring(1) : '-$display';
  }

  void percent() {
    if (display == error) return;
    display = format(_value() / 100);
    _startNew = true;
  }

  double _value() => double.tryParse(display) ?? 0;

  /// 3.0 → "3", 0.1 + 0.2 → "0.3": at most 12 significant digits, no trailing zeros.
  static String format(double v) {
    if (v.isNaN || v.isInfinite) return error;
    if (v == v.truncateToDouble() && v.abs() < 1e15)
      return v.toInt().toString();
    var s = double.parse(v.toStringAsPrecision(12)).toString();
    if (s.contains('e')) return s;
    if (s.contains('.')) s = s.replaceFirst(RegExp(r'\.?0+$'), '');
    return s;
  }
}
