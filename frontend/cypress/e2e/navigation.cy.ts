describe('Navigation', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
		cy.visit('/', { onBeforeLoad(win) { win.localStorage.setItem('soundtime_lang', 'en'); } });
	});

	it('loads the home page', () => {
		cy.url().should('eq', Cypress.config('baseUrl') + '/');
	});

	it('navigates to explore page', () => {
		cy.get('#workspace-directory nav a[href="/explore"]').click();
		cy.url().should('include', '/explore');
	});

	for (const page of ['library', 'playlists', 'favorites', 'history']) {
		it('navigates to ' + page + ' from the sidebar', () => {
			cy.get('#workspace-directory a[href="/' + page + '"]').click();
			cy.location('pathname').should('eq', '/' + page);
		});
	}

	it('keeps the library navigation in view on desktop', () => {
		cy.get('#workspace-directory a[href="/albums"]').should('be.visible');
		cy.get('#directory-toggle').should('not.exist');
	});

	it('opens the menu on mobile and closes it when a destination is chosen', () => {
		cy.viewport(390, 844);
		cy.get('#directory-toggle').click();
		cy.get('[role="dialog"] a[href="/albums"]').click();
		cy.location('pathname').should('eq', '/albums');
		cy.get('#directory-toggle').should('have.attr', 'aria-expanded', 'false');
	});

	it('navigates to settings page', () => {
		cy.get('.workspace-header button[data-popover-trigger]').click();
		cy.get('[role=dialog] a[href="/settings"]').click();
		cy.url().should('include', '/settings');
	});

	it('opens the global search from navigation', () => {
		cy.get('button[aria-keyshortcuts]:visible').click();
		cy.get('[role="dialog"] input[type="search"]').should('be.focused');
	});

	it('can dismiss and restore the idle desktop queue from the player', () => {
		cy.get('#workspace-queue h2').should('have.text', 'Queue');
		cy.get('#workspace-queue [data-close-queue]').click();
		cy.get('[data-dock-queue]').should('be.focused').click();
		cy.get('#workspace-queue [data-close-queue]').should('be.focused');
		cy.press(Cypress.Keyboard.Keys.ESC);
		cy.get('[data-dock-queue]').should('be.focused').and('have.attr', 'aria-expanded', 'false');
	});

	it('opens the queue on mobile before choosing a recording', () => {
		cy.viewport(390, 844);
		cy.get('.workspace-header .mobile-queue').click();
		cy.get('#workspace-queue[role=dialog] [data-close-queue]').should('be.visible').click();
		cy.get('.workspace-header .mobile-queue').should('be.focused');
	});

	it('keeps the selected preview in the player when navigating', () => {
		const feeds = ['tracks/popular', 'tracks/random', 'tracks/recently-added', 'albums/recent', 'genres', 'artists/top', 'editorial-playlists', 'stats/overview'];
		for (const feed of feeds) cy.intercept('GET', '**/api/' + feed + '*', { statusCode: 503 });
		cy.reload();
		cy.get('[data-testid="idle-player"]').should('be.visible');
		cy.contains('main button', 'Listen to the offline demo').click();
		cy.contains('section[aria-label="After Hours"] button', 'Listen now').click();
		cy.get('[data-testid="audio-player"]').should('contain', 'Last Light');
		cy.get('a[href="/explore"]:visible').first().click();
		cy.get('[data-testid="audio-player"]').should('contain', 'Last Light');
	});
});
