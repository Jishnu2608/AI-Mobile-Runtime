import { Calculator, ERROR } from '../src/calculator';

function keys(s: string): Calculator {
  const c = new Calculator();
  for (const k of s) {
    if (/^[0-9]$/.test(k)) c.digit(k);
    else if (k === '.') c.dot();
    else if (k === '=') c.equals();
    else if (k === 'C') c.clear();
    else if (k === '<') c.backspace();
    else if (k === '~') c.toggleSign();
    else if (k === '%') c.percent();
    else c.operate(k);
  }
  return c;
}

test('continues from a result', () => expect(keys('12+3=+5=').display).toBe('20'));
test('adds decimals exactly', () => expect(keys('0.1+0.2=').display).toBe('0.3'));
test('division by zero is an error', () => expect(keys('5÷0=').display).toBe(ERROR));
test('chained operators work left to right', () => expect(keys('12+3×4=').display).toBe('60'));
test('sign, percent and backspace', () => {
  expect(keys('7~').display).toBe('-7');
  expect(keys('50%').display).toBe('0.5');
  expect(keys('123<').display).toBe('12');
});
test('expression shows the pending step', () => expect(keys('12+').expression).toBe('12 +'));
