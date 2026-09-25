import { describe, expect, it } from 'vitest';
import { bearerPasswordProblem } from './password';

describe('bearerPasswordProblem', () => {
	it.each(['correct horse battery', 'p@ss~word!123', '{[(<Punctuation>)]}'])(
		'accepts %j',
		(password) => {
			expect(bearerPasswordProblem(password)).toBeNull();
		}
	);

	it.each(['Grüezi-2026', 'euro€sign', 'password123 ', ' password123', 'tab\tinside', 'line\nbreak'])(
		'rejects %j',
		(password) => {
			expect(bearerPasswordProblem(password)).not.toBeNull();
		}
	);
});
