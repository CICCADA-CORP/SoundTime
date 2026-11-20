import { readFileSync } from 'node:fs';
import { defineConfig } from 'cypress';

export default defineConfig({
	e2e: {
		baseUrl: 'http://localhost:5173',
		supportFile: 'cypress/support/e2e.ts',
		specPattern: process.env.SOUNDTIME_E2E_FIXTURE ? 'cypress/private/**/*.cy.ts' : 'cypress/e2e/**/*.cy.ts',
		setupNodeEvents(on, config) {
			const file = process.env.SOUNDTIME_E2E_FIXTURE;
			if (file) {
				const fixture = JSON.parse(readFileSync(file, 'utf8'));
				if (fixture.origin !== new URL(config.baseUrl!).origin) throw new Error('Fixture origin must match the disposable test instance');
				// Share one real sign-in per account across specs. Only the listening spec exercises the login form.
				const sessions = new Map<string, { access_token: string; refresh_token: string }>();
				on('task', {
					privateFixture: () => fixture,
					async privateTokens(username: string) {
						const account = [fixture.member, fixture.other, fixture.admin].find(user => user?.username === username);
						if (!account) throw new Error('Unknown private fixture account');
						const cached = sessions.get(username);
						if (cached) return cached;
						const url = new URL('/api/auth/login', fixture.origin);
						const options = { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(account) };
						let response = await fetch(url, options);
						if (response.status === 429) {
							const seconds = Number(response.headers.get('Retry-After') ?? response.headers.get('X-RateLimit-After') ?? 5);
							await new Promise(resolve => setTimeout(resolve, 1000 * Math.max(1, Math.min(seconds + 1, 30))));
							response = await fetch(url, options);
						}
						if (!response.ok) throw new Error('Private fixture sign-in returned HTTP ' + response.status);
						const { tokens } = await response.json();
						sessions.set(username, tokens);
						return tokens;
					},
				});
			}
		},
		viewportWidth: 1280,
		viewportHeight: 720,
		video: false,
		screenshotOnRunFailure: true,
		defaultCommandTimeout: 10000,
		requestTimeout: 10000,
		responseTimeout: 30000,
		retries: {
			runMode: process.env.SOUNDTIME_E2E_FIXTURE ? 0 : 2,
			openMode: 0
		}
	}
});
