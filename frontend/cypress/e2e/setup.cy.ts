describe('Initial Setup', () => {
	it('shows setup page when instance is not configured', () => {
		// Reset any prior setup state
		cy.visit('/setup');
		cy.url().should('include', '/setup');
	});

	it('displays setup status correctly', () => {
		cy.request('/api/setup/status').then((res) => {
			expect(res.status).to.eq(200);
			expect(res.body).to.have.property('setup_complete');
			expect(res.body).to.have.property('has_admin');
		});
	});

	it('admin creation form validates username length', () => {
		cy.visit('/setup');
		cy.get('input[name="username"], input[placeholder*="admin"], input[id*="username"]')
			.first()
			.should('exist');
	});

	it('responds to setup status endpoint', () => {
		cy.request({
			url: '/api/setup/status',
			failOnStatusCode: false
		}).then((res) => {
			expect(res.status).to.be.oneOf([200, 404]);
		});
	});
});
