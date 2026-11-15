import React, { useRef, useState } from 'react';
import { Platform, Pressable, StatusBar, StyleSheet, Text, View } from 'react-native';

import { Calculator, OPERATORS } from './src/calculator';

const ROWS = [
  ['AC', '⌫', '%', '÷'],
  ['7', '8', '9', '×'],
  ['4', '5', '6', '−'],
  ['1', '2', '3', '+'],
  ['±', '0', '.', '='],
];

function colorOf(key: string): string {
  if (OPERATORS.includes(key) || key === '=') return '#FF9F0A';
  if (['AC', '⌫', '%', '±'].includes(key)) return '#3A414C';
  return '#262B33';
}

function CalculatorScreen() {
  const calc = useRef(new Calculator()).current;
  const [, redraw] = useState(0);

  const press = (key: string) => {
    switch (key) {
      case 'AC':
        calc.clear();
        break;
      case '⌫':
        calc.backspace();
        break;
      case '%':
        calc.percent();
        break;
      case '±':
        calc.toggleSign();
        break;
      case '.':
        calc.dot();
        break;
      case '=':
        calc.equals();
        break;
      default:
        if (/^[0-9]$/.test(key)) calc.digit(key);
        else calc.operate(key);
    }
    redraw(n => n + 1);
  };

  return (
    // the app draws behind the system bars (edge to edge): keep clear of them
    <View style={styles.screen}>
      <StatusBar barStyle="light-content" translucent backgroundColor="transparent" />
      <Text style={styles.expression}>{calc.expression}</Text>
      <Text style={styles.display} numberOfLines={1} adjustsFontSizeToFit>
        {calc.display}
      </Text>
      {ROWS.map(row => (
        <View key={row.join()} style={styles.row}>
          {row.map(key => (
            <Pressable
              key={key}
              accessibilityRole="button"
              accessibilityLabel={key}
              onPress={() => press(key)}
              style={({ pressed }) => [styles.key, { backgroundColor: colorOf(key), opacity: pressed ? 0.7 : 1 }]}>
              <Text style={styles.keyText}>{key}</Text>
            </Pressable>
          ))}
        </View>
      ))}
    </View>
  );
}

// No native modules on purpose: their C++ build paths pass Windows' 260-character limit in
// deep folders (react-native-safe-area-context did here).
export default function App() {
  return <CalculatorScreen />;
}

const styles = StyleSheet.create({
  screen: {
    flex: 1,
    backgroundColor: '#101318',
    justifyContent: 'flex-end',
    padding: 16,
    paddingTop: (Platform.OS === 'android' ? StatusBar.currentHeight ?? 24 : 44) + 16,
    paddingBottom: 40,
  },
  expression: { color: '#9AA4B2', fontSize: 24, textAlign: 'right', minHeight: 30 },
  display: { color: '#FFFFFF', fontSize: 64, fontWeight: '300', textAlign: 'right', marginBottom: 16 },
  row: { flexDirection: 'row', gap: 12, marginVertical: 6 },
  key: { flex: 1, aspectRatio: 1, borderRadius: 999, alignItems: 'center', justifyContent: 'center' },
  keyText: { color: '#FFFFFF', fontSize: 26 },
});
