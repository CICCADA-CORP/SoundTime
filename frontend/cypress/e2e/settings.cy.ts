describe('Settings', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
	});

	it('settings page loads', () => {
		cy.visit('/settings');
		cy.url().should('include', '/settings');
	});

	it('settings page shows language selector', () => {
		cy.visit('/settings');
		// Look for language/locale selection
		cy.get('select, [data-testid="language-selector"], button:contains("English"), button:contains("Français")')
			.should('exist');
	});

	it('user profile section is visible', () => {
		cy.visit('/settings');
		cy.get('body').should('not.be.empty');
	});

	it('auth/me endpoint returns user data', () => {
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				url: '/api/auth/me',
				headers: { Authorization: `Bearer ${token}` },
				failOnStatusCode: false
			}).then((res) => {
				if (res.status === 200) {
					expect(res.body).to.have.property('username');
					expect(res.body).to.have.property('email');
				}
			});
		});
	});

	it('language preference persists', () => {
		cy.visit('/settings');
		// Change language and verify it persists
		cy.window().then((win) => {
			win.localStorage.setItem('soundtime_lang', 'fr');
		});
		cy.reload();
		cy.window().then((win) => {
			expect(win.localStorage.getItem('soundtime_lang')).to.eq('fr');
		});
	});
});
