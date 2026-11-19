/// <reference types="cypress" />

// Custom commands for SoundTime E2E tests

declare global {
	namespace Cypress {
		interface Chainable {
			login(username: string, password: string): Chainable<void>;
			loginAsAdmin(): Chainable<void>;
			setupInstance(adminUser?: { username: string; email: string; password: string }): Chainable<void>;
			apiRequest(method: string, path: string, body?: object): Chainable<Cypress.Response<unknown>>;
		}
	}
}

// Login via API and store tokens
Cypress.Commands.add('login', (username: string, password: string) => {
	cy.request({
		method: 'POST',
		url: '/api/auth/login',
		body: { username, password }
	}).then((response) => {
		const { access_token, refresh_token } = response.body.tokens;
		window.localStorage.setItem('soundtime_access_token', access_token);
		window.localStorage.setItem('soundtime_refresh_token', refresh_token);
	});
});

// Login as the default admin user
Cypress.Commands.add('loginAsAdmin', () => {
	cy.login('admin', 'Admin123!');
});

// Resume partial setup using the credentials that created the disposable instance.
Cypress.Commands.add('setupInstance', (adminUser) => {
	const user = adminUser ?? { username: 'admin', email: 'admin@soundtime.test', password: 'Admin123!' };
	cy.request('/api/setup/status').then(({ body: status }) => {
		if (status.setup_complete) return;
		const session = status.has_admin
			? cy.request('POST', '/api/auth/login', { username: user.username, password: user.password })
			: cy.request('POST', '/api/setup/admin', user);
		session.then(({ body }) => {
			const headers = { Authorization: 'Bearer ' + body.tokens.access_token };
			cy.request({ method: 'POST', url: '/api/setup/instance', headers,
				body: { instance_name: 'Test Instance', instance_description: 'E2E test instance' } });
			cy.request({ method: 'POST', url: '/api/setup/complete', headers,
				body: { p2p_enabled: false, open_registrations: true, max_upload_size_mb: 50 } });
		});
	});
});

// Authenticated API request
Cypress.Commands.add('apiRequest', (method: string, path: string, body?: object) => {
	const token = window.localStorage.getItem('soundtime_access_token');
	cy.request({
		method,
		url: `/api${path}`,
		headers: token ? { Authorization: `Bearer ${token}` } : {},
		body,
		failOnStatusCode: false
	});
});

export {};
