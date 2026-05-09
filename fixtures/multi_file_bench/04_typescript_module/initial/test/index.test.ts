import { sumAndGreet } from '../src/index';

test('sum', () => {
  expect(sumAndGreet(2,3)).toBe('sum=5');
});
