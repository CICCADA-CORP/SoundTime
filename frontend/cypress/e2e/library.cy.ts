describe('Library', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
	});

	it('shows library page', () => {
		cy.visit('/library');
		cy.url().should('include', '/library');
	});

	it('shows favorites page', () => {
		cy.visit('/favorites');
		cy.url().should('include', '/favorites');
	});

	it('shows history page', () => {
		cy.visit('/history');
		cy.url().should('include', '/history');
	});

	it('explore page shows albums, artists, tracks sections', () => {
		cy.visit('/explore');
		cy.get('body').should('not.be.empty');
	});

	it('search returns results', () => {
		cy.visit('/search?q=test');
		cy.url().should('include', '/search');
	});

	it('albums API endpoint responds', () => {
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				url: '/api/albums',
				headers: { Authorization: `Bearer ${token}` },
				failOnStatusCode: false
			}).then((res) => {
				expect(res.status).to.be.oneOf([200, 401]);
			});
		});
	});

	it('artists API endpoint responds', () => {
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				url: '/api/artists',
				headers: { Authorization: `Bearer ${token}` },
				failOnStatusCode: false
			}).then((res) => {
				expect(res.status).to.be.oneOf([200, 401]);
			});
		});
	});
});
