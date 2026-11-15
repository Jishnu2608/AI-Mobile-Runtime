/** The calculator's state and rules, kept apart from the screen so they can be unit-tested. */
export const ERROR = 'Error';
export const OPERATORS = '+−×÷';
const MAX_DIGITS = 15;

export class Calculator {
  /** The number on the display (or `Error`). */
  display = '0';
  /** The pending calculation shown above the display, e.g. `12 +`. */
  expression = '';
  private accumulator: number | null = null;
  private operator: string | null = null;
  private startNew = true;

  digit(d: string): void {
    if (!/^[0-9]$/.test(d)) throw new Error(`not a digit: ${d}`);
    if (this.startNew || this.display === '0' || this.display === ERROR) {
      this.display = d;
      this.startNew = false;
    } else if (this.display.length < MAX_DIGITS) {
      this.display += d;
    }
  }

  dot(): void {
    if (this.startNew || this.display === ERROR) {
      this.display = '0.';
      this.startNew = false;
    } else if (!this.display.includes('.')) {
      this.display += '.';
    }
  }

  /** `+ − × ÷`: a pending one is worked out first (12 + 3 × → 15 ×). */
  operate(op: string): void {
    if (op.length !== 1 || !OPERATORS.includes(op)) throw new Error(`not an operator: ${op}`);
    if (this.display === ERROR) return;
    if (this.operator !== null && !this.startNew) this.equals();
    if (this.display === ERROR) return;
    this.accumulator = this.value();
    this.operator = op;
    this.expression = `${this.display} ${op}`;
    this.startNew = true;
  }

  equals(): void {
    const a = this.accumulator;
    const op = this.operator;
    if (a === null || op === null) return;
    const b = this.value();
    let result: number | null;
    switch (op) {
      case '+':
        result = a + b;
        break;
      case '−':
        result = a - b;
        break;
      case '×':
        result = a * b;
        break;
      default:
        result = b === 0 ? null : a / b;
    }
    this.display = result === null ? ERROR : format(result);
    this.expression = '';
    this.accumulator = null;
    this.operator = null;
    this.startNew = true;
  }

  clear(): void {
    this.display = '0';
    this.expression = '';
    this.accumulator = null;
    this.operator = null;
    this.startNew = true;
  }

  backspace(): void {
    if (this.startNew || this.display === ERROR) return;
    this.display = this.display.slice(0, -1);
    if (this.display === '' || this.display === '-') this.display = '0';
  }

  toggleSign(): void {
    if (this.display === '0' || this.display === ERROR) return;
    this.display = this.display.startsWith('-') ? this.display.slice(1) : `-${this.display}`;
  }

  percent(): void {
    if (this.display === ERROR) return;
    this.display = format(this.value() / 100);
    this.startNew = true;
  }

  private value(): number {
    const v = Number(this.display);
    return Number.isNaN(v) ? 0 : v;
  }
}

/** 3 → "3", 0.1 + 0.2 → "0.3": at most 12 significant digits, no trailing zeros. */
export function format(v: number): string {
  if (!Number.isFinite(v)) return ERROR;
  if (Number.isInteger(v) && Math.abs(v) < 1e15) return String(v);
  return String(Number(v.toPrecision(12)));
}
