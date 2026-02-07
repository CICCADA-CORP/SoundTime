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

	it('navigates to library page', () => {
		cy.get('a[href="/library"]').first().click();
		cy.url().should('include', '/library');
	});

	it('navigates to playlists page', () => {
		cy.get('a[href="/playlists"]').first().click();
		cy.url().should('include', '/playlists');
	});

	it('navigates to favorites page', () => {
		cy.get('a[href="/favorites"]').first().click();
		cy.url().should('include', '/favorites');
	});

	it('navigates to history page', () => {
		cy.get('a[href="/history"]').first().click();
		cy.url().should('include', '/history');
	});

	it('navigates to settings page', () => {
		cy.get('a[href="/settings"]').first().click();
		cy.url().should('include', '/settings');
	});

	it('search bar is visible', () => {
		cy.get('input[type="search"], input[placeholder*="search"], input[placeholder*="Search"], input[placeholder*="Rechercher"]').should('exist');
	});

	it('audio player is present in layout', () => {
		cy.get('[data-testid="audio-player"], .audio-player, audio').should('exist');
	});
});
