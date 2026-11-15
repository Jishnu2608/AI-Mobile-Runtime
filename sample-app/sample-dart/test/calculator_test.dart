import 'package:flutter_test/flutter_test.dart';
import 'package:sample_calculator_dart/calculator.dart';

Calculator keys(String s) {
  final c = Calculator();
  for (final k in s.split('')) {
    switch (k) {
      case '.':
        c.dot();
      case '=':
        c.equals();
      case 'C':
        c.clear();
      case '<':
        c.backspace();
      case '~':
        c.toggleSign();
      case '%':
        c.percent();
      default:
        if ('0123456789'.contains(k)) {
          c.digit(k);
        } else {
          c.operator(k);
        }
    }
  }
  return c;
}

void main() {
  test('continues from a result', () => expect(keys('12+3=+5=').display, '20'));
  test('adds decimals exactly', () => expect(keys('0.1+0.2=').display, '0.3'));
  test('division by zero is an error', () => expect(keys('5÷0=').display, Calculator.error));
  test('chained operators work left to right', () => expect(keys('12+3×4=').display, '60'));
  test('sign, percent and backspace', () {
    expect(keys('7~').display, '-7');
    expect(keys('50%').display, '0.5');
    expect(keys('123<').display, '12');
  });
  test('expression shows the pending step', () => expect(keys('12+').expression, '12 +'));
}
