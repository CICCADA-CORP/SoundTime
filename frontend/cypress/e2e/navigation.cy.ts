describe('Navigation', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
		cy.visit('/');
	});

	it('loads the home page', () => {
		cy.url().should('eq', Cypress.config('baseUrl') + '/');
	});

	it('navigates to explore page', () => {
		cy.get('a[href="/explore"]').first().click();
		cy.url().should('include', '/explore');
	});

	for (const page of ['library', 'playlists', 'favorites', 'history']) {
		it('navigates to ' + page + ' from the sidebar', () => {
			cy.get('#workspace-sidebar a[href="/' + page + '"]').click();
			cy.location('pathname').should('eq', '/' + page);
		});
	}

	it('restores the desktop sidebar with the header toggle', () => {
		cy.get('#sidebar-toggle').click().should('have.attr', 'aria-expanded', 'false');
		cy.get('#workspace-sidebar').should('not.be.visible');
		cy.get('#sidebar-toggle').click().should('have.attr', 'aria-expanded', 'true');
		cy.get('#workspace-sidebar a[href="/albums"]').should('be.visible');
	});

	it('opens the menu on mobile and closes it when a destination is chosen', () => {
		cy.viewport(390, 844);
		cy.get('#sidebar-toggle').click();
		cy.get('[role="dialog"] a[href="/albums"]').click();
		cy.location('pathname').should('eq', '/albums');
		cy.get('#sidebar-toggle').should('have.attr', 'aria-expanded', 'false');
	});

	it('navigates to settings page', () => {
		cy.get('a[href="/settings"]:visible').first().click();
		cy.url().should('include', '/settings');
	});

	it('opens the global search from navigation', () => {
		cy.get('button[aria-keyshortcuts]:visible').click();
		cy.get('[role="dialog"] input[type="search"]').should('be.focused');
	});

	it('keeps the selected preview in the player when navigating', () => {
		const feeds = ['tracks/popular', 'tracks/random', 'albums/recent', 'artists/top', 'editorial-playlists', 'stats/overview'];
		for (const feed of feeds) cy.intercept('GET', '**/api/' + feed + '*', { statusCode: 503 });
		cy.reload();
		cy.get('[data-testid="audio-player"]').should('not.exist');
		cy.get('section[aria-label="After Hours"] button').first().click();
		cy.get('[data-testid="audio-player"]').should('contain', 'Last Light');
		cy.get('a[href="/explore"]:visible').first().click();
		cy.get('[data-testid="audio-player"]').should('contain', 'Last Light');
	});
});
