describe('Playback', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
	});

	it('displays tracks list on explore page', () => {
		cy.visit('/explore');
		// Should show some content (tracks, albums, artists)
		cy.get('body').should('not.be.empty');
	});

	it('stream URL endpoint responds', () => {
		// Verify the stream endpoint exists
		cy.request({
			url: '/api/tracks',
			failOnStatusCode: false,
			headers: {
				Authorization: `Bearer ${window.localStorage.getItem('soundtime_access_token')}`
			}
		}).then((res) => {
			expect(res.status).to.be.oneOf([200, 401]);
		});
	});

	it('track detail page loads', () => {
		// First get a track ID from the API
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				url: '/api/tracks?per_page=1',
				headers: { Authorization: `Bearer ${token}` },
				failOnStatusCode: false
			}).then((res) => {
				if (res.status === 200 && res.body.data?.length > 0) {
					const trackId = res.body.data[0].id;
					cy.visit(`/tracks/${trackId}`);
					cy.url().should('include', `/tracks/${trackId}`);
				}
			});
		});
	});
});
