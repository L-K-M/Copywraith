/**
 * Explains why `password` cannot serve as the API bearer token, or returns null.
 *
 * Mirrors `bearer_password_problem` in `copywraith-core`: clients send the
 * password in an `Authorization: Bearer` header, which carries only visible
 * ASCII and spaces, and servers strip leading and trailing whitespace.
 */
export function bearerPasswordProblem(password: string): string | null {
	if (!/^[\x20-\x7e]*$/.test(password)) {
		return 'Passwords can only contain ASCII letters, digits, punctuation and spaces, because clients send them in an HTTP header.';
	}
	if (password.startsWith(' ') || password.endsWith(' ')) {
		return 'Passwords cannot start or end with a space.';
	}
	return null;
}
