import { add } from './utils';

export function sumAndGreet(a: number, b: number): string {
  const s = add(a, b);
  return `sum=${s}`;
}
